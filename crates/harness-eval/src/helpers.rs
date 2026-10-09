use crate::{cell::Cell, session::Inner, Result};
use serde_json::{json, Value};
use std::time::Instant;
mod handles;

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
struct HelperError {
    code: String,
    message: String,
}

fn failure(code: &str, message: impl Into<String>) -> crate::Error {
    Box::new(HelperError {
        code: code.into(),
        message: message.into(),
    })
}

pub(crate) async fn reply(session: &Inner, cell: &Cell, event: &Value) -> Value {
    let mut reply = match dispatch(session, cell, event).await {
        Ok(result) => json!({"type":"reply","id":event["id"],"result":result}),
        Err(error) => {
            let value = error.downcast_ref::<HelperError>().map_or_else(
                || json!(error.to_string()),
                |error| json!({"code":error.code,"message":error.message}),
            );
            json!({"type":"reply","id":event["id"],"error":value})
        }
    };
    if cell
        .tools_changed
        .swap(false, std::sync::atomic::Ordering::AcqRel)
    {
        reply["tools"] = cell.tools.lock().await.clone();
    }
    reply
}

pub(crate) async fn dispatch(session: &Inner, cell: &Cell, event: &Value) -> Result<Value> {
    let args = &event["args"];
    match event["operation"].as_str() {
        Some("schema") => {
            keys(args, &["name"])?;
            let tools = cell.tools.lock().await;
            let tools = tools.as_array().ok_or("tool catalog is unavailable")?;
            if let Some(name) = args.get("name") {
                let name = name
                    .as_str()
                    .filter(|name| !name.is_empty())
                    .ok_or("tool_schema() name must be nonempty")?;
                if let Some(tool) = tools.iter().find(|tool| tool["name"] == name) {
                    return Ok(
                        json!({"name":name,"description":tool["description"],"parameters":tool["parameters"]}),
                    );
                }
                let names = tools
                    .iter()
                    .filter_map(|tool| tool["name"].as_str())
                    .filter(|candidate| {
                        candidate.to_lowercase().contains(&name.to_lowercase())
                            || name.to_lowercase().contains(&candidate.to_lowercase())
                    })
                    .take(5)
                    .collect::<Vec<_>>();
                Err(format!(
                    "tool_schema() found no tool named {name:?}.{}",
                    if names.is_empty() {
                        String::new()
                    } else {
                        format!(" Did you mean: {}?", names.join(", "))
                    }
                )
                .into())
            } else {
                Ok(
                    json!({"tools":tools.iter().map(|tool| tool["name"].clone()).collect::<Vec<_>>()}),
                )
            }
        }
        Some("tool") => {
            let name = args["name"].as_str().ok_or("tool name is missing")?;
            let result = tool(session, cell, name, args["parameters"].clone()).await?;
            Ok(marshal(&result))
        }
        Some("completion") => {
            let response = session.call(cell, "completion", args.clone()).await?;
            Ok(response.get("value").unwrap_or(&response["text"]).clone())
        }
        Some("agent") => agent(session, cell, args).await,
        Some("output") => output(session, cell, args).await,
        Some("workpool") => workpool(session, cell, args).await,
        Some("install") => crate::packages::install(session, cell, args).await,
        Some("wait") => handles::wait(session, cell, args).await,
        Some("control") => handles::control(session, cell, args).await,
        _ => Err("unknown eval helper operation".into()),
    }
}

async fn workpool(session: &Inner, cell: &Cell, args: &Value) -> Result<Value> {
    let catalog = cell
        .tools
        .lock()
        .await
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    if !catalog
        .iter()
        .any(|name| name == "workpool" || name == "spawn_subagent")
    {
        return Err(failure(
            "workpool_unavailable",
            "No active host workpool tool",
        ));
    }
    let result = if catalog.iter().any(|name| name == "workpool") {
        tool(session, cell, "workpool", args.clone()).await?
    } else {
        session.call(cell, "workpool", args.clone()).await?
    };
    if args["op"] == "create" {
        let details = &result["details"];
        if let Some(error) = details.get("error") {
            return Err(failure(
                error["code"].as_str().unwrap_or("workpool_unavailable"),
                error["message"]
                    .as_str()
                    .unwrap_or("Workpool creation failed"),
            ));
        }
        let valid = details["pool_id"]
            .as_str()
            .and_then(|id| id.strip_prefix("wp_"))
            .is_some_and(|id| {
                id.len() == 32
                    && id
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            });
        if !valid || details["isError"] == true {
            return Err(failure(
                "workpool_unavailable",
                "Host did not return a workpool identity",
            ));
        }
    }
    Ok(marshal(&result))
}

async fn tool(session: &Inner, cell: &Cell, name: &str, args: Value) -> Result<Value> {
    if name == "eval" {
        return Err("recursive eval is not allowed".into());
    }
    if !cell
        .tools
        .lock()
        .await
        .as_array()
        .into_iter()
        .flatten()
        .any(|tool| tool["name"] == name)
    {
        return Err(
            format!("unknown tool {name:?}; use tool_schema() to list available tools").into(),
        );
    }
    let (index, mut summary) = {
        let mut state = cell.state.lock().await;
        let index = state.tool_calls.count;
        let mut summary = json!({"name":crate::metadata::capped(name,128),"ok":null,"startedAt":crate::cell::epoch_ms()});
        if index < 30 {
            summary["callId"] =
                crate::metadata::capped(&format!("{}-{index}", cell.id), 128).into();
            let (args, truncated) = crate::metadata::arguments(&args);
            if let Some(args) = args {
                summary["args"] = args;
            }
            if truncated {
                summary["argsTruncated"] = true.into();
            }
        }
        let index = state.tool_calls.begin(&summary);
        (index, summary)
    };
    let started = Instant::now();
    let result = session
        .call(cell, "tool", json!({"name":name,"parameters":args}))
        .await;
    summary["durationMs"] = json!(started.elapsed().as_millis());
    match &result {
        Ok(result) => {
            summary["ok"] = json!(result["details"]["isError"] != true);
            let text = result_text(result);
            if index < 30 {
                summary["resultPreview"] = crate::metadata::capped(
                    &text.split_whitespace().collect::<Vec<_>>().join(" "),
                    160,
                )
                .into();
            }
            if result["details"]["isError"] == true {
                summary["error"] = crate::metadata::capped(&text, 512).into();
            }
        }
        Err(error) => {
            summary["ok"] = false.into();
            summary["error"] = crate::metadata::capped(&error.to_string(), 512).into();
        }
    }
    cell.state.lock().await.tool_calls.finish(index, summary);
    result.map_err(|error| format!("{error}\n(see tool_schema({name:?}) for parameters)").into())
}

fn marshal(result: &Value) -> Value {
    let text = result_text(result);
    let images: Vec<_> = result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|part| part["type"] == "image")
        .map(|part| json!({"mimeType":part["mimeType"],"dataBase64":part["data"]}))
        .collect();
    let details = &result["details"];
    let error = details["isError"] == true;
    if images.is_empty() && details.as_object().is_some_and(serde_json::Map::is_empty) && !error {
        return json!({"text":text});
    }
    json!({"text":text,"details":details,"images":images,"hasError":error})
}

fn result_text(result: &Value) -> String {
    result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| part["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

async fn agent(session: &Inner, cell: &Cell, args: &Value) -> Result<Value> {
    keys(
        args,
        &[
            "prompt", "agent", "model", "label", "schema", "handle", "tools", "isolated", "apply",
            "merge",
        ],
    )?;
    let mut prompt = args["prompt"]
        .as_str()
        .filter(|prompt| !prompt.is_empty())
        .ok_or("agent() requires a nonempty prompt")?
        .to_owned();
    for key in ["agent", "model"] {
        if args
            .get(key)
            .is_some_and(|value| value.as_str().is_none_or(str::is_empty))
        {
            return Err(format!("agent() {key} must be nonempty text").into());
        }
    }
    for key in ["handle", "isolated", "apply"] {
        if args.get(key).is_some_and(|value| !value.is_boolean()) {
            return Err(format!("agent() {key} must be a boolean").into());
        }
    }
    if args.get("label").is_some_and(|value| !value.is_string()) {
        return Err("agent() label must be text".into());
    }
    if args.get("tools").is_some_and(|value| {
        value.as_array().is_none_or(|tools| {
            tools
                .iter()
                .any(|tool| tool.as_str().is_none_or(str::is_empty))
        })
    }) {
        return Err("agent() tools must be an array of names".into());
    }
    if args.get("merge").is_some_and(|value| {
        !value.is_boolean() && !matches!(value.as_str(), Some("patch" | "branch"))
    }) {
        return Err("agent() merge must be a boolean, patch or branch".into());
    }
    if let Some(schema) = args.get("schema") {
        prompt.push_str(&format!(
            "\n\nRespond ONLY with JSON matching this JSON-Schema:\n{schema}"
        ));
    }
    let mut parameters = json!({"prompt":prompt,"run_in_background":args["handle"] == true});
    for (source, destination) in [
        ("agent", "subagent_type"),
        ("model", "model"),
        ("label", "name"),
        ("tools", "tools"),
    ] {
        if let Some(value) = args.get(source) {
            parameters[destination] = value.clone();
        }
    }
    let isolation = cell
        .tools
        .lock()
        .await
        .as_array()
        .into_iter()
        .flatten()
        .find(|tool| tool["name"] == "spawn_subagent")
        .is_some_and(|tool| tool["parameters"]["properties"].get("isolated").is_some());
    if isolation {
        for key in ["isolated", "apply", "merge"] {
            if let Some(value) = args.get(key) {
                parameters[key] = match (key, value) {
                    ("merge", Value::Bool(true)) => json!("branch"),
                    ("merge", Value::Bool(false)) => json!("patch"),
                    _ => value.clone(),
                };
            }
        }
    } else if ["isolated", "apply", "merge"]
        .iter()
        .any(|key| args.get(*key).is_some())
    {
        cell.accept(json!({"type":"status","event":{"op":"agent","id":cell.id,"status":"running","warning":"isolated/apply/merge unsupported (no isolation in task engine)"}}), session.options.settings.status_events).await?;
    }
    let result = tool(session, cell, "spawn_subagent", parameters).await?;
    let text = result_text(&result);
    if args["handle"] != true {
        if result["details"]["isolation"]["changes_applied"] == false {
            return Err(failure(
                "isolation_not_applied",
                format!(
                    "agent() isolated changes were not applied: {}",
                    result["details"]["isolation"]
                ),
            ));
        }
        return if args.get("schema").is_some() {
            Ok(serde_json::from_str(&text)?)
        } else {
            Ok(Value::String(text))
        };
    }
    let id = result["details"]["task_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            failure(
                "invalid_task_handle",
                "agent() requires a successful task handle",
            )
        })?;
    let epoch = result["details"]["run_epoch"].as_u64().ok_or_else(|| {
        failure(
            "invalid_task_handle",
            "agent() requires a nonnegative run_epoch",
        )
    })?;
    if result["details"]["isError"] == true || result["details"].get("error").is_some() {
        return Err(failure(
            "invalid_task_handle",
            "agent() requires a successful task handle",
        ));
    }
    let mut value = json!({"text":text,"output":text,"id":id,"handle":format!("agent://{id}"),"run_epoch":epoch,"agent":args["agent"]});
    if args.get("schema").is_some() {
        value["data"] = serde_json::from_str(&text)?;
    }
    if result["details"]["isolation"].is_object() {
        value["details"] = json!({"isolation":result["details"]["isolation"]});
    }
    Ok(value)
}

async fn output(session: &Inner, cell: &Cell, args: &Value) -> Result<Value> {
    keys(args, &["ids", "format", "offset", "limit"])?;
    let ids = args["ids"]
        .as_array()
        .filter(|ids| !ids.is_empty())
        .ok_or("output() requires at least one id")?;
    if ids.iter().any(|id| id.as_str().is_none_or(str::is_empty)) {
        return Err("output() ids must be nonempty strings".into());
    }
    for key in ["offset", "limit"] {
        if args
            .get(key)
            .is_some_and(|value| value.as_u64().is_none_or(|value| value == 0))
        {
            return Err(format!("output() {key} must be a positive integer").into());
        }
    }
    if args
        .get("format")
        .is_some_and(|format| !matches!(format.as_str(), Some("raw" | "tail")))
    {
        return Err("output() format must be raw or tail".into());
    }
    let results = futures_util::future::try_join_all(ids.iter().map(|id| async move {
        let result = tool(
            session,
            cell,
            "get_command_or_subagent_output",
            json!({"task_ids":[id],"timeout":0}),
        )
        .await?;
        let text = result_text(&result).replace("\r\n", "\n");
        let lines: Vec<_> = text.split('\n').collect();
        let start = if args["format"] == "tail" {
            lines.len().saturating_sub(200)
        } else {
            0
        };
        let offset =
            usize::try_from(args["offset"].as_u64().unwrap_or(1) - 1).unwrap_or(usize::MAX);
        let limit =
            usize::try_from(args["limit"].as_u64().unwrap_or(u64::MAX)).unwrap_or(usize::MAX);
        Ok::<_, crate::Error>(Value::String(
            lines
                .into_iter()
                .skip(start.saturating_add(offset))
                .take(limit)
                .collect::<Vec<_>>()
                .join("\n"),
        ))
    }))
    .await?;
    if results.len() == 1 {
        Ok(results.into_iter().next().unwrap_or(Value::Null))
    } else {
        Ok(Value::Array(results))
    }
}

fn keys(value: &Value, allowed: &[&str]) -> Result<()> {
    let object = value
        .as_object()
        .ok_or("helper arguments must be an object")?;
    if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(format!("unexpected helper argument {key:?}").into());
    }
    Ok(())
}
