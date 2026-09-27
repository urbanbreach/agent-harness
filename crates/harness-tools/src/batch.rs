use harness_core::tool::{Tool, ToolCapability, ToolContext, ToolError, ToolResult};
use serde::Deserialize;
use serde_json::{json, Value};

pub(crate) struct BatchTool;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    tool_calls: Vec<Call>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Call {
    tool: String,
    #[serde(alias = "args", alias = "arguments")]
    parameters: Value,
}
#[async_trait::async_trait]
impl Tool for BatchTool {
    fn id(&self) -> &str {
        "batch"
    }
    fn description(&self) -> &str {
        "Run up to 25 independent tools concurrently. Results follow input order. Nested batches are rejected; each call keeps its own permissions."
    }
    fn capability(&self) -> ToolCapability {
        ToolCapability::SpawnAgent
    }
    fn parameters_json_schema(&self) -> Value {
        json!({"type":"object","properties":{"tool_calls":{"type":"array","minItems":1,"maxItems":25,"items":{"type":"object","properties":{"tool":{"type":"string"},"parameters":{"type":"object"}},"required":["tool","parameters"],"additionalProperties":false}}},"required":["tool_calls"],"additionalProperties":false})
    }
    async fn call(&self, ctx: ToolContext, args: Value) -> Result<ToolResult, ToolError> {
        let args: Args =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
        if args.tool_calls.is_empty() || args.tool_calls.len() > 25 {
            return Err(ToolError::InvalidArguments(
                "batch requires 1–25 calls".into(),
            ));
        }
        let count = args.tool_calls.len();
        let mut jobs = tokio::task::JoinSet::new();
        for (index, call) in args.tool_calls.into_iter().enumerate() {
            let handle = ctx.coordinator.clone();
            let parent = ctx.tool_call_id.to_string();
            jobs.spawn(async move {
                let result = handle
                    .execute_nested_tool(parent, call.tool.clone(), call.parameters)
                    .await;
                (index, call.tool, result)
            });
        }
        let mut details = vec![Value::Null; count];
        let mut media = vec![Vec::new(); count];
        let mut artifacts = Vec::new();
        let (mut media_count, mut media_bytes, mut media_overflow) = (0usize, 0u64, false);
        let mut successful = 0;
        while let Some(result) = jobs.join_next().await {
            let (index, tool, result) =
                result.map_err(|_| ToolError::Execution("batch worker stopped".into()))?;
            details[index] = match result {
                Ok(output) => {
                    let failed = output.is_error();
                    successful += usize::from(!failed);
                    if !media_overflow {
                        media_count += output.attachments.len();
                        media_bytes += output.attachments.iter().map(|a| a.size).sum::<u64>();
                        media_overflow = media_count
                            > harness_core::attachment_transport::MAX_ATTACHMENTS
                            || media_bytes
                                > harness_core::attachment_transport::MAX_REQUEST_ATTACHMENT_BYTES
                                    as u64;
                        if media_overflow {
                            media.clear();
                        } else {
                            media[index] = output.attachments;
                        }
                    }
                    artifacts.extend(output.artifacts.iter().cloned());
                    json!({"tool":tool,"status":if failed {"failed"} else {"succeeded"},"output":output.display_text,"structured_json":output.structured_json,"artifacts":output.artifacts})
                }
                Err(error) => json!({"tool":tool,"status":"failed","output":error.to_string()}),
            };
        }
        if ctx.cancellation.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        if media_overflow {
            return Err(ToolError::Execution(
                "batch exceeds the attachment limit; read fewer files".into(),
            ));
        }
        let failed = count - successful;
        let mut text = format!("{successful} succeeded, {failed} failed");
        for (index, detail) in details.iter().enumerate() {
            text.push_str(&format!(
                "\n\n{}. {} ({}):\n{}",
                index + 1,
                detail["tool"].as_str().unwrap_or_default(),
                detail["status"].as_str().unwrap_or_default(),
                detail["output"].as_str().unwrap_or_default()
            ));
        }
        artifacts.sort_by(|a, b| a.path.cmp(&b.path));
        artifacts.dedup();
        Ok(ToolResult::structured_with_artifacts(
            text,
            json!({"successful":successful,"failed":failed,"is_error":failed > 0,"details":details}),
            artifacts,
        ).with_attachments(media.into_iter().flatten().collect()))
    }
}
