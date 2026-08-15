use crate::UnwrapOrAbort;
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod background_projection;
pub use background_projection::{
    project_background_request, resolve_all_background_request_refs,
    resolve_background_request_ref, BackgroundRequestProjection, BackgroundRequestProjectionError,
    BackgroundRequestRef, BackgroundToolCallCounts,
};

mod run_projection;
pub use run_projection::{
    project_run_summary, project_timeline_index, RunCounts, RunSummary, TimelineEventRef,
    TimelineIndex,
};

mod resume_projection;
pub use resume_projection::{
    inspect_resume_plan, project_resume_plan, ChildSessionTerminalState, LifecycleSegmentStatus,
    ResumeArtifactSnapshot, ResumeBackgroundTaskNotificationSnapshot, ResumeChildSessionSnapshot,
    ResumeIdWatermarks, ResumePlan, ResumeProviderLifecycleMetadata, ResumeTaskSnapshot,
    ResumeToolCallSnapshot,
};

mod session_catalog_projection;
pub use session_catalog_projection::{
    load_run_metadata, project_session_catalog_entry, RecordedRuntimeContext, RunMetadata,
    SessionCatalogEntry, SessionCatalogMetadata, SessionModeSource,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Finished,
    Failed,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProjectionError {
    #[error("events must be strictly increasing by seq: previous={previous}, current={current}")]
    NonMonotonicSeq { previous: u64, current: u64 },
    #[error("events must be contiguous by seq: expected={expected}, current={current}")]
    NonContiguousSeq { expected: u64, current: u64 },
    #[error("events contain multiple run ids: expected={expected}, actual={actual}")]
    RunIdMismatch { expected: String, actual: String },
    #[error(
        "invalid {counter_kind} id `{id}`; expected prefix `{expected_prefix}` followed by digits"
    )]
    InvalidCounterId {
        counter_kind: &'static str,
        id: String,
        expected_prefix: &'static str,
    },
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
    use super::{
        project_background_request, project_resume_plan, project_run_summary,
        project_timeline_index, resolve_background_request_ref, BackgroundRequestProjectionError,
        ChildSessionTerminalState, ProjectionError, RunStatus,
    };
    use crate::event::{
        ActorKind, AgentSpawnedEvent, BackgroundTaskNotificationEvent,
        BackgroundTaskNotificationStatus, EventActor, EventEnvelopeV1, EventV1, PermissionDecision,
        PermissionRequestedEvent, PermissionResolvedEvent, RunFailedEvent, RunFinishedEvent,
        RunStartedEvent, TaskCancelledEvent, TaskCompletedEvent, TaskResultLateEvent,
        TaskScheduleState, TaskScheduledEvent, TaskTerminalScope, ToolCallFinishedEvent,
        ToolCallRequestedEvent, ToolCallStatus, SCHEMA_VERSION,
    };
    use crate::UnwrapOrAbort;

    #[test]
    fn applying_same_jsonl_twice_yields_identical_run_summary() {
        // arrange
        // act
        // assert
        let jsonl = fixture_jsonl();
        let first: Vec<EventEnvelopeV1> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap_or_abort())
            .collect();
        let second: Vec<EventEnvelopeV1> = jsonl
            .lines()
            .map(|line| serde_json::from_str(line).unwrap_or_abort())
            .collect();

        let summary_a = project_run_summary(first.iter()).unwrap_or_abort();
        let summary_b = project_run_summary(second.iter()).unwrap_or_abort();

        assert_eq!(summary_a, summary_b);
        assert_eq!(summary_a.status, RunStatus::Finished);
        assert!(summary_a.tasks_in_flight.is_empty());
        assert!(summary_a.pending_permissions.is_empty());
    }

    #[test]
    fn projections_ignore_side_effects_during_replay() {
        // arrange
        // act
        // assert
        let events = [
            envelope(
                1,
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "demo".into(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                2,
                Some("corr-1"),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_1".into(),
                    tool_id: "shell.run".to_string(),
                    args_summary: "{\"cmd\":\"touch /tmp/should_not_run\"}".to_string(),
                    args_digest: "digest123456".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                3,
                Some("corr-1"),
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "perm_1".to_string(),
                    kind: "shell".to_string(),
                    tool_call_id: Some("toolcall_1".into()),
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
                    task_id: "task_1".to_string().into(),
                    state: TaskScheduleState::Started,
                    queue_key: None,
                    metadata: None,
                }),
            ),
            envelope(
                6,
                Some("corr-1"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_1".to_string().into(),
                    result_summary: "done".to_string(),
                    result_digest: "resultdigest".to_string(),
                    metadata: None,
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

        let summary = project_run_summary(events.iter()).unwrap_or_abort();
        let timeline = project_timeline_index(events.iter()).unwrap_or_abort();

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
        // arrange
        // act
        // assert
        let events = [
            envelope(
                2,
                None,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "demo".into(),
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

    #[test]
    fn background_projection_resolves_lineage_and_terminal_result_from_events() {
        // arrange
        // act
        // assert
        let parent_actor = EventActor::new(ActorKind::Worker, Some("agent_parent".to_string()));
        let child_actor = EventActor::new(ActorKind::Worker, Some("agent_child".to_string()));
        let events = [
            envelope(
                1,
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_parent".to_string(),
                    profile: "deep".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                2,
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "general".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                }),
            ),
            envelope_with_actor(
                3,
                Some("req_child"),
                child_actor.clone(),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string().into(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-1".to_string()),
                    metadata: None,
                }),
            ),
            envelope_with_actor(
                4,
                Some("req_child"),
                child_actor.clone(),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000001".into(),
                    tool_id: "read".to_string(),
                    args_summary: "{}".to_string(),
                    args_digest: "argsdigest".to_string(),
                    metadata: None,
                }),
            ),
            envelope_with_actor(
                5,
                Some("req_child"),
                child_actor.clone(),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000001".into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("read ok".to_string()),
                    output_digest: Some("outdigest".to_string()),
                    output_json: None,
                    metadata: None,
                }),
            ),
            envelope_with_actor(
                6,
                Some("req_child"),
                child_actor,
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string().into(),
                    result_summary: "child done".to_string(),
                    result_digest: "resultdigest".to_string(),
                    metadata: None,
                }),
            ),
        ];

        let request_ref =
            resolve_background_request_ref(events.iter(), &parent_actor, Some("req_child"), None)
                .unwrap_or_abort();
        let projection = project_background_request(events.iter(), &request_ref).unwrap_or_abort();

        assert_eq!(projection.request_id, "req_child".into());
        assert_eq!(projection.session_id.as_deref(), Some("agent_child"));
        assert_eq!(projection.scheduler_task_id.as_deref(), Some("task_000001"));
        assert_eq!(projection.status, "completed");
        assert!(projection.terminal);
        assert_eq!(projection.result_summary.as_deref(), Some("child done"));
        assert_eq!(projection.tool_calls.requested, 1);
        assert_eq!(projection.tool_calls.succeeded, 1);
        assert_eq!(projection.tool_calls.failed, 0);
    }

    #[test]
    fn background_notification_projects_failed_resume_and_request_state() {
        // arrange
        // act
        // assert
        let parent_actor = EventActor::new(ActorKind::Worker, Some("agent_parent".to_string()));
        let child_actor = EventActor::new(ActorKind::Worker, Some("agent_child".to_string()));
        let events = [
            envelope(
                1,
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_parent".to_string(),
                    profile: "deep".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                2,
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "general".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                }),
            ),
            envelope_with_actor(
                3,
                Some("req_child"),
                child_actor,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string().into(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-1".to_string()),
                    metadata: None,
                }),
            ),
            envelope(
                4,
                Some("background_task_notification:req_child"),
                EventV1::BackgroundTaskNotification(BackgroundTaskNotificationEvent {
                    parent_session_id: "agent_parent".into(),
                    parent_agent_id: Some("agent_parent".to_string()),
                    child_session_id: "agent_child".into(),
                    child_request_id: "req_child".to_string(),
                    task_id: "task_000001".to_string().into(),
                    description: "investigate".to_string(),
                    status: BackgroundTaskNotificationStatus::Failed,
                    summary: "provider failed closed".to_string(),
                    terminal_event_id: "evt-terminal".to_string(),
                    terminal_task_id: "task_000001".to_string(),
                    delivered_turn_request_id: Some("req_parent_notice".to_string()),
                }),
            ),
        ];

        let request_ref =
            resolve_background_request_ref(events.iter(), &parent_actor, Some("req_child"), None)
                .unwrap_or_abort();
        let projection = project_background_request(events.iter(), &request_ref).unwrap_or_abort();
        assert_eq!(projection.status, "failed");
        assert!(projection.terminal);
        assert_eq!(projection.session_id.as_deref(), Some("agent_child"));
        assert_eq!(
            projection.failure_summary.as_deref(),
            Some("provider failed closed")
        );

        let plan = project_resume_plan(events.iter(), "run_projection").unwrap_or_abort();
        assert!(plan.tasks_in_flight.is_empty());
        let child = plan.child_sessions.get("agent_child").unwrap_or_abort();
        assert_eq!(
            child.terminal_state,
            Some(ChildSessionTerminalState::Failed)
        );
        assert_eq!(
            child.terminal_reason.as_deref(),
            Some("provider failed closed")
        );
        let notification = child.background_notification.as_ref().unwrap_or_abort();
        assert_eq!(
            notification.status,
            BackgroundTaskNotificationStatus::Failed
        );
        assert_eq!(notification.terminal_event_id, "evt-terminal");
        assert_eq!(
            notification.delivered_turn_request_id.as_deref(),
            Some("req_parent_notice")
        );
    }

    #[test]
    fn background_projection_denies_requests_outside_worker_lineage() {
        // arrange
        // act
        // assert
        let other_actor = EventActor::new(ActorKind::Worker, Some("agent_other".to_string()));
        let events = [
            envelope(
                1,
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "general".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                }),
            ),
            envelope_with_actor(
                2,
                Some("req_child"),
                EventActor::new(ActorKind::Worker, Some("agent_child".to_string())),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string().into(),
                    state: TaskScheduleState::Queued,
                    queue_key: Some("provider_model:mock:model-1".to_string()),
                    metadata: None,
                }),
            ),
        ];

        let err =
            resolve_background_request_ref(events.iter(), &other_actor, Some("req_child"), None)
                .expect_err("unrelated worker cannot read child request");
        assert_eq!(err, BackgroundRequestProjectionError::Unauthorized);
    }

    #[test]
    fn background_request_resolution_prefers_explicit_request_id_over_session_hint() {
        // arrange
        // act
        // assert
        let actor = EventActor::new(ActorKind::Worker, Some("agent_parent".to_string()));
        let child_actor = EventActor::new(ActorKind::Worker, Some("agent_child".to_string()));
        let events = [
            envelope(
                1,
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "general".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                }),
            ),
            envelope_with_actor(
                2,
                Some("req_first"),
                child_actor.clone(),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string().into(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-1".to_string()),
                    metadata: None,
                }),
            ),
            envelope_with_actor(
                3,
                Some("req_second"),
                child_actor,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000002".to_string().into(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-1".to_string()),
                    metadata: None,
                }),
            ),
        ];

        let request_ref = resolve_background_request_ref(
            events.iter(),
            &actor,
            Some("req_first"),
            Some("agent_child"),
        )
        .unwrap_or_abort();

        assert_eq!(request_ref.request_id, "req_first".into());
    }

    #[test]
    fn background_projection_preserves_cancelled_late_result_state() {
        // arrange
        // act
        // assert
        let parent_actor = EventActor::new(ActorKind::Worker, Some("agent_parent".to_string()));
        let child_actor = EventActor::new(ActorKind::Worker, Some("agent_child".to_string()));
        let events = [
            envelope(
                1,
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "general".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                }),
            ),
            envelope_with_actor(
                2,
                Some("req_child"),
                child_actor.clone(),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string().into(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-1".to_string()),
                    metadata: None,
                }),
            ),
            envelope_with_actor(
                3,
                Some("req_child"),
                child_actor.clone(),
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id: "task_000001".to_string().into(),
                    reason: "cancelled by test".to_string(),
                    task_scope: Some(TaskTerminalScope::AgentTurn),
                }),
            ),
            envelope_with_actor(
                4,
                Some("req_child"),
                child_actor,
                EventV1::TaskResultLate(TaskResultLateEvent {
                    task_id: "task_000001".to_string().into(),
                    result_digest: "latedigest".to_string(),
                }),
            ),
        ];

        let request_ref =
            resolve_background_request_ref(events.iter(), &parent_actor, Some("req_child"), None)
                .unwrap_or_abort();
        let projection = project_background_request(events.iter(), &request_ref).unwrap_or_abort();

        assert_eq!(projection.status, "cancelled");
        assert!(projection.terminal);
        assert!(projection.late_result);
        assert_eq!(
            projection.cancel_reason.as_deref(),
            Some("cancelled by test")
        );
        assert_eq!(
            projection.failure_summary.as_deref(),
            Some("cancelled by test")
        );
    }

    #[test]
    fn background_projection_ignores_correlated_tool_task_terminal_events() {
        // arrange
        // act
        // assert
        let parent_actor = EventActor::new(ActorKind::Worker, Some("agent_parent".to_string()));
        let child_actor = EventActor::new(ActorKind::Worker, Some("agent_child".to_string()));
        let events = [
            envelope(
                1,
                None,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "general".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                }),
            ),
            envelope_with_actor(
                2,
                Some("req_child"),
                child_actor.clone(),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string().into(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-1".to_string()),
                    metadata: None,
                }),
            ),
            envelope_with_actor(
                3,
                Some("req_child"),
                child_actor.clone(),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000002".to_string().into(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("tool:read".to_string()),
                    metadata: None,
                }),
            ),
            envelope_with_actor(
                4,
                Some("req_child"),
                child_actor,
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000002".to_string().into(),
                    result_summary: "tool done".to_string(),
                    result_digest: "tooldigest".to_string(),
                    metadata: None,
                }),
            ),
        ];

        let request_ref =
            resolve_background_request_ref(events.iter(), &parent_actor, Some("req_child"), None)
                .unwrap_or_abort();
        let projection = project_background_request(events.iter(), &request_ref).unwrap_or_abort();

        assert_eq!(projection.scheduler_task_id.as_deref(), Some("task_000001"));
        assert_eq!(projection.status, "running");
        assert!(!projection.terminal);
        assert_eq!(projection.result_summary, None);
    }

    fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt-{seq:04}"),
            seq,
            run_id: "run_projection".into(),
            mono_ms: seq,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            correlation_id: correlation_id.map(str::to_string),
            causation_id: None,
            stream_key: Some("run:run_projection".to_string()),
            payload,
        }
    }

    fn envelope_with_actor(
        seq: u64,
        correlation_id: Option<&str>,
        actor: EventActor,
        payload: EventV1,
    ) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            actor,
            ..envelope(seq, correlation_id, payload)
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
