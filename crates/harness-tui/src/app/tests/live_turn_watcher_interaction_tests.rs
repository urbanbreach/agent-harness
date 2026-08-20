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
    // Given: an idle live shell with one background task watcher.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_watcher",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_watcher".into(),
            state: TaskScheduleState::Started,
            queue_key: Some("background:analysis".to_string()),
            metadata: None,
        }),
    ));
    let frame_area = Rect::new(0, 0, 100, 30);
    let cue = ui::live_turn_watching_rect(&app, frame_area).expect("watcher cue");

    // When: the operator clicks the persistent watcher cue.
    let handled = app.handle_mouse(
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
    );

    // Then: Harness opens its existing task/status dashboard.
    assert!(handled);
    assert!(app.status_dashboard_is_active());
}
