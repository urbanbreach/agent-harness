// allow: SIZE_OK — agent operations (task delegation + control plane)
use std::time::Duration;

use harness_core::config::registered_profile_model_metadata;
use harness_core::coord::AgentRuntimeInfo;
use harness_core::event::{
    EventV1, TaskCancelledEvent, TaskCompletedEvent, TaskScheduleState, TaskTerminalScope,
    ToolCallStatus,
};
use harness_core::redact::{DefaultRedactor, Redactor};
use harness_core::store::{EventStoreError, EventStream};
use harness_core::tool::{ToolContext, ToolError};
use harness_core::ToolResultExt;
use serde::Serialize;
use serde_json::{json, Value};
use tokio::time::{sleep, Instant};
use tokio_stream::StreamExt;

use super::AgentSpawnRequest;
mod child_next_actions;
use crate::control_plane::TaskSkillContext;
use child_next_actions::child_next_actions;

const DEFAULT_TASK_WAIT_TIMEOUT_MS: u64 = 300_000;
const CHILD_RESULT_SUMMARY_MAX_CHARS: usize = 1200;

#[derive(Debug, Clone, Default, Serialize)]
pub(super) struct ChildToolCallCounts {
    pub(super) requested: u64,
    pub(super) succeeded: u64,
    pub(super) failed: u64,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ChildPermissionMetadata {
    pub(super) spawn_permission_kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) parent_scope: Option<String>,
    pub(super) child_scope: String,
    pub(super) scope_relation: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ChildRuntimeMetadata {
    pub(super) profile: String,
    pub(super) role: &'static str,
    pub(super) model_ref: String,
    pub(super) model: ChildModelMetadata,
    pub(super) toolset: Vec<String>,
    pub(super) permission_posture: ChildPermissionPosture,
    pub(super) can_redelegate: bool,
    pub(super) has_background_output: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) parent_agent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ChildModelMetadata {
    pub(super) model_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) provider: Option<String>,
    pub(super) model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) variant: Option<String>,
    pub(super) fallback_chain: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ChildPromptMetadata {
    pub(super) source: &'static str,
    pub(super) status: &'static str,
    pub(super) profile: String,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ChildPermissionPosture {
    pub(super) spawn: &'static str,
    pub(super) edit: &'static str,
    pub(super) bash: &'static str,
    pub(super) question: &'static str,
    pub(super) task: &'static str,
    pub(super) webfetch: &'static str,
    pub(super) websearch: &'static str,
    pub(super) codesearch: &'static str,
    pub(super) lsp: &'static str,
    pub(super) background_output: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ChildSessionObservability {
    pub(super) session_id: String,
    pub(super) request_id: String,
    pub(super) profile: String,
    pub(super) background: bool,
    pub(super) mode: &'static str,
    pub(super) status: &'static str,
    pub(super) resumed_existing_session: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) result_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) failure_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) child_summary: Option<ChildSummary>,
    pub(super) tool_calls: ChildToolCallCounts,
    pub(super) permissions: ChildPermissionMetadata,
    pub(super) runtime: ChildRuntimeMetadata,
}

#[derive(Debug, Clone)]
pub(super) struct ChildRequestObservability {
    pub(super) status: &'static str,
    pub(super) duration_ms: Option<u64>,
    pub(super) result_summary: Option<String>,
    pub(super) failure_summary: Option<String>,
    pub(super) child_summary: Option<ChildSummary>,
    pub(super) tool_calls: ChildToolCallCounts,
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct ChildSummary {
    pub(super) kind: &'static str,
    pub(super) summary: String,
    pub(super) max_chars: usize,
    pub(super) original_chars: usize,
    pub(super) truncated: bool,
}

#[derive(Debug, Clone, Copy)]
enum ChildTerminalState {
    Completed,
    Failed,
    TimedOut,
}

pub(super) async fn wait_for_request_completion(
    ctx: &ToolContext,
    request_id: &str,
) -> Result<ChildRequestObservability, ToolError> {
    let mut stream = subscribe_events(ctx).await?;
    let deadline = Instant::now() + Duration::from_millis(DEFAULT_TASK_WAIT_TIMEOUT_MS);
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let next =
            tokio::time::timeout(remaining.min(Duration::from_millis(250)), stream.next()).await;
        match next {
            Ok(Some(Ok(event))) => match &event.payload {
                EventV1::TaskCompleted(data)
                    if event.correlation_id.as_deref() == Some(request_id)
                        && task_completed_marks_child_agent_turn(data) =>
                {
                    return summarize_child_request(ctx, request_id, ChildTerminalState::Completed)
                        .await;
                }
                EventV1::TaskCancelled(data)
                    if event.correlation_id.as_deref() == Some(request_id)
                        && task_cancelled_marks_child_agent_turn(data) =>
                {
                    return summarize_child_request(ctx, request_id, ChildTerminalState::Failed)
                        .await;
                }
                _ => {}
            },
            Ok(Some(Err(err))) => {
                return Err(map_event_stream_error(err));
            }
            Ok(None) | Err(_) => {
                sleep(Duration::from_millis(10)).await;
            }
        }
    }
    summarize_child_request(ctx, request_id, ChildTerminalState::TimedOut).await
}

async fn summarize_child_request(
    ctx: &ToolContext,
    request_id: &str,
    terminal_state: ChildTerminalState,
) -> Result<ChildRequestObservability, ToolError> {
    let mut replay = replay_events(ctx).await?;

    let mut tool_calls = ChildToolCallCounts::default();
    let mut started_mono_ms = None;
    let mut observed_result_summary = None;
    let mut observed_failure_summary = None;
    let mut observed_duration_ms = None;
    let mut observed_status = None;

    while let Some(next) = replay.next().await {
        let event = next.map_err(map_replay_stream_error)?;
        if event.correlation_id.as_deref() != Some(request_id) {
            continue;
        }

        match &event.payload {
            EventV1::TaskScheduled(data) if data.state == TaskScheduleState::Started => {
                started_mono_ms.get_or_insert(event.mono_ms);
            }
            EventV1::ToolCallRequested(_) => {
                tool_calls.requested = tool_calls.requested.saturating_add(1);
            }
            EventV1::ToolCallFinished(data) => match data.status {
                ToolCallStatus::Succeeded => {
                    tool_calls.succeeded = tool_calls.succeeded.saturating_add(1);
                }
                ToolCallStatus::Failed => {
                    tool_calls.failed = tool_calls.failed.saturating_add(1);
                }
            },
            EventV1::TaskCompleted(data) if task_completed_marks_child_agent_turn(data) => {
                observed_status = Some("completed");
                observed_result_summary = Some(data.result_summary.clone());
                let timing = data
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.timing.as_ref());
                started_mono_ms = timing
                    .and_then(|metadata| metadata.started_mono_ms)
                    .or(started_mono_ms);
                observed_duration_ms = timing
                    .and_then(|metadata| metadata.elapsed_ms)
                    .or_else(|| elapsed_ms_from_events(started_mono_ms, event.mono_ms));
            }
            EventV1::TaskCancelled(data) if task_cancelled_marks_child_agent_turn(data) => {
                observed_status = Some("failed");
                observed_failure_summary = Some(data.reason.clone());
                observed_duration_ms = elapsed_ms_from_events(started_mono_ms, event.mono_ms);
            }
            _ => {}
        }
    }

    let (status, failure_summary) = match (observed_status, terminal_state) {
        (Some(status), _) => (status, observed_failure_summary),
        (None, ChildTerminalState::Completed) => ("completed", observed_failure_summary),
        (None, ChildTerminalState::Failed) => ("failed", observed_failure_summary),
        (None, ChildTerminalState::TimedOut) => (
            "timed_out",
            Some(format!("timed out waiting for task request {request_id}")),
        ),
    };
    let (result_summary, result_child_summary) =
        cap_optional_child_summary("result", observed_result_summary);
    let (failure_summary, failure_child_summary) =
        cap_optional_child_summary("failure", failure_summary);

    Ok(ChildRequestObservability {
        status,
        duration_ms: observed_duration_ms,
        result_summary,
        failure_summary,
        child_summary: result_child_summary.or(failure_child_summary),
        tool_calls,
    })
}

async fn subscribe_events(ctx: &ToolContext) -> Result<EventStream, ToolError> {
    let store = ctx
        .coordinator
        .event_store()
        .await
        .tool_err("failed to access event store")?;
    store.subscribe(1).tool_err("failed to subscribe to events")
}

pub(super) async fn replay_events(ctx: &ToolContext) -> Result<EventStream, ToolError> {
    let store = ctx
        .coordinator
        .event_store()
        .await
        .tool_err("failed to access event store")?;
    store.replay(1).tool_err("failed to replay events")
}

fn map_event_stream_error(err: EventStoreError) -> ToolError {
    ToolError::Execution(format!("failed to consume event stream: {err}"))
}

pub(super) fn map_replay_stream_error(err: EventStoreError) -> ToolError {
    ToolError::Execution(format!("failed to replay event stream: {err}"))
}

fn elapsed_ms_from_events(started_mono_ms: Option<u64>, finished_mono_ms: u64) -> Option<u64> {
    started_mono_ms.map(|started| finished_mono_ms.saturating_sub(started))
}

fn task_completed_marks_child_agent_turn(data: &TaskCompletedEvent) -> bool {
    data.metadata
        .as_ref()
        .and_then(|metadata| metadata.task_scope)
        .is_some_and(|scope| matches!(scope, TaskTerminalScope::AgentTurn))
}

fn task_cancelled_marks_child_agent_turn(data: &TaskCancelledEvent) -> bool {
    data.task_scope
        .is_some_and(|scope| matches!(scope, TaskTerminalScope::AgentTurn))
}

pub(super) fn child_permission_metadata(
    ctx: &ToolContext,
    runtime: &AgentRuntimeInfo,
) -> ChildPermissionMetadata {
    let parent_scope = ctx.profile.clone();
    let scope_relation = if parent_scope.as_deref() == Some(runtime.profile_name.as_str()) {
        "inherits_parent_scope"
    } else {
        "isolated_by_child_profile"
    };

    ChildPermissionMetadata {
        spawn_permission_kind: "task",
        parent_scope,
        child_scope: runtime.profile_name.clone(),
        scope_relation,
    }
}

pub(super) fn child_runtime_metadata(runtime: &AgentRuntimeInfo) -> ChildRuntimeMetadata {
    ChildRuntimeMetadata {
        profile: runtime.profile_name.clone(),
        model_ref: runtime.model_ref.clone(),
        role: child_route_role(runtime),
        model: child_model_metadata(&runtime.profile_name, &runtime.model_ref),
        permission_posture: child_permission_posture(&runtime.toolset),
        can_redelegate: runtime.toolset.iter().any(|tool| tool == "task"),
        has_background_output: runtime
            .toolset
            .iter()
            .any(|tool| tool == "background_output"),
        toolset: runtime.toolset.clone(),
        parent_agent_id: runtime.parent_agent_id.clone(),
    }
}

fn child_model_metadata(profile_name: &str, model_ref: &str) -> ChildModelMetadata {
    if let Some(metadata) = registered_profile_model_metadata(profile_name) {
        return ChildModelMetadata {
            model_ref: model_ref.to_string(),
            provider: Some(metadata.provider),
            model: metadata.model,
            variant: metadata.variant,
            fallback_chain: Vec::new(),
        };
    }

    let (provider, model, variant) = split_model_ref(model_ref);
    ChildModelMetadata {
        model_ref: model_ref.to_string(),
        provider,
        model,
        variant,
        fallback_chain: Vec::new(),
    }
}

fn split_model_ref(model_ref: &str) -> (Option<String>, String, Option<String>) {
    let mut slash_parts = model_ref.split('/');
    let first = slash_parts.next();
    let second = slash_parts.next();
    let third = slash_parts.next();
    if let (Some(provider), Some(model)) = (first, second) {
        return (
            Some(provider.to_string()),
            model.to_string(),
            third.map(str::to_string),
        );
    }

    let mut colon_parts = model_ref.splitn(2, ':');
    match (colon_parts.next(), colon_parts.next()) {
        (Some(provider), Some(model)) => (Some(provider.to_string()), model.to_string(), None),
        _ => (None, model_ref.to_string(), None),
    }
}

fn child_route_role(runtime: &AgentRuntimeInfo) -> &'static str {
    if runtime.parent_agent_id.is_some() {
        "subagent"
    } else {
        "primary"
    }
}

fn child_permission_posture(toolset: &[String]) -> ChildPermissionPosture {
    ChildPermissionPosture {
        spawn: "checked_before_child_turn",
        edit: tool_permission_posture(toolset, "edit"),
        bash: tool_permission_posture(toolset, "bash"),
        question: tool_permission_posture(toolset, "question"),
        task: tool_permission_posture(toolset, "task"),
        webfetch: tool_permission_posture(toolset, "webfetch"),
        websearch: tool_permission_posture(toolset, "websearch"),
        codesearch: tool_permission_posture(toolset, "codesearch"),
        lsp: tool_permission_posture(toolset, "lsp"),
        background_output: tool_permission_posture(toolset, "background_output"),
    }
}

fn tool_permission_posture(toolset: &[String], tool_id: &str) -> &'static str {
    if toolset.iter().any(|tool| tool == tool_id) {
        "available_subject_to_runtime_permission"
    } else {
        "deny_by_toolset"
    }
}

pub(super) fn cap_optional_child_summary(
    kind: &'static str,
    summary: Option<String>,
) -> (Option<String>, Option<ChildSummary>) {
    summary
        .map(|summary| {
            let child_summary = cap_child_summary(kind, &summary);
            (Some(child_summary.summary.clone()), Some(child_summary))
        })
        .unwrap_or((None, None))
}

fn cap_child_summary(kind: &'static str, summary: &str) -> ChildSummary {
    let redacted = DefaultRedactor::default().redact_text(summary.trim());
    let redacted_chars = redacted.chars().count();
    let already_ellipsized = redacted.ends_with('…');
    let truncated = redacted_chars > CHILD_RESULT_SUMMARY_MAX_CHARS || already_ellipsized;
    let original_chars = if already_ellipsized && redacted_chars <= CHILD_RESULT_SUMMARY_MAX_CHARS {
        CHILD_RESULT_SUMMARY_MAX_CHARS + 1
    } else {
        redacted_chars
    };
    let mut capped = redacted
        .chars()
        .take(CHILD_RESULT_SUMMARY_MAX_CHARS)
        .collect::<String>();
    if redacted_chars > CHILD_RESULT_SUMMARY_MAX_CHARS {
        capped.push('…');
    }
    ChildSummary {
        kind,
        summary: capped,
        max_chars: CHILD_RESULT_SUMMARY_MAX_CHARS,
        original_chars,
        truncated,
    }
}

pub(super) fn child_session_observability(
    session_id: &str,
    request_id: &str,
    request: &AgentSpawnRequest,
    resumed_existing_session: bool,
    permissions: &ChildPermissionMetadata,
    runtime: ChildRuntimeMetadata,
    observability: ChildRequestObservability,
) -> ChildSessionObservability {
    ChildSessionObservability {
        session_id: session_id.to_string(),
        request_id: request_id.to_string(),
        profile: runtime.profile.clone(),
        background: request.run_in_background,
        mode: if request.run_in_background {
            "background"
        } else {
            "foreground"
        },
        status: observability.status,
        resumed_existing_session,
        duration_ms: observability.duration_ms,
        result_summary: observability.result_summary,
        failure_summary: observability.failure_summary,
        child_summary: observability.child_summary,
        tool_calls: observability.tool_calls,
        permissions: permissions.clone(),
        runtime,
    }
}

pub(super) fn spawn_result_json(
    request: &AgentSpawnRequest,
    loaded_skills: &[TaskSkillContext],
    agent_id: &str,
    request_id: &str,
    lineage: Value,
    child_session: &ChildSessionObservability,
) -> Value {
    json!({
        "description": request.description,
        "profile": child_session.runtime.profile,
        "task_id": agent_id,
        "session_id": agent_id,
        "request_id": request_id,
        "child_session_id": agent_id,
        "child_request_id": request_id,
        "lineage": lineage,
        "route": route_metadata(loaded_skills, &child_session.runtime, &child_session.permissions),
        "load_skills": request.load_skills,
        "skills": request.load_skills,
        "loaded_skills": loaded_skill_metadata(loaded_skills),
        "command": request.command,
        "background": child_session.background,
        "mode": child_session.mode,
        "status": child_session.status,
        "duration_ms": child_session.duration_ms,
        "result_summary": child_session.result_summary,
        "failure_summary": child_session.failure_summary,
        "child_summary": child_session.child_summary,
        "child_tool_call_count": child_session.tool_calls.requested,
        "child_tool_call_counts": child_session.tool_calls,
        "resumed_existing_session": child_session.resumed_existing_session,
        "permissions": child_session.permissions,
        "runtime": child_session.runtime,
        "child_runtime": child_session.runtime,
        "effective_model_ref": child_session.runtime.model_ref,
        "child_toolset": child_session.runtime.toolset,
        "can_redelegate": child_session.runtime.can_redelegate,
        "has_background_output": child_session.runtime.has_background_output,
        "next_actions": child_next_actions(request, agent_id, request_id),
        "child_session": child_session,
    })
}

fn route_metadata(
    loaded_skills: &[TaskSkillContext],
    runtime: &ChildRuntimeMetadata,
    permissions: &ChildPermissionMetadata,
) -> Value {
    json!({
        "profile_id": runtime.profile.clone(),
        "role": runtime.role,
        "hidden": false,
        "prompt": ChildPromptMetadata {
            source: "runtime_profile",
            status: "resolved_by_coordinator",
            profile: runtime.profile.clone(),
        },
        "model": runtime.model.clone(),
        "toolset": runtime.toolset.clone(),
        "permission_posture": runtime.permission_posture.clone(),
        "permissions": permissions,
        "loaded_skills": loaded_skill_metadata(loaded_skills),
    })
}

fn loaded_skill_metadata(loaded_skills: &[TaskSkillContext]) -> Value {
    Value::Array(
        loaded_skills
            .iter()
            .map(|skill| json!(skill.metadata))
            .collect(),
    )
}
