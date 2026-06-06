use std::collections::BTreeMap;

use crate::event::{
    BackgroundTaskNotificationStatus, ExecutionTimingMetadata, HookExecutionMetadata,
    TaskLineageMetadata,
};

use super::metadata_merge::merge_hook_execution;
use super::{AgentTurnProjectionState, ChildSessionTerminalState, ResumeChildSessionSnapshot};

pub(super) fn apply_child_session_metadata(
    child_sessions: &mut BTreeMap<String, ResumeChildSessionSnapshot>,
    lineage: Option<&TaskLineageMetadata>,
    parent_session_id: Option<&str>,
    timing: Option<&ExecutionTimingMetadata>,
    hook_executions: &[HookExecutionMetadata],
) {
    let Some(lineage) = lineage else {
        return;
    };
    let Some(child_session_id) = lineage.child_session_id.as_ref() else {
        return;
    };

    let child = child_sessions.entry(child_session_id.clone()).or_default();
    if child.parent_tool_call_id.is_none() {
        child.parent_tool_call_id = lineage.parent_tool_call_id.clone();
    }
    if child.parent_task_id.is_none() {
        child.parent_task_id = lineage.parent_task_id.clone();
    }
    if child.parent_request_id.is_none() {
        child.parent_request_id = lineage.parent_request_id.clone();
    }
    if child.parent_session_id.is_none() {
        child.parent_session_id = lineage
            .parent_session_id
            .clone()
            .or_else(|| parent_session_id.map(str::to_string));
    }
    if let Some(request_id) = lineage.child_request_id.as_ref() {
        child.latest_child_request_id = Some(request_id.clone());
    }
    if child.provider_id.is_none() {
        child.provider_id = lineage.child_provider_id.clone();
    }
    if child.model_id.is_none() {
        child.model_id = lineage.child_model_id.clone();
    }
    if let Some(timing) = timing {
        child.timing = Some(timing.clone());
    }
    for hook_execution in hook_executions {
        merge_hook_execution(&mut child.hook_executions, hook_execution.clone());
    }
}

pub(super) fn apply_agent_turn_terminal_state(
    child_sessions: &mut BTreeMap<String, ResumeChildSessionSnapshot>,
    turn: &AgentTurnProjectionState,
    terminal_state: ChildSessionTerminalState,
    terminal_reason: Option<String>,
    finished_mono_ms: u64,
    timing_override: Option<ExecutionTimingMetadata>,
    hook_executions: &[HookExecutionMetadata],
) {
    let child = child_sessions.entry(turn.agent_id.clone()).or_default();
    child.latest_child_request_id = turn.request_id.clone();
    if let Some(provider_id) = turn.provider_id.as_ref() {
        child.provider_id = Some(provider_id.clone());
    }
    if let Some(model_id) = turn.model_id.as_ref() {
        child.model_id = Some(model_id.clone());
    }
    child.terminal_state = Some(terminal_state);
    child.terminal_reason = terminal_reason;
    child.timing = Some(
        timing_override
            .unwrap_or_else(|| derive_timing_from_start(turn.started_mono_ms, finished_mono_ms)),
    );
    for hook_execution in hook_executions {
        merge_hook_execution(&mut child.hook_executions, hook_execution.clone());
    }
}

pub(super) fn child_terminal_state_from_background_status(
    status: BackgroundTaskNotificationStatus,
) -> ChildSessionTerminalState {
    match status {
        BackgroundTaskNotificationStatus::Completed => ChildSessionTerminalState::Completed,
        BackgroundTaskNotificationStatus::Cancelled => ChildSessionTerminalState::Cancelled,
        BackgroundTaskNotificationStatus::Failed => ChildSessionTerminalState::Failed,
        BackgroundTaskNotificationStatus::TimedOut => ChildSessionTerminalState::TimedOut,
    }
}

pub(super) fn derive_timing_from_start(
    started_mono_ms: u64,
    finished_mono_ms: u64,
) -> ExecutionTimingMetadata {
    ExecutionTimingMetadata {
        started_mono_ms: Some(started_mono_ms),
        finished_mono_ms: Some(finished_mono_ms),
        elapsed_ms: Some(finished_mono_ms.saturating_sub(started_mono_ms)),
    }
}
