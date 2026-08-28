use super::*;

pub(super) fn add_task(
    task: &harness_core::transcript_projection::ProjectedTaskPart,
    rows: &mut BTreeMap<String, OrchestrationTaskRow>,
    turn_terminals: &mut BTreeMap<String, SettledTurnTerminal>,
    agent_id: Option<&str>,
    request_id: Option<&str>,
) {
    let lineage = task.lineage.as_ref();
    let task_id = task.task_id.to_string();
    let row = rows
        .entry(task_id.clone())
        .or_insert_with(|| OrchestrationTaskRow {
            task_id: task.task_id.to_string(),
            queue_key: task.queue_key.clone(),
            state: task_state(task.state),
            warning: task.reason.clone(),
            owner_kind: agent_id.map_or(ActorKind::System, |_| ActorKind::Worker),
            owner_agent_id: agent_id.map(str::to_string),
            request_id: request_id.map(str::to_string),
            parent_tool_call_id: lineage.and_then(|value| value.parent_tool_call_id.clone()),
            parent_request_id: lineage.and_then(|value| value.parent_request_id.clone()),
            child_session_id: lineage.and_then(|value| value.child_session_id.clone()),
            child_request_id: lineage.and_then(|value| value.child_request_id.clone()),
            result_summary: task.result_summary.clone(),
            child_tool_call_count: 0,
            current_child_tool_title: None,
            timing_elapsed_ms: task.timing_elapsed_ms,
            first_seq: task.provenance.first_seq,
            last_seq: task.provenance.last_seq,
            first_mono_ms: task.provenance.first_seq,
            last_mono_ms: task.provenance.last_seq,
            first_timestamp: None,
            last_timestamp: None,
        });
    row.state = task_state(task.state);
    if task.queue_key.is_some() {
        row.queue_key.clone_from(&task.queue_key);
    }
    if task.reason.is_some() {
        row.warning.clone_from(&task.reason);
    }
    if task.result_summary.is_some() {
        row.result_summary.clone_from(&task.result_summary);
    }
    if let Some(lineage) = lineage {
        if lineage.parent_tool_call_id.is_some() {
            row.parent_tool_call_id
                .clone_from(&lineage.parent_tool_call_id);
        }
        if lineage.parent_request_id.is_some() {
            row.parent_request_id.clone_from(&lineage.parent_request_id);
        }
        if lineage.child_session_id.is_some() {
            row.child_session_id.clone_from(&lineage.child_session_id);
        }
        if lineage.child_request_id.is_some() {
            row.child_request_id.clone_from(&lineage.child_request_id);
        }
    }
    if request_id.is_some() {
        row.request_id = request_id.map(str::to_string);
    }
    if task.timing_elapsed_ms.is_some() {
        row.timing_elapsed_ms = task.timing_elapsed_ms;
    }
    row.last_seq = task.provenance.last_seq;
    row.last_mono_ms = task.terminal_mono_ms.unwrap_or(task.provenance.last_seq);

    let is_turn_terminal = task.terminal_scope
        == Some(harness_core::event::TaskTerminalScope::AgentTurn)
        || task.terminal_scope.is_none()
            && task.state == ProjectedTaskState::Completed
            && lineage
                .and_then(|value| value.parent_tool_call_id.as_ref())
                .is_none();
    if is_turn_terminal {
        if let Some(request_id) = request_id {
            turn_terminals.insert(
                request_id.to_string(),
                SettledTurnTerminal {
                    state: task.state,
                    reason: task.reason.clone(),
                    result_summary: task.result_summary.clone(),
                    elapsed_ms: task.timing_elapsed_ms,
                    terminal_mono_ms: task.terminal_mono_ms,
                },
            );
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct SettledTurnTerminal {
    state: ProjectedTaskState,
    reason: Option<String>,
    result_summary: Option<String>,
    elapsed_ms: Option<u64>,
    terminal_mono_ms: Option<u64>,
}

pub(super) fn apply_turn_terminals(
    activities: &mut VecDeque<ActivityEntry>,
    terminals: &BTreeMap<String, SettledTurnTerminal>,
    completed: &mut std::collections::BTreeSet<String>,
    elapsed: &mut BTreeMap<String, u64>,
) {
    for (request_id, terminal) in terminals {
        let Some(activity) = activities
            .iter_mut()
            .find(|activity| activity.request_id == *request_id)
        else {
            continue;
        };
        if let Some(terminal_mono_ms) = terminal.terminal_mono_ms {
            activity.last_mono_ms = terminal_mono_ms;
        }
        if let Some(value) = terminal.elapsed_ms.or_else(|| {
            terminal
                .terminal_mono_ms
                .map(|terminal| terminal.saturating_sub(activity.first_mono_ms))
        }) {
            elapsed.insert(request_id.clone(), value);
        }
        match terminal.state {
            ProjectedTaskState::Completed => {
                activity.status = ActivityStatus::Done;
                if activity.transcript_text.is_empty() {
                    if let Some(result_summary) = terminal.result_summary.as_ref() {
                        activity.transcript_text.clone_from(result_summary);
                        activity.bump_revision();
                    }
                }
                completed.insert(request_id.clone());
            }
            ProjectedTaskState::Cancelled => {
                activity.status = ActivityStatus::Error;
                activity.error_message =
                    match (terminal.reason.as_deref(), activity.error_message.take()) {
                        (Some(reason), Some(existing)) if !existing.contains(reason) => {
                            Some(format!("{reason} · {existing}"))
                        }
                        (Some(reason), _) => Some(reason.to_string()),
                        (None, existing) => existing,
                    };
                completed.insert(request_id.clone());
            }
            ProjectedTaskState::Queued
            | ProjectedTaskState::Started
            | ProjectedTaskState::LateResult => {}
        }
    }
}

const fn task_state(state: ProjectedTaskState) -> OrchestrationTaskState {
    match state {
        ProjectedTaskState::Queued => OrchestrationTaskState::Queued,
        ProjectedTaskState::Started => OrchestrationTaskState::Running,
        ProjectedTaskState::Cancelled => OrchestrationTaskState::Cancelled,
        ProjectedTaskState::Completed => OrchestrationTaskState::Completed,
        ProjectedTaskState::LateResult => OrchestrationTaskState::LateResult,
    }
}
