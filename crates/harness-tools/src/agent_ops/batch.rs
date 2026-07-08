use harness_core::tool::{canonical_tool_id_for, ToolContext, ToolError, ToolResult};
use harness_core::ToolResultExt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::task::JoinSet;

use crate::text_json_tool_result;

const MAX_BATCH_CALLS: usize = 25;
const BATCH_NESTED_ERROR: &str = "batch cannot be nested inside batch";
const BATCH_MAX_CALLS_ERROR: &str = "Maximum of 25 tools allowed in batch";

#[derive(Debug, Deserialize, Serialize, JsonSchema, Clone)]
#[serde(deny_unknown_fields)]
pub(crate) struct BatchCall {
    pub(crate) tool: String,
    #[serde(alias = "args", alias = "arguments")]
    pub(crate) parameters: Value,
}

#[derive(Debug)]
struct BatchCallOutcome {
    index: usize,
    tool_id: String,
    canonical_tool_id: Option<String>,
    parameters: Value,
    result: Result<ToolResult, String>,
}

pub(super) async fn execute_batch(
    ctx: &ToolContext,
    calls: Vec<BatchCall>,
) -> Result<ToolResult, ToolError> {
    if calls.is_empty() {
        return Err(ToolError::InvalidArguments(
            "Provide at least one tool call".to_string(),
        ));
    }

    let requested_call_count = calls.len();
    let mut join_set = JoinSet::new();

    let mut outcomes = Vec::new();
    for (index, call) in calls.into_iter().enumerate() {
        let tool_id = call.tool;
        let parameters = call.parameters;
        if index >= MAX_BATCH_CALLS {
            let canonical_tool_id = canonical_tool_id_for(&tool_id).map(str::to_string);
            outcomes.push(BatchCallOutcome {
                index,
                tool_id,
                canonical_tool_id,
                parameters,
                result: Err(BATCH_MAX_CALLS_ERROR.to_string()),
            });
            continue;
        }

        let canonical_tool_id = canonical_tool_id_for(&tool_id).map(str::to_string);
        if canonical_tool_id.as_deref() == Some("batch") {
            outcomes.push(BatchCallOutcome {
                index,
                tool_id,
                canonical_tool_id,
                parameters,
                result: Err(BATCH_NESTED_ERROR.to_string()),
            });
            continue;
        }

        let coordinator = ctx.coordinator.clone();
        let actor = ctx.actor.clone();
        let category = ctx.category.clone();
        join_set.spawn(async move {
            let execution_parameters = parameters.clone();
            let result = coordinator
                .execute_agent_tool_call(actor, category, tool_id.clone(), execution_parameters)
                .await;
            BatchCallOutcome {
                index,
                tool_id,
                canonical_tool_id,
                parameters,
                result,
            }
        });
    }

    while let Some(joined) = join_set.join_next().await {
        outcomes
            .push(joined.tool_err("batch join failed")?);
    }

    outcomes.sort_by_key(|outcome| outcome.index);

    let successful = outcomes
        .iter()
        .filter(|outcome| outcome.result.is_ok())
        .count();
    let failed = outcomes.len().saturating_sub(successful);

    let details = outcomes
        .into_iter()
        .map(batch_detail_json)
        .collect::<Vec<_>>();
    let display_text = format_batch_display(successful, failed, &details);

    Ok(text_json_tool_result(
        display_text,
        json!({
            "successful": successful,
            "failed": failed,
            "requested_call_count": requested_call_count,
            "processed_call_count": details.len(),
            "discarded_call_count": requested_call_count.saturating_sub(MAX_BATCH_CALLS),
            "max_calls": MAX_BATCH_CALLS,
            "execution": {
                "concurrency": "parallel",
                "result_order": "input",
                "nested_batch_disallowed": true,
            },
            "audit": {
                "successful": successful,
                "failed": failed,
                "requested_call_count": requested_call_count,
                "processed_call_count": details.len(),
                "discarded_call_count": requested_call_count.saturating_sub(MAX_BATCH_CALLS),
            },
            "details": details,
        }),
    ))
}

fn format_batch_display(successful: usize, failed: usize, details: &[Value]) -> String {
    let mut lines = Vec::with_capacity(details.len() + 4);
    lines.push(if failed == 0 {
        format!("All {successful} tools executed successfully.")
    } else {
        format!("Executed {successful} tools successfully. {failed} failed.")
    });
    lines.push("Batch results (input order):".to_string());
    for detail in details {
        let index = detail
            .get("index")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        let tool_id = detail
            .get("tool_id")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let status = detail
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        lines.push(format!("[{index}] {tool_id}: {status}"));
    }
    lines.push(
        "Permission attribution: each child tool call uses its own coordinator permission check."
            .to_string(),
    );
    lines.join("\n")
}

fn batch_detail_json(outcome: BatchCallOutcome) -> Value {
    let BatchCallOutcome {
        index,
        tool_id,
        canonical_tool_id,
        parameters,
        result,
    } = outcome;

    match result {
        Ok(value) => {
            let result = json!({
                "success": true,
                "status": "succeeded",
                "summary": value.display_text,
                "structured_output": value.structured_json,
                "artifacts": value.artifacts,
            });

            json!({
                "index": index,
                "tool_id": &tool_id,
                "canonical_tool_id": &canonical_tool_id,
                "request": batch_request_json(&tool_id, &canonical_tool_id, &parameters),
                "success": true,
                "status": "succeeded",
                "summary": result.get("summary").cloned(),
                "structured_output": result.get("structured_output").cloned(),
                "artifacts": result.get("artifacts").cloned(),
                "result": result,
            })
        }
        Err(error) => {
            let result = json!({
                "success": false,
                "status": "failed",
                "error": error,
            });

            json!({
                "index": index,
                "tool_id": &tool_id,
                "canonical_tool_id": &canonical_tool_id,
                "request": batch_request_json(&tool_id, &canonical_tool_id, &parameters),
                "success": false,
                "status": "failed",
                "error": result.get("error").cloned(),
                "result": result,
            })
        }
    }
}

fn batch_request_json(
    tool_id: &str,
    canonical_tool_id: &Option<String>,
    parameters: &Value,
) -> Value {
    json!({
        "tool_id": tool_id,
        "canonical_tool_id": canonical_tool_id,
        "parameter_shape": parameter_shape(parameters),
        "parameter_keys": parameter_keys(parameters),
        "parameters_redacted": true,
    })
}

fn parameter_shape(parameters: &Value) -> &'static str {
    match parameters {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn parameter_keys(parameters: &Value) -> Vec<String> {
    parameters
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default()
}
