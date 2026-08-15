use super::*;

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionDecision, PermissionRequestedEvent,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, RunFailedEvent, TaskCancelledEvent,
    TaskScheduleState, TaskScheduledEvent, TaskTerminalScope, ToolCallRequestedEvent,
    ToolCallStartedEvent, SCHEMA_VERSION,
};

#[derive(Clone, Copy)]
pub(super) struct ExpectedDockRows {
    pub(super) width: u16,
    pub(super) height: u16,
    pub(super) status: u16,
    pub(super) composer: u16,
    pub(super) disclosure: u16,
    pub(super) outer_spacer: u16,
}

pub(super) const VIEWPORTS: [ExpectedDockRows; 7] = [
    ExpectedDockRows {
        width: 120,
        height: 50,
        status: 42,
        composer: 44,
        disclosure: 48,
        outer_spacer: 1,
    },
    ExpectedDockRows {
        width: 120,
        height: 40,
        status: 32,
        composer: 34,
        disclosure: 38,
        outer_spacer: 1,
    },
    ExpectedDockRows {
        width: 100,
        height: 30,
        status: 22,
        composer: 24,
        disclosure: 28,
        outer_spacer: 1,
    },
    ExpectedDockRows {
        width: 80,
        height: 24,
        status: 16,
        composer: 18,
        disclosure: 22,
        outer_spacer: 1,
    },
    ExpectedDockRows {
        width: 79,
        height: 24,
        status: 16,
        composer: 18,
        disclosure: 22,
        outer_spacer: 1,
    },
    ExpectedDockRows {
        width: 60,
        height: 20,
        status: 15,
        composer: 16,
        disclosure: 19,
        outer_spacer: 0,
    },
    ExpectedDockRows {
        width: 140,
        height: 40,
        status: 32,
        composer: 34,
        disclosure: 38,
        outer_spacer: 1,
    },
];

fn envelope(seq: u64, request_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-live-dock-{seq:04}"),
        seq,
        run_id: "run-live-dock".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("live-dock-tests".to_string())),
        correlation_id: request_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run-live-dock".to_string()),
        payload,
    }
}

fn provider_started(seq: u64, request_id: &str) -> EventEnvelopeV1 {
    envelope(
        seq,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "default".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "live dock geometry".to_string(),
            request_digest: format!("digest-{request_id}"),
            metadata: None,
        }),
    )
}

pub(super) fn waiting_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(provider_started(1, "req-waiting"));
    app
}

pub(super) fn interruptible_waiting_app() -> AppState {
    let mut app = waiting_app();
    app.ingest_event(envelope(
        2,
        Some("req-waiting"),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task-waiting".into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:model-1".to_string()),
            metadata: None,
        }),
    ));
    app
}

pub(super) fn parked_app() -> AppState {
    let mut app = waiting_app();
    app.ingest_event(envelope(
        2,
        Some("req-waiting"),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tool-background-output".into(),
            tool_id: "background_output".to_string(),
            args_summary: r#"{"task_id":"bg-1","block":true}"#.to_string(),
            args_digest: "digest-background-output".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some("req-waiting"),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tool-background-output".into(),
        }),
    ));
    app
}

pub(super) fn watcher_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req-watcher"),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task-watcher".into(),
            state: TaskScheduleState::Started,
            queue_key: Some("background:analysis".to_string()),
            metadata: None,
        }),
    ));
    app
}

pub(super) fn permission_app() -> AppState {
    let mut app = waiting_app();
    app.ingest_event(envelope(
        2,
        Some("tool-edit"),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm-edit".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tool-edit".into()),
            summary: "Edit demo.txt".to_string(),
            request_digest: "digest-permission".to_string(),
            timeout_ms: 30_000,
            default_decision: PermissionDecision::Deny,
        }),
    ));
    app
}

pub(super) fn completed_app() -> AppState {
    let mut app = waiting_app();
    app.ingest_event(envelope(
        2,
        Some("req-waiting"),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req-waiting".into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-output".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
    app
}

pub(super) fn failed_app() -> AppState {
    let mut app = waiting_app();
    app.ingest_event(envelope(
        2,
        Some("req-waiting"),
        EventV1::RunFailed(RunFailedEvent {
            error: "provider failed".to_string(),
        }),
    ));
    app
}

pub(super) fn cancelled_app() -> AppState {
    let mut app = waiting_app();
    app.ingest_event(envelope(
        2,
        Some("req-waiting"),
        EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: "req-waiting".into(),
            reason: "operator cancelled".to_string(),
            task_scope: Some(TaskTerminalScope::AgentTurn),
        }),
    ));
    app
}
