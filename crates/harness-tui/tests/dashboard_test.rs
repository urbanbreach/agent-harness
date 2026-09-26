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
            failure: false,
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
    // arrange
    // act
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
        // assert
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
    // arrange
    // act
    let model = project(&registry(), &DashboardEligibilityRules::default());
    let ordered = model
        .rows
        .iter()
        .map(|row| row.selection_key.as_str())
        .collect::<Vec<_>>();
    // assert
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
    // arrange
    // act
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
    // assert
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
    // arrange
    // act
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
    // assert
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

#[test]
fn completed_live_turn_uses_updated_title_and_leaves_working_roster() {
    let events = vec![
        started("live", 1),
        marker("live", 2, Marker::Stream),
        event(
            "live",
            3,
            EventV1::SessionTitleUpdated(harness_core::event::SessionTitleUpdatedEvent {
                title: "Renamed live session".into(),
            }),
        ),
        event(
            "live",
            4,
            EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
                task_id: "turn".into(),
                result_summary: "done".into(),
                result_digest: "digest".into(),
                metadata: Some(harness_core::event::TaskCompletionMetadata {
                    task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                    ..Default::default()
                }),
            }),
        ),
        marker("live", 5, Marker::Finished),
        started("live", 6),
    ];
    let registry = DashboardReplayRegistry::from_sessions(vec![session(
        "live",
        None,
        SessionModeSource::InteractiveLive,
        events,
    )]);
    let model = build_dashboard_read_model(&registry, &DashboardEligibilityRules::default())
        .unwrap_or_abort();
    let row = model.row("live").unwrap_or_abort();
    assert_eq!(row.title.as_deref(), Some("Renamed live session"));
    assert_eq!(row.status, DashboardStatus::Completed);
}

#[test]
fn shared_child_events_preserve_parent_status_and_root_visibility() {
    use harness_core::event::{
        AgentSpawnedEvent, AgentStoppedEvent, BackgroundTaskNotificationEvent,
        BackgroundTaskNotificationStatus, PermissionDecision, PermissionRequestedEvent,
        ProviderRequestStartedEvent, TaskCompletedEvent, TaskCompletionMetadata,
        TaskLineageMetadata, TaskScheduleMetadata, TaskTerminalScope,
    };
    let root = event(
        "parent",
        2,
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: "owner-agent".into(),
            profile: "build".into(),
            parent_agent_id: None,
        }),
    );
    let mut events = vec![
        started("parent", 1),
        root,
        event(
            "parent",
            3,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "child-agent".into(),
                profile: "build".into(),
                parent_agent_id: Some("owner-agent".into()),
            }),
        ),
    ];
    let terminal = EventV1::TaskCompleted(TaskCompletedEvent {
        task_id: "child-turn".into(),
        result_summary: "done".into(),
        result_digest: "digest".into(),
        metadata: Some(TaskCompletionMetadata {
            task_scope: Some(TaskTerminalScope::AgentTurn),
            lineage: Some(TaskLineageMetadata {
                parent_session_id: Some("parent".into()),
                child_session_id: Some("child".into()),
                ..Default::default()
            }),
            ..Default::default()
        }),
    });
    let notification = EventV1::BackgroundTaskNotification(BackgroundTaskNotificationEvent {
        parent_session_id: "parent".into(),
        parent_agent_id: Some("owner-agent".into()),
        child_session_id: "child".into(),
        child_request_id: "child-request".into(),
        task_id: "child-turn".into(),
        description: "child task".into(),
        status: BackgroundTaskNotificationStatus::Completed,
        summary: "done".into(),
        terminal_event_id: "terminal-child".into(),
        terminal_task_id: "child-turn".into(),
        delivered_turn_request_id: None,
    });
    for blocked in [false, true] {
        if blocked {
            events.push(event(
                "parent",
                4,
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "permission".into(),
                    kind: "edit_fs".into(),
                    tool_call_id: None,
                    summary: "Review edit".into(),
                    request_digest: "digest".into(),
                    timeout_ms: 30000,
                    default_decision: PermissionDecision::Deny,
                }),
            ));
        }
        let expected = if blocked {
            DashboardStatus::AwaitingInput
        } else {
            DashboardStatus::Running
        };
        let mut journal = events.clone();
        for (index, payload) in [
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "child-turn".into(),
                state: TaskScheduleState::Queued,
                queue_key: None,
                metadata: Some(TaskScheduleMetadata {
                    lineage: Some(TaskLineageMetadata {
                        parent_session_id: Some("parent".into()),
                        child_session_id: Some("child".into()),
                        ..Default::default()
                    }),
                }),
            }),
            terminal.clone(),
            EventV1::TaskCancelled(TaskCancelledEvent {
                failure: false,
                task_id: "child-turn".into(),
                reason: "stop".into(),
                task_scope: Some(TaskTerminalScope::AgentTurn),
            }),
            EventV1::AgentStopped(AgentStoppedEvent {
                agent_id: "child-agent".into(),
                reason: "done".into(),
            }),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "child-request".into(),
                provider_id: "mock".into(),
                model_id: "mock".into(),
                prompt_summary: "child".into(),
                request_digest: "digest".into(),
                metadata: None,
            }),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "child-request".into(),
                delta: "reply".into(),
            }),
            notification.clone(),
        ]
        .into_iter()
        .enumerate()
        {
            let mut child_event = event("parent", 10 + index as u64, payload);
            child_event.correlation_id = Some("child-request".into());
            if !matches!(
                child_event.payload,
                EventV1::ProviderStreamDelta(_)
                    | EventV1::ProviderRequestStarted(_)
                    | EventV1::TaskCancelled(_)
                    | EventV1::AgentStopped(_)
                    | EventV1::BackgroundTaskNotification(_)
            ) {
                child_event.actor = EventActor::new(ActorKind::Worker, Some("child-agent".into()));
            }
            journal.push(child_event);
            let model = project(
                &DashboardReplayRegistry::from_sessions(vec![
                    session(
                        "parent",
                        Some("parent"),
                        SessionModeSource::InteractiveLive,
                        journal.clone(),
                    ),
                    session(
                        "child",
                        None,
                        SessionModeSource::InteractiveLive,
                        vec![started("child", 1), event("child", 2, notification.clone())],
                    ),
                ]),
                &DashboardEligibilityRules::default(),
            );
            let parent = model.row("parent").unwrap_or_abort();
            assert_eq!(parent.status, expected, "child event {index}");
            assert!(parent.relationship.parent.is_none());
            assert!(!parent.relationship.is_background);
            assert_eq!(
                parent.relationship.children,
                vec![SelectionKey::new("child")]
            );
            assert_eq!(
                model.row("child").unwrap_or_abort().status,
                DashboardStatus::Completed
            );
        }
        if !blocked {
            if let Some(directory) = std::env::var_os("HARNESS_TOOL_RUNTIME_HARNESS_DIR") {
                use ratatui::{
                    backend::CrosstermBackend, layout::Rect, Terminal, TerminalOptions, Viewport,
                };
                let directory = std::path::PathBuf::from(directory);
                std::fs::create_dir_all(&directory).unwrap_or_abort();
                let mut app = harness_tui::app::AppState::new_replay(
                    directory.join("parent"),
                    journal.clone(),
                );
                for (width, height) in [(120, 40), (60, 20)] {
                    let area = Rect::new(0, 0, width, height);
                    app.open_status_dashboard_at(area);
                    let mut bytes = Vec::new();
                    let mut terminal = Terminal::with_options(
                        CrosstermBackend::new(&mut bytes),
                        TerminalOptions {
                            viewport: Viewport::Fixed(area),
                        },
                    )
                    .unwrap_or_abort();
                    terminal
                        .draw(|frame| harness_tui::ui::render_app(frame, &app))
                        .unwrap_or_abort();
                    drop(terminal);
                    std::fs::write(
                        directory.join(format!(
                            "dashboard-parent-running-{width}x{height}-motion-0ms.ansi"
                        )),
                        bytes,
                    )
                    .unwrap_or_abort();
                }
            }
        }
        let mut own_terminal = terminal.clone();
        if let EventV1::TaskCompleted(task) = &mut own_terminal {
            task.task_id = "owner-turn".into();
            task.metadata.as_mut().unwrap_or_abort().lineage = None;
        }
        let mut own_event = event("parent", 20, own_terminal);
        own_event.actor = EventActor::new(ActorKind::Worker, Some("owner-agent".into()));
        journal.push(own_event);
        let model = project(
            &DashboardReplayRegistry::from_sessions(vec![session(
                "parent",
                None,
                SessionModeSource::InteractiveLive,
                journal,
            )]),
            &DashboardEligibilityRules::default(),
        );
        assert_eq!(
            model.row("parent").unwrap_or_abort().status,
            DashboardStatus::Completed
        );
    }
}
