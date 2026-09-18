use super::*;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

#[path = "live_turn_scope_regression.rs"]
mod live_turn_scope_regression_tests;

#[path = "live_turn_lineage_geometry_regression.rs"]
mod live_turn_lineage_geometry_regression_tests;

#[path = "live_turn_watcher_dedup_tests.rs"]
mod live_turn_watcher_dedup_regression_tests;

pub(super) fn ingest_demotable_child_turn(app: &mut AppState) {
    let actor = EventActor::new(ActorKind::Worker, Some("agent_child_demote".to_string()));
    let scheduled = |seq| {
        envelope_with_actor(
            seq,
            "req_child_demote",
            actor.clone(),
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_child_demote".into(),
                state: TaskScheduleState::Started,
                queue_key: Some("provider_model:default:gpt-5.4-mini".to_string()),
                metadata: None,
            }),
        )
    };
    app.ingest_event(scheduled(6));
    app.ingest_event(envelope(
        7,
        "req_child_demote",
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_child_demote".into(),
            result_summary: String::new(),
            result_digest: "digest-task_child_demote".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("tool_foreground_child".to_string()),
                    child_request_id: Some("req_child_demote".to_string()),
                    ..TaskLineageMetadata::default()
                }),
                ..TaskCompletionMetadata::default()
            }),
        }),
    ));
    app.ingest_event(scheduled(8));
}

pub(super) fn clicking_live_turn_watcher_opens_status_dashboard() {
    // Given: a streaming parent turn with background task watchers.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(provider_started(1, "req_watcher", "default", "model"));
    for seq in 2..=4 {
        app.ingest_event(envelope(
            seq,
            "req_watcher",
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: format!("task_watcher_{seq}").into(),
                state: TaskScheduleState::Started,
                queue_key: Some("background:analysis".to_string()),
                metadata: None,
            }),
        ));
    }
    let live = |id: &str, text: &str| {
        RuntimeEvent::Live(Box::new(LiveEventEnvelope {
            event_id: id.into(),
            run_id: "run_app_tests".into(),
            mono_ms: 5,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("app-tests".into())),
            correlation_id: Some("req_watcher".into()),
            causation_id: None,
            stream_key: None,
            payload: LiveEventV1::ProviderTextDelta {
                request_id: "req_watcher".into(),
                delta: text.into(),
            },
        }))
    };
    app.ingest_runtime_event(live("first", "Live dashboard preview"));
    let frame_area = Rect::new(0, 0, 100, 40);
    let cue = ui::live_turn_watching_rect(&app, frame_area).expect("watcher cue");

    // When: the operator double-clicks the watcher while text is still streaming.
    for _ in 0..2 {
        assert!(app.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: cue.x,
                row: cue.y,
                modifiers: KeyModifiers::NONE,
            },
            frame_area,
            None,
            None,
            None,
        ));
    }

    // The preview uses the live projection and continues updating; Escape stays responsive.
    assert!(app.status_dashboard_is_active());
    assert!(render_text(&app, 100, 40).contains("Live dashboard preview"));
    app.ingest_runtime_event(live("next", " keeps updating"));
    assert!(render_text(&app, 100, 40).contains("keeps updating"));
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.status_dashboard_is_active());
}
