// allow: SIZE_OK — agent operations (task delegation + control plane)
use harness_core::coord::{
    background_wait_condition_satisfied, BackgroundWaitMode, BackgroundWaitOutcome,
    CoordinatorError,
};
use harness_core::event::{EventEnvelopeV1, EventV1};
use harness_core::proj::{
    resolve_all_background_request_refs, BackgroundRequestProjection, BackgroundToolCallCounts,
};
use harness_core::redact::{DefaultRedactor, Redactor};
use harness_core::tool::{ArtifactRef, ToolContext, ToolError, ToolResult};
use harness_core::ToolResultExt;
use serde_json::{json, Value};
use tokio_stream::StreamExt;

use super::child_metadata::{
    cap_optional_child_summary, child_runtime_metadata, map_replay_stream_error, replay_events,
    ChildRuntimeMetadata, ChildSummary, ChildToolCallCounts,
};
use crate::session_tools::{
    load_child_session_events, summarize_event, MAX_TOOL_INLINE_JSON_CHARS,
};
use crate::{text_json_artifacts_tool_result, text_json_tool_result};

const MAX_BACKGROUND_OUTPUT_TIMEOUT_MS: u64 = 300_000;

#[derive(Debug, Clone)]
pub(crate) struct BackgroundOutputRequest {
    pub(crate) task_id: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) request_id: Option<String>,
    pub(crate) request_ids: Vec<String>,
    pub(crate) wait_mode: Option<String>,
    pub(crate) block: bool,
    pub(crate) timeout_ms: u64,
    pub(crate) cancel: bool,
    pub(crate) reason: Option<String>,
    pub(crate) full_session: bool,
    pub(crate) include_thinking: bool,
    pub(crate) message_limit: Option<u32>,
    pub(crate) since_message_id: Option<String>,
    pub(crate) include_tool_results: bool,
    pub(crate) thinking_max_chars: Option<u32>,
    pub(crate) from_end: bool,
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
    let multi_request_ids = normalize_multi_request_ids(&request);
    if multi_request_ids.len() > 1 {
        return background_output_multi_wait(ctx, &request, multi_request_ids).await;
    }
    let request_id = multi_request_ids
        .into_iter()
        .next()
        .or_else(|| trimmed_selector(request.request_id.as_deref()).map(str::to_string));
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

    let mut artifacts = Vec::new();
    let mut full_session_value = Value::Null;
    let mut thinking_value = Value::Null;

    if request.full_session || request.include_thinking {
        if let Some(session_id) = summary.session_id.as_deref() {
            if let Some(child_events) = load_child_session_events(ctx, session_id)? {
                if request.full_session {
                    let payload = build_full_session_payload(&child_events, &request);
                    let payload_str = serde_json::to_string_pretty(&payload)
                        .tool_err("failed to serialize full_session payload")?;
                    if payload_str.len() > MAX_TOOL_INLINE_JSON_CHARS {
                        let artifact = ctx
                            .artifact_store()
                            .map_err(|err| ToolError::Execution(err.to_string()))?
                            .write_text("background-full-session.json", &payload_str)
                            .map_err(|err| ToolError::Execution(err.to_string()))?;
                        let artifact_ref = ArtifactRef {
                            path: artifact.path,
                            digest: artifact.digest,
                        };
                        artifacts.push(artifact_ref.clone());
                        full_session_value = json!({
                            "spilled": true,
                            "artifact": artifact_ref,
                            "event_count": payload.get("event_count").and_then(Value::as_u64).unwrap_or(0),
                        });
                    } else {
                        full_session_value = payload;
                    }
                }

                if request.include_thinking {
                    if let Some((inline, artifact_ref)) =
                        build_thinking_artifact(ctx, &child_events, &request)?
                    {
                        artifacts.push(artifact_ref);
                        thinking_value = inline;
                    }
                }
            }
        }
    }

    let mut payload = json!({
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
    });

    if !full_session_value.is_null() {
        payload["full_session"] = full_session_value;
    }
    if !thinking_value.is_null() {
        payload["thinking"] = thinking_value;
    }

    if artifacts.is_empty() {
        Ok(text_json_tool_result(
            format_background_output(&summary, timed_out),
            payload,
        ))
    } else {
        Ok(text_json_artifacts_tool_result(
            format_background_output(&summary, timed_out),
            payload,
            artifacts,
        ))
    }
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

fn normalize_multi_request_ids(request: &BackgroundOutputRequest) -> Vec<String> {
    let mut ids = Vec::new();
    for raw in &request.request_ids {
        let Some(id) = trimmed_selector(Some(raw.as_str())) else {
            continue;
        };
        if !ids.iter().any(|existing| existing == id) {
            ids.push(id.to_string());
        }
    }
    if ids.is_empty() {
        if let Some(id) = trimmed_selector(request.request_id.as_deref()) {
            ids.push(id.to_string());
        }
    }
    ids
}

fn parse_wait_mode(raw: Option<&str>) -> Result<BackgroundWaitMode, ToolError> {
    let Some(raw) = trimmed_selector(raw) else {
        return Err(ToolError::InvalidArguments(
            "wait_mode is required when request_ids has more than one entry (any|all)".to_string(),
        ));
    };
    BackgroundWaitMode::parse(raw).ok_or_else(|| {
        ToolError::InvalidArguments(format!("wait_mode must be `any` or `all`, got `{raw}`"))
    })
}

async fn background_output_multi_wait(
    ctx: &ToolContext,
    request: &BackgroundOutputRequest,
    request_ids: Vec<String>,
) -> Result<ToolResult, ToolError> {
    if request.cancel {
        return Err(ToolError::InvalidArguments(
            "background_output cancel is not supported with multi request_ids; use background_cancel"
                .to_string(),
        ));
    }
    if request.full_session || request.include_thinking {
        return Err(ToolError::InvalidArguments(
            "full_session and include_thinking require a single request_id".to_string(),
        ));
    }
    let wait_mode = parse_wait_mode(request.wait_mode.as_deref())?;

    let mut summaries = Vec::with_capacity(request_ids.len());
    for request_id in &request_ids {
        let summary = background_summary_from_projection(
            ctx.coordinator
                .background_request_projection(ctx.actor.clone(), Some(request_id.clone()), None)
                .await
                .map_err(map_background_request_error)?,
        );
        summaries.push(summary);
    }

    let terminal_flags: Vec<(String, bool)> = summaries
        .iter()
        .map(|summary| (summary.request_id.clone(), summary.terminal))
        .collect();
    let mut wait_outcome = BackgroundWaitOutcome {
        satisfied: background_wait_condition_satisfied(wait_mode, &terminal_flags),
        first_terminal_request_id: harness_core::coord::first_terminal_request_id(&terminal_flags),
    };
    let mut timed_out = false;

    if request.block && !wait_outcome.satisfied {
        let mut targets = Vec::with_capacity(summaries.len());
        let mut already_terminal = Vec::new();
        for summary in &summaries {
            if summary.terminal {
                already_terminal.push(summary.request_id.clone());
                continue;
            }
            let scheduler_task_id = summary.scheduler_task_id.clone().ok_or_else(|| {
                ToolError::InvalidArguments(format!(
                    "cannot wait for background request `{}` because no scheduler task id was observed yet",
                    summary.request_id
                ))
            })?;
            targets.push((summary.request_id.clone(), scheduler_task_id));
        }
        wait_outcome = ctx
            .coordinator
            .wait_background_requests_terminal(
                &targets,
                wait_mode,
                &already_terminal,
                request.timeout_ms,
            )
            .await
            .map_err(map_background_request_error)?;

        summaries.clear();
        for request_id in &request_ids {
            let summary = background_summary_from_projection(
                ctx.coordinator
                    .background_request_projection(
                        ctx.actor.clone(),
                        Some(request_id.clone()),
                        None,
                    )
                    .await
                    .map_err(map_background_request_error)?,
            );
            summaries.push(summary);
        }
        let terminal_flags: Vec<(String, bool)> = summaries
            .iter()
            .map(|summary| (summary.request_id.clone(), summary.terminal))
            .collect();
        let satisfied = background_wait_condition_satisfied(wait_mode, &terminal_flags);
        timed_out = !satisfied;
        wait_outcome.satisfied = satisfied;
        if matches!(wait_mode, BackgroundWaitMode::All)
            || wait_outcome.first_terminal_request_id.is_none()
        {
            wait_outcome.first_terminal_request_id =
                harness_core::coord::first_terminal_request_id(&terminal_flags);
        }
    }

    let mut results = Vec::with_capacity(summaries.len());
    for summary in &summaries {
        results.push(json!({
            "request_id": summary.request_id,
            "task_id": summary.session_id,
            "session_id": summary.session_id,
            "scheduler_task_id": summary.scheduler_task_id,
            "status": summary.status,
            "terminal": summary.terminal,
            "duration_ms": summary.duration_ms,
            "result_summary": summary.result_summary,
            "failure_summary": summary.failure_summary,
            "late_result": summary.late_result,
            "cancel_reason": summary.cancel_reason,
        }));
    }

    let primary = wait_outcome
        .first_terminal_request_id
        .as_ref()
        .and_then(|request_id| {
            summaries
                .iter()
                .find(|summary| summary.request_id == *request_id)
        })
        .or_else(|| summaries.first());

    let text = format_multi_wait_output(wait_mode, &wait_outcome, timed_out, &summaries);
    let payload = json!({
        "wait_mode": wait_mode.as_str(),
        "request_ids": request_ids,
        "results": results,
        "first_terminal_request_id": wait_outcome.first_terminal_request_id,
        "satisfied": wait_outcome.satisfied,
        "request_id": primary.map(|summary| summary.request_id.clone()),
        "task_id": primary.and_then(|summary| summary.session_id.clone()),
        "session_id": primary.and_then(|summary| summary.session_id.clone()),
        "scheduler_task_id": primary.and_then(|summary| summary.scheduler_task_id.clone()),
        "status": primary.map(|summary| summary.status.clone()),
        "mode": "background",
        "terminal": wait_outcome.satisfied,
        "block": request.block,
        "timed_out": timed_out,
        "timeout_ms": request.timeout_ms,
        "result_summary": primary.and_then(|summary| summary.result_summary.clone()),
        "failure_summary": primary.and_then(|summary| summary.failure_summary.clone()),
        "late_result": primary.map(|summary| summary.late_result).unwrap_or(false),
        "source": "event_replay",
    });

    Ok(text_json_tool_result(text, payload))
}

fn format_multi_wait_output(
    wait_mode: BackgroundWaitMode,
    outcome: &BackgroundWaitOutcome,
    timed_out: bool,
    summaries: &[BackgroundRequestSummary],
) -> String {
    let terminal_count = summaries.iter().filter(|summary| summary.terminal).count();
    if timed_out {
        format!(
            "background_output wait_{} timed out with {}/{} terminal",
            wait_mode.as_str(),
            terminal_count,
            summaries.len()
        )
    } else if outcome.satisfied {
        match (wait_mode, outcome.first_terminal_request_id.as_deref()) {
            (BackgroundWaitMode::Any, Some(request_id)) => {
                format!("background_output wait_any satisfied by `{request_id}` ({terminal_count}/{} terminal)", summaries.len())
            }
            _ => format!(
                "background_output wait_{} satisfied ({terminal_count}/{} terminal)",
                wait_mode.as_str(),
                summaries.len()
            ),
        }
    } else {
        format!(
            "background_output wait_{} not satisfied ({terminal_count}/{} terminal)",
            wait_mode.as_str(),
            summaries.len()
        )
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

fn build_full_session_payload(
    events: &[EventEnvelopeV1],
    request: &BackgroundOutputRequest,
) -> Value {
    let all_event_summaries: Vec<Value> = events.iter().map(summarize_event).collect();

    let mut messages: Vec<Value> = events
        .iter()
        .filter(|event| {
            matches!(
                event.payload,
                EventV1::UserMessageSubmitted(_) | EventV1::AssistantMessageFinished(_)
            )
        })
        .map(summarize_event)
        .collect();

    if let Some(since_id) = request.since_message_id.as_deref() {
        let position = messages
            .iter()
            .position(|msg| msg.get("event_id").and_then(Value::as_str) == Some(since_id));
        if let Some(pos) = position {
            messages = messages.split_at(pos + 1).1.to_vec();
        }
    }

    if request.from_end {
        messages.reverse();
    }

    let max_messages = request
        .message_limit
        .map(|n| (n as usize).min(200))
        .unwrap_or(200);
    messages.truncate(max_messages);

    let mut payload = json!({
        "events": all_event_summaries,
        "messages": messages,
        "event_count": all_event_summaries.len(),
        "message_count": messages.len(),
    });

    if request.include_tool_results {
        let tool_results: Vec<Value> = events
            .iter()
            .filter(|event| matches!(event.payload, EventV1::ToolCallFinished(_)))
            .map(summarize_event)
            .collect();
        payload["tool_results"] = json!(tool_results);
    }

    payload
}

fn build_thinking_artifact(
    ctx: &ToolContext,
    events: &[EventEnvelopeV1],
    request: &BackgroundOutputRequest,
) -> Result<Option<(Value, ArtifactRef)>, ToolError> {
    let max_chars = request.thinking_max_chars.unwrap_or(2000) as usize;

    let mut thinking_blocks: Vec<(String, String)> = Vec::new();
    let mut current_request_id: Option<String> = None;
    let mut current_text = String::new();

    for event in events {
        if let EventV1::ProviderReasoningDelta(data) = &event.payload {
            let req_id = data.request_id.to_string();
            if current_request_id.as_deref() != Some(req_id.as_str()) {
                if let Some(prev_id) = current_request_id.take() {
                    thinking_blocks.push((prev_id, std::mem::take(&mut current_text)));
                }
                current_request_id = Some(req_id);
            }
            current_text.push_str(&data.delta);
        }
    }
    if let Some(req_id) = current_request_id {
        thinking_blocks.push((req_id, current_text));
    }

    if thinking_blocks.is_empty() {
        return Ok(None);
    }

    let thinking_json: Vec<Value> = thinking_blocks
        .iter()
        .map(|(req_id, text)| {
            let char_count = text.chars().count();
            let truncated = char_count > max_chars;
            let capped = if truncated {
                let mut s: String = text.chars().take(max_chars).collect();
                s.push('…');
                s
            } else {
                text.clone()
            };
            json!({
                "request_id": req_id,
                "thinking": capped,
                "original_chars": char_count,
                "truncated": truncated,
            })
        })
        .collect();

    let body = serde_json::to_string_pretty(&json!(thinking_json))
        .tool_err("failed to serialize thinking content")?;

    let artifact = ctx
        .artifact_store()
        .map_err(|err| ToolError::Execution(err.to_string()))?
        .write_text("background-thinking.json", &body)
        .map_err(|err| ToolError::Execution(err.to_string()))?;

    let artifact_ref = ArtifactRef {
        path: artifact.path,
        digest: artifact.digest,
    };

    let inline = json!({
        "artifact": artifact_ref,
        "block_count": thinking_json.len(),
        "spilled": true,
    });

    Ok(Some((inline, artifact_ref)))
}
