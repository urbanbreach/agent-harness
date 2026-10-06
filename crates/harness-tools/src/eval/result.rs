use super::*;
use base64::Engine;
use std::io::Read;

pub(super) fn text(result: &Value) -> String {
    result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) async fn convert(
    ctx: &ToolContext,
    host: &host::Host,
    result: Value,
) -> Result<ToolResult, ToolError> {
    let mut output = ToolResult::structured(text(&result), result["details"].clone());
    let data = output
        .structured_json
        .as_mut()
        .ok_or_else(|| failure("eval result details missing"))?;
    data["is_error"] = json!(data["isError"] == true);
    data["cell_id"] = data["cell_id"]
        .as_str()
        .unwrap_or(ctx.tool_call_id.as_str())
        .to_owned()
        .into();
    if let Some(content) = result["content"].as_array() {
        let notice = content
            .iter()
            .skip(1)
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if !notice.is_empty() {
            data["memory_notice"] = notice.into();
        }
        for (index, part) in content.iter().filter(|p| p["type"] == "image").enumerate() {
            if index >= harness_core::attachment_transport::MAX_ATTACHMENTS {
                return Err(failure("eval exceeds the image attachment limit"));
            }
            let bytes = base64::prelude::BASE64_STANDARD
                .decode(
                    part["data"]
                        .as_str()
                        .ok_or_else(|| failure("eval image data missing"))?,
                )
                .map_err(failure)?;
            let id = format!("{}-image-{index}", ctx.tool_call_id);
            let attachment = tokio::task::spawn_blocking(move || images::attachment(id, bytes))
                .await
                .map_err(failure)??;
            output.attachments.push(attachment);
        }
    }
    // Only the engine's output metadata names a spill. User JSON and nested
    // tool results may legitimately contain fields with the same name.
    if let Some(path) = data["meta"]["artifactId"].as_str().map(str::to_owned) {
        let canonical = std::fs::canonicalize(&path)?;
        if !canonical.starts_with(host.scratch.path().join("artifacts")) {
            return Err(failure("eval artifact is outside its private directory"));
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&canonical)?
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(failure("eval artifact exceeds 8 MiB"));
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let artifact = ctx
            .coordinator
            .retain_eval_output(ctx.tool_call_id.to_string(), text)
            .await
            .map_err(failure)?;
        let file_name = std::path::Path::new(&artifact.path)
            .file_name()
            .ok_or_else(|| failure("invalid retained eval artifact"))?;
        let readable = ctx
            .artifacts_dir
            .join(file_name)
            .canonicalize()?
            .to_string_lossy()
            .into_owned();
        output.display_text = output.display_text.replace(&path, &readable);
        replace_path(data, &path, &readable);
        output.artifacts.push(artifact);
    }
    Ok(output)
}

fn replace_path(data: &mut Value, old: &str, new: &str) {
    match data {
        Value::String(text) => *text = text.replace(old, new),
        Value::Object(object) => {
            for value in object.values_mut() {
                replace_path(value, old, new);
            }
        }
        Value::Array(values) => {
            for value in values {
                replace_path(value, old, new);
            }
        }
        _ => {}
    }
}

pub(super) async fn dispatch(
    ctx: &ToolContext,
    session: &Arc<harness_eval::Session>,
    event: Value,
    cancellation: CancellationToken,
) -> Result<(), ToolError> {
    let response = match event["operation"].as_str() {
        Some("tool") => {
            let name = event["args"]["name"].as_str().unwrap_or_default();
            let mut args = event["args"]["parameters"].clone();
            if name == "spawn_subagent" {
                kernel_tools::publish(ctx, session, &args).await?;
                adapt_agent(&mut args);
            }
            let child = format!(
                "{}-eval-{}",
                ctx.tool_call_id,
                event["id"].as_str().unwrap_or_default()
            );
            ctx.coordinator.execute_nested_tool_with_id(ctx.tool_call_id.to_string(), name, args, Some(child), Some(cancellation)).await.and_then(|result| {
                let mut content = vec![json!({"type":"text","text":result.display_text})];
                for attachment in &result.attachments {
                    let bytes = attachment.bytes().map_err(|e| harness_core::coord::CoordinatorError::Invalid(e.to_string()))?;
                    content.push(json!({"type":"image","data":base64::prelude::BASE64_STANDARD.encode(bytes),"mimeType":attachment.mime}));
                }
                let mut details = result.structured_json.unwrap_or_else(|| json!({}));
                if !details.is_object() { details = json!({"value":details}); }
                if name == "spawn_subagent" && details["subagent_id"].is_string() {
                    details["task_id"] = details["subagent_id"].clone();
                    details["run_epoch"] = json!(0);
                    if let Some(text) = details["output"].as_str() {
                        content[0] = json!({"type":"text","text":text});
                    }
                }
                details["isError"] = json!(details["is_error"] == true);
                Ok(json!({"content":content,"details":details}))
            }).map_err(|e| e.to_string())
        }
        Some("workpool") => {
            let args = &event["args"];
            if args["op"] == "create" {
                let grant = if args.get("tools").is_some() {
                    json!({"tools":args["tools"]})
                } else {
                    args["agent"].clone()
                };
                kernel_tools::publish(ctx, session, &grant).await?;
            }
            ctx.coordinator.eval_workpool(ctx.tool_call_id.to_string(), args.clone(), cancellation).await
                .map(|details| json!({"content":[{"type":"text","text":details.to_string()}],"details":details}))
                .map_err(|error| error.to_string())
        }
        Some("completion") => ctx
            .coordinator
            .eval_completion(
                ctx.tool_call_id.to_string(),
                event["args"].clone(),
                cancellation,
            )
            .await
            .map_err(|e| e.to_string()),
        _ => Err("unknown eval host operation".into()),
    };
    let mut reply = match response {
        Ok(result) => json!({"type":"reply","id":event["id"],"result":result}),
        Err(error) => json!({"type":"reply","id":event["id"],"error":error}),
    };
    // Discovery can publish new MCP tools within this very cell. Refresh the
    // bridge catalog so tool_schema() and error hints see those descriptors.
    if event["operation"] == "tool"
        && event["args"]["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("mcp."))
    {
        reply["tools"] = ctx
            .coordinator
            .eval_tool_catalog(ctx.tool_call_id.to_string())
            .await
            .map_err(failure)?;
    }
    session.send(reply).await.map_err(failure)
}

fn adapt_agent(args: &mut Value) {
    let Some(object) = args.as_object_mut() else {
        return;
    };
    if let Some(background) = object.remove("run_in_background") {
        object.insert("background".into(), background);
    }
    if let Some(label) = object.remove("name") {
        object.insert("description".into(), label);
    }
    object
        .entry("description")
        .or_insert(json!("Eval subagent"));
}
