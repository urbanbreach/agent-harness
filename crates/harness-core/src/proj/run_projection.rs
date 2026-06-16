use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::event::{EventEnvelopeV1, EventV1};

use super::{enforce_seq, ProjectionError, RunStatus};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RunCounts {
    pub total_events: u64,
    pub by_type: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSummary {
    pub status: RunStatus,
    pub counts: RunCounts,
    pub last_error: Option<String>,
    pub tasks_in_flight: BTreeSet<String>,
    pub pending_permissions: BTreeSet<String>,
}

impl Default for RunSummary {
    fn default() -> Self {
        Self {
            status: RunStatus::Running,
            counts: RunCounts::default(),
            last_error: None,
            tasks_in_flight: BTreeSet::new(),
            pending_permissions: BTreeSet::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineEventRef {
    pub seq: u64,
    pub event_id: String,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub stream_key: Option<String>,
    pub event_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TimelineIndex {
    pub events: Vec<TimelineEventRef>,
    pub correlation_groups: BTreeMap<String, Vec<u64>>,
}

pub fn project_run_summary<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
) -> Result<RunSummary, ProjectionError> {
    let mut summary = RunSummary::default();
    let mut last_seq: Option<u64> = None;

    for event in events {
        enforce_seq(last_seq, event.seq)?;
        last_seq = Some(event.seq);
        apply_run_summary_event(&mut summary, event);
    }

    Ok(summary)
}

pub fn project_timeline_index<'a>(
    events: impl IntoIterator<Item = &'a EventEnvelopeV1>,
) -> Result<TimelineIndex, ProjectionError> {
    let mut index = TimelineIndex::default();
    let mut last_seq: Option<u64> = None;

    for event in events {
        enforce_seq(last_seq, event.seq)?;
        last_seq = Some(event.seq);
        apply_timeline_event(&mut index, event);
    }

    Ok(index)
}

fn apply_run_summary_event(summary: &mut RunSummary, event: &EventEnvelopeV1) {
    summary.counts.total_events += 1;
    let event_type = event_type_name(&event.payload);
    *summary.counts.by_type.entry(event_type).or_insert(0) += 1;

    match &event.payload {
        EventV1::RunStarted(_) => {
            summary.status = RunStatus::Running;
        }
        EventV1::SessionTitleUpdated(_) => {}
        EventV1::RunFinished(_) => {
            summary.status = RunStatus::Finished;
        }
        EventV1::RunFailed(payload) => {
            summary.status = RunStatus::Failed;
            summary.last_error = Some(payload.error.clone());
        }
        EventV1::TaskScheduled(payload) => {
            summary.tasks_in_flight.insert(payload.task_id.clone());
        }
        EventV1::TaskCancelled(payload) => {
            summary.tasks_in_flight.remove(&payload.task_id);
        }
        EventV1::TaskCompleted(payload) => {
            summary.tasks_in_flight.remove(&payload.task_id);
        }
        EventV1::BackgroundTaskNotification(payload) => {
            summary.tasks_in_flight.remove(&payload.task_id);
            summary.tasks_in_flight.remove(&payload.terminal_task_id);
        }
        EventV1::PermissionRequested(payload) => {
            summary
                .pending_permissions
                .insert(payload.permission_id.clone());
        }
        EventV1::PermissionResolved(payload) => {
            summary.pending_permissions.remove(&payload.permission_id);
        }
        EventV1::UserMessageSubmitted(_) => {}
        _ => {}
    }
}

fn apply_timeline_event(index: &mut TimelineIndex, event: &EventEnvelopeV1) {
    if let Some(correlation_id) = &event.correlation_id {
        index
            .correlation_groups
            .entry(correlation_id.clone())
            .or_default()
            .push(event.seq);
    }

    index.events.push(TimelineEventRef {
        seq: event.seq,
        event_id: event.event_id.clone(),
        correlation_id: event.correlation_id.clone(),
        causation_id: event.causation_id.clone(),
        stream_key: event.stream_key.clone(),
        event_type: event_type_name(&event.payload),
    });
}

fn event_type_name(event: &EventV1) -> String {
    match event {
        EventV1::RunStarted(_) => "run_started",
        EventV1::SessionTitleUpdated(_) => "session_title_updated",
        EventV1::RunFinished(_) => "run_finished",
        EventV1::RunFailed(_) => "run_failed",
        EventV1::AgentSpawned(_) => "agent_spawned",
        EventV1::AgentStopped(_) => "agent_stopped",
        EventV1::TaskScheduled(_) => "task_scheduled",
        EventV1::TaskCancelled(_) => "task_cancelled",
        EventV1::TaskCompleted(_) => "task_completed",
        EventV1::TaskResultLate(_) => "task_result_late",
        EventV1::BackgroundTaskNotification(_) => "background_task_notification",
        EventV1::StaleDetected(_) => "stale_detected",
        EventV1::ProviderRequestStarted(_) => "provider_request_started",
        EventV1::ProviderStreamDelta(_) => "provider_stream_delta",
        EventV1::ProviderReasoningDelta(_) => "provider_reasoning_delta",
        EventV1::ProviderRequestFinished(_) => "provider_request_finished",
        EventV1::AssistantMessageFinished(_) => "assistant_message_finished",
        EventV1::CompactionRequested(_) => "compaction_requested",
        EventV1::CompactionWritten(_) => "compaction_written",
        EventV1::CompactionApplied(_) => "compaction_applied",
        EventV1::CompactionFailed(_) => "compaction_failed",
        EventV1::ToolCallRequested(_) => "tool_call_requested",
        EventV1::ToolCallStarted(_) => "tool_call_started",
        EventV1::ToolCallFinished(_) => "tool_call_finished",
        EventV1::PermissionRequested(_) => "permission_requested",
        EventV1::PermissionGrantRecorded(_) => "permission_grant_recorded",
        EventV1::PermissionResolved(_) => "permission_resolved",
        EventV1::EditProposed(_) => "edit_proposed",
        EventV1::EditApplied(_) => "edit_applied",
        EventV1::EditRejected(_) => "edit_rejected",
        EventV1::ArtifactWritten(_) => "artifact_written",
        EventV1::PolicyViolationDetected(_) => "policy_violation_detected",
        EventV1::UiIntentReceived(_) => "ui_intent_received",
        EventV1::UserMessageSubmitted(_) => "user_message_submitted",
        EventV1::WorkspaceSnapshot(_) => "workspace_snapshot",
        EventV1::WorkspaceReverted(_) => "workspace_reverted",
    }
    .to_string()
}
