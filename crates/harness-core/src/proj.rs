use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::event::{EventEnvelopeV1, EventV1};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Finished,
    Failed,
}

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

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProjectionError {
    #[error("events must be strictly increasing by seq: previous={previous}, current={current}")]
    NonMonotonicSeq { previous: u64, current: u64 },
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
        EventV1::PermissionRequested(payload) => {
            summary
                .pending_permissions
                .insert(payload.permission_id.clone());
        }
        EventV1::PermissionResolved(payload) => {
            summary.pending_permissions.remove(&payload.permission_id);
        }
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
        EventV1::RunFinished(_) => "run_finished",
        EventV1::RunFailed(_) => "run_failed",
        EventV1::AgentSpawned(_) => "agent_spawned",
        EventV1::AgentStopped(_) => "agent_stopped",
        EventV1::TaskScheduled(_) => "task_scheduled",
        EventV1::TaskCancelled(_) => "task_cancelled",
        EventV1::TaskCompleted(_) => "task_completed",
        EventV1::TaskResultLate(_) => "task_result_late",
        EventV1::StaleDetected(_) => "stale_detected",
        EventV1::ProviderRequestStarted(_) => "provider_request_started",
        EventV1::ProviderStreamDelta(_) => "provider_stream_delta",
        EventV1::ProviderRequestFinished(_) => "provider_request_finished",
        EventV1::ToolCallRequested(_) => "tool_call_requested",
        EventV1::ToolCallStarted(_) => "tool_call_started",
        EventV1::ToolCallFinished(_) => "tool_call_finished",
        EventV1::PermissionRequested(_) => "permission_requested",
        EventV1::PermissionResolved(_) => "permission_resolved",
        EventV1::EditProposed(_) => "edit_proposed",
        EventV1::EditApplied(_) => "edit_applied",
        EventV1::EditRejected(_) => "edit_rejected",
        EventV1::ArtifactWritten(_) => "artifact_written",
        EventV1::PolicyViolationDetected(_) => "policy_violation_detected",
        EventV1::UiIntentReceived(_) => "ui_intent_received",
    }
    .to_string()
}

fn enforce_seq(last_seq: Option<u64>, current_seq: u64) -> Result<(), ProjectionError> {
    if let Some(previous) = last_seq {
        if current_seq <= previous {
            return Err(ProjectionError::NonMonotonicSeq {
                previous,
                current: current_seq,
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{project_run_summary, project_timeline_index, ProjectionError, RunStatus};
    use crate::event::{
        ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionDecision,
        PermissionRequestedEvent, PermissionResolvedEvent, RunFailedEvent, RunFinishedEvent,
        RunStartedEvent, TaskCompletedEvent, TaskScheduleState, TaskScheduledEvent,
        ToolCallRequestedEvent, SCHEMA_VERSION,
    };

    #[test]
    fn applying_same_jsonl_twice_yields_identical_run_summary() {
        let jsonl = fixture_jsonl();
        let first: Vec<EventEnvelopeV1> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).expect("valid fixture line"))
            .collect();
        let second: Vec<EventEnvelopeV1> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).expect("valid fixture line"))
            .collect();

        let summary_a = project_run_summary(first.iter()).expect("project first replay");
        let summary_b = project_run_summary(second.iter()).expect("project second replay");

        assert_eq!(summary_a, summary_b);
        assert_eq!(summary_a.status, RunStatus::Finished);
        assert!(summary_a.tasks_in_flight.is_empty());
        assert!(summary_a.pending_permissions.is_empty());
    }

    #[test]
    fn projections_ignore_side_effects_during_replay() {
        let events = [
            envelope(
                1,
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "demo".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                2,
                Some("corr-1"),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_1".to_string(),
                    tool_id: "shell.run".to_string(),
                    args_summary: "{\"cmd\":\"touch /tmp/should_not_run\"}".to_string(),
                    args_digest: "digest123456".to_string(),
                }),
            ),
            envelope(
                3,
                Some("corr-1"),
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "perm_1".to_string(),
                    kind: "shell".to_string(),
                    tool_call_id: Some("toolcall_1".to_string()),
                    summary: "allow command".to_string(),
                    request_digest: "reqdigest1234".to_string(),
                    timeout_ms: 1000,
                    default_decision: PermissionDecision::Deny,
                }),
            ),
            envelope(
                4,
                Some("corr-1"),
                EventV1::PermissionResolved(PermissionResolvedEvent {
                    permission_id: "perm_1".to_string(),
                    decision: PermissionDecision::Allow,
                    reason: Some("approved".to_string()),
                }),
            ),
            envelope(
                5,
                Some("corr-1"),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_1".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: None,
                }),
            ),
            envelope(
                6,
                Some("corr-1"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_1".to_string(),
                    result_summary: "done".to_string(),
                    result_digest: "resultdigest".to_string(),
                }),
            ),
            envelope(
                7,
                Some("corr-1"),
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "ok".to_string(),
                }),
            ),
        ];

        let summary = project_run_summary(events.iter()).expect("project summary");
        let timeline = project_timeline_index(events.iter()).expect("project timeline");

        assert_eq!(summary.status, RunStatus::Finished);
        assert!(summary.tasks_in_flight.is_empty());
        assert!(summary.pending_permissions.is_empty());
        assert_eq!(timeline.events.len(), 7);
        assert_eq!(
            timeline.correlation_groups.get("corr-1"),
            Some(&vec![2, 3, 4, 5, 6, 7])
        );
    }

    #[test]
    fn projections_require_strict_seq_order() {
        let events = [
            envelope(
                2,
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "demo".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                1,
                None,
                EventV1::RunFailed(RunFailedEvent {
                    error: "out of order".to_string(),
                }),
            ),
        ];

        let err = project_run_summary(events.iter()).expect_err("must reject non-monotonic seq");
        assert!(matches!(
            err,
            ProjectionError::NonMonotonicSeq {
                previous: 2,
                current: 1
            }
        ));
    }

    fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{seq:04}"),
            seq,
            run_id: "run_projection".to_string(),
            mono_ms: seq,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: correlation_id.map(str::to_string),
            causation_id: None,
            stream_key: Some("run:run_projection".to_string()),
            payload,
        }
    }

    fn fixture_jsonl() -> &'static str {
        r#"{"schema_version":1,"event_id":"evt-0001","seq":1,"run_id":"run_fixture","mono_ms":1,"actor":{"kind":"system","agent_id":"coordinator"},"stream_key":"run:run_fixture","payload":{"event_type":"run_started","data":{"run_name":"fixture","workspace_root":"/workspace/project"}}}
{"schema_version":1,"event_id":"evt-0002","seq":2,"run_id":"run_fixture","mono_ms":2,"actor":{"kind":"system","agent_id":"coordinator"},"correlation_id":"corr-1","stream_key":"task:task_1","payload":{"event_type":"task_scheduled","data":{"task_id":"task_1","state":"started"}}}
{"schema_version":1,"event_id":"evt-0003","seq":3,"run_id":"run_fixture","mono_ms":3,"actor":{"kind":"system","agent_id":"coordinator"},"correlation_id":"corr-1","stream_key":"permission:perm_1","payload":{"event_type":"permission_requested","data":{"permission_id":"perm_1","kind":"shell","tool_call_id":"toolcall_1","summary":"allow command","request_digest":"reqdigest1234","timeout_ms":1000,"default_decision":"deny"}}}
{"schema_version":1,"event_id":"evt-0004","seq":4,"run_id":"run_fixture","mono_ms":4,"actor":{"kind":"system","agent_id":"coordinator"},"correlation_id":"corr-1","stream_key":"permission:perm_1","payload":{"event_type":"permission_resolved","data":{"permission_id":"perm_1","decision":"allow","reason":"approved"}}}
{"schema_version":1,"event_id":"evt-0005","seq":5,"run_id":"run_fixture","mono_ms":5,"actor":{"kind":"system","agent_id":"coordinator"},"correlation_id":"corr-1","stream_key":"task:task_1","payload":{"event_type":"task_completed","data":{"task_id":"task_1","result_summary":"done","result_digest":"resultdigest"}}}
{"schema_version":1,"event_id":"evt-0006","seq":6,"run_id":"run_fixture","mono_ms":6,"actor":{"kind":"system","agent_id":"coordinator"},"stream_key":"run:run_fixture","payload":{"event_type":"run_finished","data":{"summary":"ok"}}}"#
    }
}
