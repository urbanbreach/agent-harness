use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, TaskScheduleState, TaskScheduledEvent,
    SCHEMA_VERSION,
};
use harness_tui::{app::AppState, layout::FrameLayoutPlan, render_test::render_to_buffer, ui};
use ratatui::layout::Rect;

fn watcher_event() -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: "evt-live-turn-color-0001".to_string(),
        seq: 1,
        run_id: "run-live-turn-color".into(),
        mono_ms: 1,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("live-turn-color-test".to_string())),
        correlation_id: Some("req-live-turn-color".to_string()),
        causation_id: None,
        stream_key: Some("run:run-live-turn-color".to_string()),
        payload: EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task-watcher".into(),
            state: TaskScheduleState::Started,
            queue_key: Some("background:analysis".to_string()),
            metadata: None,
        }),
    }
}

#[test]
fn watcher_pulse_uses_system_accent_while_label_stays_muted() {
    // arrange
    // act
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(watcher_event());
    let area = Rect::new(0, 0, 100, 30);
    let status = FrameLayoutPlan::for_app(&app, area)
        .status
        .expect("watcher status row");
    let buffer = render_to_buffer(&app, area, |app, frame, _area| {
        ui::render_app(frame, app);
    });
    let icon_x = status.x.saturating_add(2);
    let label_x = icon_x.saturating_add(2);
    let theme = app.theme();

    // assert
    assert_eq!(buffer[(status.x, status.y)].symbol(), " ");
    assert_eq!(buffer[(status.x.saturating_add(1), status.y)].symbol(), " ");
    assert_eq!(buffer[(icon_x, status.y)].fg, theme.status.info);
    assert_eq!(buffer[(label_x, status.y)].fg, theme.text.secondary);
}
