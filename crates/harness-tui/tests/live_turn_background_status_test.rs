use std::sync::Arc;
use std::time::Duration;

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, TaskCompletedEvent,
    TaskCompletionMetadata, TaskScheduleState, TaskScheduledEvent, TaskTerminalScope,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::AppState;
use harness_tui::layout::FrameLayoutPlan;
use harness_tui::render_test::render_to_buffer;
use harness_tui::ui;
use ratatui::layout::Rect;

const HEIGHT: u16 = 30;
const WIDTH: u16 = 100;
const REQUEST_ID: &str = "req_live_turn_background";

fn envelope(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-live-turn-background-{seq:04}"),
        seq,
        run_id: "run_live_turn_background".into(),
        mono_ms: seq.saturating_mul(1_000),
        ts: None,
        actor: EventActor::new(
            ActorKind::System,
            Some("background-status-test".to_string()),
        ),
        correlation_id: Some(REQUEST_ID.to_string()),
        causation_id: None,
        stream_key: Some("run:run_live_turn_background".to_string()),
        payload,
    }
}

fn scheduled(task_id: &str, queue_key: &str) -> EventV1 {
    EventV1::TaskScheduled(TaskScheduledEvent {
        task_id: task_id.into(),
        state: TaskScheduleState::Started,
        queue_key: Some(queue_key.to_string()),
        metadata: None,
    })
}

fn completed(task_id: &str, turn_level: bool) -> EventV1 {
    EventV1::TaskCompleted(TaskCompletedEvent {
        task_id: task_id.into(),
        result_summary: format!("{task_id} complete"),
        result_digest: format!("digest-{task_id}"),
        metadata: turn_level.then(|| TaskCompletionMetadata {
            task_scope: Some(TaskTerminalScope::AgentTurn),
            ..TaskCompletionMetadata::default()
        }),
    })
}

fn status_text(app: &AppState) -> Option<String> {
    let area = Rect::new(0, 0, WIDTH, HEIGHT);
    let status = FrameLayoutPlan::for_app(app, area).status?;
    let buffer = render_to_buffer(app, area, |app, frame, _area| {
        ui::render_app(frame, app);
    });
    let row_start = usize::from(status.y) * usize::from(WIDTH) + usize::from(status.x);
    Some(
        buffer.content[row_start..row_start + usize::from(status.width)]
            .iter()
            .map(|cell| cell.symbol())
            .collect(),
    )
}

fn active_app() -> AppState {
    let mut app = AppState::new_live(None, false, Some(Arc::new(|_| {})));
    app.ingest_event(envelope(
        1,
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: REQUEST_ID.into(),
            text: "Show background status".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        scheduled("task_foreground", "provider_model:mock:model-status"),
    ));
    app.ingest_event(envelope(
        3,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: REQUEST_ID.into(),
            provider_id: "mock".to_string(),
            model_id: "model-status".to_string(),
            prompt_summary: "Show background status".to_string(),
            request_digest: "digest-background-status".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        4,
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: REQUEST_ID.into(),
            delta: "Background-aware response".to_string(),
        }),
    ));
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(4_200));
    app
}

#[test]
fn background_only_work_keeps_one_status_row_until_completion() {
    // arrange
    // Given: a live app with one non-turn-level task and no foreground request.
    let mut app = AppState::new_live(None, false, Some(Arc::new(|_| {})));
    app.ingest_event(envelope(
        1,
        scheduled("task_background", "background:analysis"),
    ));

    // When: the background-only state renders.
    let row = status_text(&app).expect("background status row");

    // Then: it reports work without foreground-only controls or synthetic timing.
    assert!(row.contains("1 command still running"), "row: {row:?}");
    assert!(row.contains("○ 1 command still running"), "row: {row:?}");
    assert!(!row.contains("[stop]"), "row: {row:?}");
    assert!(!row.contains("0.0s"), "row: {row:?}");

    // act
    app.ingest_event(envelope(2, completed("task_background", false)));
    // assert
    assert!(status_text(&app).is_none());
}

#[test]
fn foreground_completion_keeps_background_status_until_background_completion() {
    // arrange
    // Given: an active foreground response with a separate background task.
    let mut app = active_app();
    app.ingest_event(envelope(
        5,
        scheduled("task_background", "background:follow-up"),
    ));

    // When: the foreground provider and turn task complete.
    app.ingest_event(envelope(
        6,
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: REQUEST_ID.into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-completed-response".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(7, completed("task_foreground", true)));

    // Then: only the remaining background work is presented.
    let row = status_text(&app).expect("post-turn background status row");
    assert!(row.contains("1 command still running"), "row: {row:?}");
    assert!(!row.contains("[stop]"), "row: {row:?}");
    assert!(!row.contains("4.2s"), "row: {row:?}");

    // act
    app.ingest_event(envelope(8, completed("task_background", false)));
    // assert
    assert!(status_text(&app).is_none());
}

#[test]
fn foreground_work_wins_over_concurrent_background_work_until_it_completes() {
    // arrange
    // Given: foreground and background tasks overlap before provider activity begins.
    let mut app = AppState::new_live(None, false, Some(Arc::new(|_| {})));
    app.ingest_event(envelope(
        1,
        scheduled("task_foreground", "provider_model:mock:model-status"),
    ));
    app.ingest_event(envelope(
        2,
        scheduled("task_background", "background:analysis"),
    ));

    // Then: the interruptible foreground presentation owns the row and stop control.
    let foreground = status_text(&app).expect("foreground waiting status row");
    assert!(
        foreground.contains("Waiting for response…"),
        "row: {foreground:?}"
    );
    assert!(foreground.contains("[stop]"), "row: {foreground:?}");
    assert!(
        !foreground.contains("Watching background"),
        "row: {foreground:?}"
    );

    // When: foreground work completes while background work remains.
    app.ingest_event(envelope(3, completed("task_foreground", true)));

    // Then: the row transitions to background-only presentation without stop.
    let background = status_text(&app).expect("background-only status row");
    assert!(
        background.contains("1 command still running"),
        "row: {background:?}"
    );
    assert!(!background.contains("[stop]"), "row: {background:?}");

    // act
    app.ingest_event(envelope(4, completed("task_background", false)));
    // assert
    assert!(status_text(&app).is_none());
}
