use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderStreamDeltaEvent, RunFailedEvent,
    RunFinishedEvent, RunStartedEvent, StaleDetectedEvent, TaskCancelledEvent, TaskScheduleState,
    TaskScheduledEvent,
};
use harness_core::proj::{RunStatus, SessionCatalogEntry, SessionModeSource};
use harness_tui::dashboard::{
    build_dashboard_read_model, DashboardEligibilityRules, DashboardGroupKey, DashboardReadModel,
    DashboardReplayRegistry, DashboardSessionInput, DashboardStatus, SelectionKey,
};
use harness_tui::UnwrapOrAbort;

fn catalog(id: &str, parent: Option<&str>, mode: SessionModeSource) -> SessionCatalogEntry {
    SessionCatalogEntry {
        run_id: id.to_string(),
        run_name: Some(format!("session {id}")),
        status: Some(RunStatus::Running),
        last_updated_at: Some("2026-08-04T00:00:00Z".to_string()),
        workspace_root: Some("/workspace".to_string()),
        profile_preset: Some("build".to_string()),
        provider_model: Some("mock/model".to_string()),
        mode_source: mode,
        is_resumable: true,
        resume_disabled_reason: None,
        artifact_count: 0,
        child_session_count: 0,
        parent_session_id: parent.map(str::to_string),
    }
}

fn session(
    id: &str,
    parent: Option<&str>,
    mode: SessionModeSource,
    events: Vec<EventEnvelopeV1>,
) -> DashboardSessionInput {
    DashboardSessionInput::new(catalog(id, parent, mode), events)
}

fn event(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: format!("evt-{run_id}-{seq}"),
        seq,
        run_id: run_id.into(),
        mono_ms: seq * 10,
        ts: Some(format!("2026-08-04T00:00:{seq:02}Z")),
        actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}

fn started(id: &str, seq: u64) -> EventEnvelopeV1 {
    event(
        id,
        seq,
        EventV1::RunStarted(RunStartedEvent {
            run_name: id.into(),
            workspace_root: "/workspace".to_string(),
        }),
    )
}

#[derive(Clone, Copy)]
enum Marker {
    Stream,
    Queued,
    Finished,
    Failed,
    Stale,
    Cancelled,
}

fn marker(id: &str, seq: u64, kind: Marker) -> EventEnvelopeV1 {
    let payload = match kind {
        Marker::Stream => EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req_stream".into(),
            delta: "reply".to_string(),
        }),
        Marker::Queued => EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: format!("task_{id}").into(),
            state: TaskScheduleState::Queued,
            queue_key: None,
            metadata: None,
        }),
        Marker::Finished => EventV1::RunFinished(RunFinishedEvent {
            summary: "done".to_string(),
        }),
        Marker::Failed => EventV1::RunFailed(RunFailedEvent {
            error: "failed".to_string(),
        }),
        Marker::Stale => EventV1::StaleDetected(StaleDetectedEvent {
            task_id: format!("task_{id}").into(),
            stale_for_ms: 500,
        }),
        Marker::Cancelled => EventV1::TaskCancelled(TaskCancelledEvent {
            task_id: format!("task_{id}").into(),
            reason: "operator".to_string(),
            task_scope: None,
        }),
    };
    event(id, seq, payload)
}

fn marked_session(
    id: &str,
    parent: Option<&str>,
    mode: SessionModeSource,
    kind: Marker,
) -> DashboardSessionInput {
    session(id, parent, mode, vec![started(id, 1), marker(id, 2, kind)])
}

fn live_input(id: &str, kind: Marker) -> DashboardSessionInput {
    marked_session(id, None, SessionModeSource::InteractiveLive, kind)
}

fn child_input(id: &str, kind: Marker) -> DashboardSessionInput {
    marked_session(id, Some("parent"), SessionModeSource::InteractiveLive, kind)
}

fn foreign_input(id: &str) -> DashboardSessionInput {
    marked_session(id, None, SessionModeSource::ReplayOnly, Marker::Finished).with_foreign(true)
}

fn project(
    registry: &DashboardReplayRegistry,
    rules: &DashboardEligibilityRules,
) -> DashboardReadModel {
    build_dashboard_read_model(registry, rules).unwrap_or_abort()
}

fn registry() -> DashboardReplayRegistry {
    let parent = session(
        "parent",
        None,
        SessionModeSource::InteractiveLive,
        vec![started("parent", 1)],
    );
    let child = child_input("child", Marker::Stream).with_read_through_seq(1);
    let background = child_input("background", Marker::Queued).with_background(true);
    let complete = live_input("finished", Marker::Finished);
    let error = live_input("failed", Marker::Failed);
    let old = live_input("stale", Marker::Stale);
    let stopped = live_input("cancelled", Marker::Cancelled);
    let foreign = foreign_input("foreign");
    DashboardReplayRegistry::from_sessions(vec![
        parent, child, background, complete, error, old, stopped, foreign,
    ])
}

#[test]
fn dashboard_projects_all_session_shapes_and_stable_relationships() {
    let model = project(&registry(), &DashboardEligibilityRules::default());
    let statuses = model.rows.iter().map(|row| row.status).collect::<Vec<_>>();
    for status in [
        DashboardStatus::Running,
        DashboardStatus::Streaming,
        DashboardStatus::Queued,
        DashboardStatus::Cancelled,
        DashboardStatus::Failed,
        DashboardStatus::Stale,
    ] {
        assert!(statuses.contains(&status));
    }
    assert_eq!(
        (
            model
                .row("foreign")
                .expect("foreign")
                .relationship
                .is_foreign,
            model
                .row("background")
                .expect("background")
                .relationship
                .is_background
        ),
        (true, true)
    );
    let child = model.row("child").expect("child");
    assert_eq!(
        child.relationship.parent.as_ref().map(SelectionKey::as_str),
        Some("parent")
    );
    assert_eq!(child.activity.unread_count, 1);
}

#[test]
fn dashboard_sorts_by_status_then_creation_and_falls_back_by_stable_key() {
    let model = project(&registry(), &DashboardEligibilityRules::default());
    let ordered = model
        .rows
        .iter()
        .map(|row| row.selection_key.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ordered,
        vec![
            "parent",
            "child",
            "background",
            "finished",
            "foreign",
            "cancelled",
            "failed",
            "stale"
        ]
    );
    assert_eq!(
        model
            .fallback_selection(Some(&SelectionKey::new("deleted")))
            .as_ref()
            .map(SelectionKey::as_str),
        Some("parent")
    );
}

#[test]
fn dashboard_normalizes_out_of_order_events_and_orphans_deleted_parents() {
    let orphan = session(
        "orphan",
        Some("deleted-parent"),
        SessionModeSource::InteractiveLive,
        vec![
            marker("orphan", 3, Marker::Finished),
            started("orphan", 1),
            marker("orphan", 2, Marker::Stream),
        ],
    );
    let model = project(
        &DashboardReplayRegistry::from_sessions(vec![orphan]),
        &DashboardEligibilityRules::default(),
    );
    let row = model.row("orphan").expect("orphan row");
    assert_eq!(row.status, DashboardStatus::Completed);
    assert_eq!(row.activity.last_event_seq, 3);
    assert_eq!(
        row.relationship.parent.as_ref().map(SelectionKey::as_str),
        Some("deleted-parent")
    );
    let orphan_group = DashboardGroupKey::Orphaned(SelectionKey::new("deleted-parent"));
    assert_eq!(row.relationship.group, orphan_group);
}

#[test]
fn dashboard_eligibility_is_configurable_without_rendered_string_inspection() {
    let rules = DashboardEligibilityRules {
        include_finished: false,
        include_foreign: false,
        ..DashboardEligibilityRules::default()
    };
    let model = project(&registry(), &rules);
    let keys = model
        .rows
        .iter()
        .map(|row| row.selection_key.as_str())
        .collect::<Vec<_>>();
    assert!(!keys.contains(&"finished"));
    assert!(!keys.contains(&"foreign"));
    assert!(
        !model
            .all_rows
            .iter()
            .find(|row| row.selection_key.as_str() == "foreign")
            .expect("foreign row")
            .eligibility
            .is_eligible
    );
}
