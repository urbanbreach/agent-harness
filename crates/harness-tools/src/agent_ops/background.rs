// allow: SIZE_OK — agent operations (task delegation + control plane)
use harness_core::coord::CoordinatorError;
use harness_core::event::{EventEnvelopeV1, EventV1};
use harness_core::proj::{
    resolve_all_background_request_refs, BackgroundRequestProjection, BackgroundToolCallCounts,
};
use harness_core::redact::{DefaultRedactor, Redactor};
use harness_core::tool::{ToolContext, ToolError, ToolResult};
use harness_core::ToolResultExt;
use serde_json::{json, Value};
use tokio_stream::StreamExt;

use super::child_metadata::{
    cap_optional_child_summary, child_runtime_metadata, map_replay_stream_error, replay_events,
    ChildRuntimeMetadata, ChildSummary, ChildToolCallCounts,
};
use crate::text_json_tool_result;

const MAX_BACKGROUND_OUTPUT_TIMEOUT_MS: u64 = 300_000;

#[derive(Debug, Clone)]
pub(crate) struct BackgroundOutputRequest {
    pub(crate) task_id: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) request_id: Option<String>,
    pub(crate) block: bool,
    pub(crate) timeout_ms: u64,
    pub(crate) cancel: bool,
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct BackgroundCancelRequest {
    pub(crate) request_id: String,
    pub(crate) reason: Option<String>,
}

#[derive(Debug, Clone)]
struct BackgroundRequestSummary {
    request_id: String,
    session_id: Option<String>,
    scheduler_task_id: Option<String>,
    status: String,
    terminal: bool,
    duration_ms: Option<u64>,
    result_summary: Option<String>,
    failure_summary: Option<String>,
    child_summary: Option<ChildSummary>,
    tool_calls: ChildToolCallCounts,
    late_result: bool,
    cancel_requested: bool,
    cancel_performed: bool,
    cancel_reason: Option<String>,
}

fn background_summary_from_projection(
    projection: BackgroundRequestProjection,
) -> BackgroundRequestSummary {
    let (result_summary, result_child_summary) =
        cap_optional_child_summary("result", projection.result_summary);
    let (failure_summary, failure_child_summary) =
        cap_optional_child_summary("failure", projection.failure_summary);
    BackgroundRequestSummary {
        request_id: projection.request_id.to_string(),
        session_id: projection.session_id,
        scheduler_task_id: projection.scheduler_task_id,
        status: projection.status,
        terminal: projection.terminal,
        duration_ms: projection.duration_ms,
        result_summary,
        failure_summary,
        child_summary: result_child_summary.or(failure_child_summary),
        tool_calls: child_tool_call_counts_from_projection(projection.tool_calls),
        late_result: projection.late_result,
        cancel_requested: false,
        cancel_performed: false,
        cancel_reason: projection.cancel_reason,
    }
}

fn child_tool_call_counts_from_projection(counts: BackgroundToolCallCounts) -> ChildToolCallCounts {
    ChildToolCallCounts {
        requested: counts.requested,
        succeeded: counts.succeeded,
        failed: counts.failed,
    }
}

pub(super) async fn background_output(
    ctx: &ToolContext,
    request: BackgroundOutputRequest,
) -> Result<ToolResult, ToolError> {
    if request.timeout_ms > MAX_BACKGROUND_OUTPUT_TIMEOUT_MS {
        return Err(ToolError::InvalidArguments(format!(
            "background_output timeout must be <= {MAX_BACKGROUND_OUTPUT_TIMEOUT_MS} ms"
        )));
    }
    let request_id = trimmed_selector(request.request_id.as_deref()).map(str::to_string);
    let selector_hint = trimmed_selector(request.session_id.as_deref())
        .or_else(|| trimmed_selector(request.task_id.as_deref()))
        .map(str::to_string);
    let mut summary = background_summary_from_projection(
        ctx.coordinator
            .background_request_projection(
                ctx.actor.clone(),
                request_id.clone(),
                selector_hint.clone(),
            )
            .await
            .map_err(map_background_request_error)?,
    );
    let mut cancel_requested_before_terminal = false;
    if request.cancel && !summary.terminal {
        let reason = sanitize_cancel_reason(
            request
                .reason
                .as_deref()
                .and_then(|reason| trimmed_selector(Some(reason)))
                .unwrap_or("cancelled by background_output"),
        );
        summary = background_summary_from_projection(
            ctx.coordinator
                .cancel_background_request(
                    ctx.actor.clone(),
                    request_id.clone(),
                    selector_hint.clone(),
                    reason.clone(),
                )
                .await
                .map_err(map_background_request_error)?,
        );
        cancel_requested_before_terminal = true;
        if summary.status == "cancelled" {
            summary.cancel_reason = summary.failure_summary.clone().or(Some(reason));
        }
    }
    summary.cancel_requested = request.cancel;
    summary.cancel_performed = cancel_requested_before_terminal && summary.status == "cancelled";
    let mut timed_out = false;
    if request.block && !summary.terminal {
        let scheduler_task_id = summary.scheduler_task_id.clone().ok_or_else(|| {
            ToolError::InvalidArguments(format!(
                "cannot wait for background request `{}` because no scheduler task id was observed yet",
                summary.request_id
            ))
        })?;
        let observed_terminal = ctx
            .coordinator
            .wait_background_request_terminal(
                summary.request_id.clone(),
                scheduler_task_id,
                request.timeout_ms,
            )
            .await
            .map_err(map_background_request_error)?;
        summary = background_summary_from_projection(
            ctx.coordinator
                .background_request_projection(
                    ctx.actor.clone(),
                    request_id.clone(),
                    selector_hint.clone(),
                )
                .await
                .map_err(map_background_request_error)?,
        );
        timed_out = !observed_terminal && !summary.terminal;
    }
    let child_runtime = background_child_runtime_metadata(ctx, &summary).await?;
    let route = background_route_metadata(ctx, &summary.request_id).await?;

    Ok(text_json_tool_result(
        format_background_output(&summary, timed_out),
        json!({
            "request_id": summary.request_id,
            "task_id": summary.session_id,
            "session_id": summary.session_id,
            "scheduler_task_id": summary.scheduler_task_id,
            "status": summary.status,
            "mode": "background",
            "terminal": summary.terminal,
            "block": request.block,
            "timed_out": timed_out,
            "timeout_ms": request.timeout_ms,
            "duration_ms": summary.duration_ms,
            "result_summary": summary.result_summary,
            "failure_summary": summary.failure_summary,
            "child_summary": summary.child_summary,
            "child_tool_call_count": summary.tool_calls.requested,
            "child_tool_call_counts": summary.tool_calls,
            "late_result": summary.late_result,
            "cancel_requested": summary.cancel_requested,
            "cancel_performed": summary.cancel_performed,
            "cancel_reason": summary.cancel_reason,
            "route": route,
            "runtime": child_runtime,
            "child_runtime": child_runtime,
            "next_actions": background_next_actions(&summary),
            "source": "event_replay",
        }),
    ))
}

pub(super) async fn background_cancel(
    ctx: &ToolContext,
    request: BackgroundCancelRequest,
) -> Result<ToolResult, ToolError> {
    let request_id = trimmed_selector(Some(&request.request_id))
        .ok_or_else(|| ToolError::InvalidArguments("request_id is required".to_string()))?
        .to_string();
    let mut summary = background_summary_from_projection(
        ctx.coordinator
            .background_request_projection(ctx.actor.clone(), Some(request_id.clone()), None)
            .await
            .map_err(map_background_request_error)?,
    );
    let previous_status = summary.status.clone();
    let previous_terminal = summary.terminal;
    let reason = sanitize_cancel_reason(
        request
            .reason
            .as_deref()
            .and_then(|reason| trimmed_selector(Some(reason)))
            .unwrap_or("cancelled by background_cancel"),
    );

    if !summary.terminal {
        summary = background_summary_from_projection(
            ctx.coordinator
                .cancel_background_request(
                    ctx.actor.clone(),
                    Some(request_id.clone()),
                    None,
                    reason.clone(),
                )
                .await
                .map_err(map_background_request_error)?,
        );
        summary.cancel_requested = true;
        summary.cancel_performed = summary.status == "cancelled";
        if summary.status == "cancelled" {
            summary.cancel_reason = summary.failure_summary.clone().or(Some(reason.clone()));
        }
    } else {
        summary.cancel_requested = true;
        summary.cancel_performed = false;
    }

    let child_runtime = background_child_runtime_metadata(ctx, &summary).await?;
    let route = background_route_metadata(ctx, &summary.request_id).await?;
    let output_cancel_reason = if summary.cancel_performed {
        summary.cancel_reason.clone().or(Some(reason))
    } else {
        summary.cancel_reason.clone()
    };

    Ok(text_json_tool_result(
        format_background_cancel(&summary, &previous_status),
        json!({
            "request_id": summary.request_id,
            "task_id": summary.session_id,
            "session_id": summary.session_id,
            "scheduler_task_id": summary.scheduler_task_id,
            "previous_status": previous_status,
            "previous_terminal": previous_terminal,
            "final_status": summary.status,
            "status": summary.status,
            "terminal": summary.terminal,
            "cancel_requested": summary.cancel_requested,
            "cancel_performed": summary.cancel_performed,
            "cancel_reason": output_cancel_reason,
            "duration_ms": summary.duration_ms,
            "result_summary": summary.result_summary,
            "failure_summary": summary.failure_summary,
            "late_result": summary.late_result,
            "route": route,
            "runtime": child_runtime,
            "child_runtime": child_runtime,
            "next_actions": background_next_actions(&summary),
            "source": "event_replay",
        }),
    ))
}

pub(super) async fn cancel_all_background_tasks(
    ctx: &ToolContext,
    reason: Option<String>,
) -> Result<ToolResult, ToolError> {
    let cancel_reason = sanitize_cancel_reason(
        reason
            .as_deref()
            .and_then(|r| trimmed_selector(Some(r)))
            .unwrap_or("cancelled by background_cancel all"),
    );

    let mut replay = replay_events(ctx).await?;
    let mut events: Vec<EventEnvelopeV1> = Vec::new();
    while let Some(next) = replay.next().await {
        events.push(next.map_err(map_replay_stream_error)?);
    }

    let refs = resolve_all_background_request_refs(&events, &ctx.actor);

    let mut cancelled = Vec::new();
    let mut skipped = Vec::new();

    for request_ref in &refs {
        let projection = ctx
            .coordinator
            .background_request_projection(
                ctx.actor.clone(),
                Some(request_ref.request_id.to_string()),
                request_ref.session_id_hint.clone(),
            )
            .await
            .map_err(map_background_request_error)?;

        if projection.terminal {
            skipped.push(json!({
                "request_id": request_ref.request_id,
                "status": projection.status,
                "terminal": true,
            }));
            continue;
        }

        let projection = ctx
            .coordinator
            .cancel_background_request(
                ctx.actor.clone(),
                Some(request_ref.request_id.to_string()),
                request_ref.session_id_hint.clone(),
                cancel_reason.clone(),
            )
            .await
            .map_err(map_background_request_error)?;

        cancelled.push(json!({
            "request_id": request_ref.request_id,
            "status": projection.status,
            "terminal": projection.terminal,
        }));
    }

    let cancelled_count = cancelled.len();
    let skipped_count = skipped.len();

    Ok(text_json_tool_result(
        format!(
            "Bulk cancellation requested for {cancelled_count} background task(s); {skipped_count} already terminal."
        ),
        json!({
            "all": true,
            "cancelled": cancelled,
            "skipped": skipped,
            "cancelled_count": cancelled_count,
            "skipped_count": skipped_count,
            "cancel_reason": cancel_reason,
            "source": "event_replay",
        }),
    ))
}

fn map_background_request_error(err: CoordinatorError) -> ToolError {
    match err {
        CoordinatorError::UnknownTask(message)
        | CoordinatorError::PermissionDenied(message)
        | CoordinatorError::PolicyViolation(message) => ToolError::InvalidArguments(message),
        other => ToolError::Execution(format!("failed to inspect background request: {other}")),
    }
}

async fn background_child_runtime_metadata(
    ctx: &ToolContext,
    summary: &BackgroundRequestSummary,
) -> Result<Option<ChildRuntimeMetadata>, ToolError> {
    let Some(session_id) = summary.session_id.as_ref() else {
        return Ok(None);
    };
    let runtime = ctx
        .coordinator
        .agent_runtime_info(session_id.clone())
        .await
        .tool_err("failed to inspect child runtime")?;
    Ok(Some(child_runtime_metadata(&runtime)))
}

async fn background_route_metadata(
    ctx: &ToolContext,
    request_id: &str,
) -> Result<Option<Value>, ToolError> {
    let mut replay = replay_events(ctx).await?;
    while let Some(next) = replay.next().await {
        let event = next.map_err(map_replay_stream_error)?;
        let EventV1::ToolCallFinished(data) = &event.payload else {
            continue;
        };
        let Some(output_json) = data.output_json.as_ref() else {
            continue;
        };
        if output_child_request_id(output_json).as_deref() == Some(request_id) {
            return Ok(output_json.get("route").cloned());
        }
    }
    Ok(None)
}

fn output_child_request_id(output_json: &Value) -> Option<String> {
    ["child_request_id", "request_id"]
        .into_iter()
        .find_map(|key| output_json.get(key).and_then(Value::as_str))
        .map(str::to_string)
        .or_else(|| {
            output_json
                .get("_harness")
                .and_then(|harness| harness.get("lineage"))
                .and_then(|lineage| lineage.get("child_request_id"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn trimmed_selector(selector: Option<&str>) -> Option<&str> {
    selector.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    })
}

fn format_background_output(summary: &BackgroundRequestSummary, timed_out: bool) -> String {
    let task_id = summary.session_id.as_deref().unwrap_or("<unknown>");
    let duration = summary
        .duration_ms
        .map(|duration| format!("\nDuration: {duration} ms"))
        .unwrap_or_default();
    let timeout_note = if timed_out {
        "\nTimed out waiting for completion; the child task was not cancelled."
    } else {
        ""
    };
    let cancel_note = if summary.cancel_performed {
        "\nCancellation requested through the coordinator."
    } else if summary.cancel_requested && summary.terminal {
        "\nCancellation was requested after the child task was already terminal."
    } else {
        ""
    };
    let body = match summary.status.as_str() {
        "completed" => summary
            .result_summary
            .as_deref()
            .unwrap_or("Background task completed without a result summary."),
        "cancelled" => summary
            .failure_summary
            .as_deref()
            .unwrap_or("Background task was cancelled without a reason."),
        "running" => "Background task is still running.",
        "queued" => "Background task is queued and has not started yet.",
        "not_found" => "No events were found for this background request.",
        _ => "Background task has events but no terminal result yet.",
    };

    format!(
        "Task Result\n\nTask ID: {task_id}\nRequest ID: {}\nStatus: {}{}{}{}\n\n---\n\n{}",
        summary.request_id, summary.status, duration, timeout_note, cancel_note, body
    )
}

fn format_background_cancel(summary: &BackgroundRequestSummary, previous_status: &str) -> String {
    if summary.cancel_performed {
        format!(
            "Cancellation requested through the coordinator for request {} ({} -> {}).",
            summary.request_id, previous_status, summary.status
        )
    } else if summary.terminal {
        format!(
            "No cancellation performed for request {} because it is already terminal (status: {}).",
            summary.request_id, summary.status
        )
    } else {
        format!(
            "Cancellation requested for request {}, but the projected status is {}.",
            summary.request_id, summary.status
        )
    }
}

fn background_next_actions(summary: &BackgroundRequestSummary) -> Value {
    let mut actions = Vec::new();
    actions.push(json!({
        "action": "check_status",
        "tool": "background_output",
        "parameters": { "request_id": summary.request_id, "block": false },
    }));
    if !summary.terminal {
        actions.push(json!({
            "action": "wait_for_result",
            "tool": "background_output",
            "parameters": { "request_id": summary.request_id, "block": true },
        }));
        actions.push(json!({
            "action": "cancel",
            "tool": "background_cancel",
            "parameters": {
                "request_id": summary.request_id,
                "reason": "cancelled by parent request"
            },
        }));
        actions.push(json!({
            "action": "cancel_compat",
            "tool": "background_output",
            "parameters": {
                "request_id": summary.request_id,
                "cancel": true,
                "reason": "cancelled by parent request"
            },
        }));
    }
    Value::Array(actions)
}

fn sanitize_cancel_reason(reason: &str) -> String {
    const MAX_CANCEL_REASON_CHARS: usize = 512;
    let redacted = DefaultRedactor::default().redact_text(reason.trim());
    let mut capped = redacted
        .chars()
        .take(MAX_CANCEL_REASON_CHARS)
        .collect::<String>();
    if redacted.chars().count() > MAX_CANCEL_REASON_CHARS {
        capped.push('…');
    }
    if capped.is_empty() {
        "cancelled by background_cancel".to_string()
    } else {
        capped
    }
}
