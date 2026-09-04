use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderReasoningDeltaEvent,
    ProviderRequestStartedEvent, ProviderStreamDeltaEvent, TaskScheduleState, TaskScheduledEvent,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::app::{AppState, LaunchMetadata};
use harness_tui::layout::FrameLayoutPlan;
use harness_tui::render_test::{render_to_buffer, render_to_string};
use harness_tui::ui;
use ratatui::layout::Rect;
use ratatui::style::Color;
use std::time::Duration;

const WIDTH: u16 = 120;
const HEIGHT: u16 = 40;

fn envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-pre-response-{seq:04}"),
        seq,
        run_id: "run_pre_response".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("pre-response-test".to_string())),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: Some("run:run_pre_response".to_string()),
        payload,
    }
}

fn render(app: &AppState) -> String {
    render_to_string(app, Rect::new(0, 0, WIDTH, HEIGHT), |app, frame, _area| {
        ui::render_app(frame, app);
    })
}

fn status_row<'a>(screen: &'a str, label: &str) -> Option<&'a str> {
    screen.lines().rev().find(|row| row.contains(label))
}

fn dock_status_text(app: &AppState) -> Option<String> {
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

fn status_text_color(app: &AppState, text: &str, occurrence: usize) -> Option<Color> {
    let area = Rect::new(0, 0, WIDTH, HEIGHT);
    let status = FrameLayoutPlan::for_app(app, area).status?;
    let buffer = render_to_buffer(app, area, |app, frame, _area| {
        ui::render_app(frame, app);
    });
    let row_start = usize::from(status.y) * usize::from(WIDTH);
    let row = &buffer.content[row_start..row_start + usize::from(WIDTH)];
    let text = text
        .chars()
        .map(|character| character.to_string())
        .collect::<Vec<_>>();
    row.windows(text.len())
        .enumerate()
        .filter(|(_, cells)| {
            cells
                .iter()
                .zip(&text)
                .all(|(cell, character)| cell.symbol() == character)
        })
        .nth(occurrence)
        .map(|(column, _)| row[column].fg)
}

fn status_glyph_color_before_label(app: &AppState, label: &str) -> Option<Color> {
    let area = Rect::new(0, 0, WIDTH, HEIGHT);
    let status = FrameLayoutPlan::for_app(app, area).status?;
    let buffer = render_to_buffer(app, area, |app, frame, _area| {
        ui::render_app(frame, app);
    });
    let row_start = usize::from(status.y) * usize::from(WIDTH);
    let row = &buffer.content[row_start..row_start + usize::from(WIDTH)];
    let label = label
        .chars()
        .map(|character| character.to_string())
        .collect::<Vec<_>>();
    let label_column = row.windows(label.len()).position(|cells| {
        cells
            .iter()
            .zip(&label)
            .all(|(cell, character)| cell.symbol() == character)
    })?;
    row[..label_column]
        .iter()
        .rev()
        .find(|cell| !cell.symbol().trim().is_empty())
        .map(|cell| cell.fg)
}

fn submitted_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-pre-response").with_mode_label("Test"),
    );
    for character in "show working state".chars() {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(character),
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));
    app
}

fn adopt_submitted_turn(app: &mut AppState, request_id: &str) {
    app.ingest_event(envelope(
        1,
        request_id,
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "show working state".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        request_id,
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_pre_response".into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:mock:model-pre-response".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        request_id,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-pre-response".to_string(),
            prompt_summary: "show working state".to_string(),
            request_digest: "digest-pre-response".to_string(),
            metadata: None,
        }),
    ));
}

#[test]
fn submit_immediately_shows_waiting_state_before_any_runtime_event() {
    // arrange
    // Given: a live prompt submitted before the coordinator emits an event.
    let app = submitted_app();

    // When: the synchronous post-submit frame is rendered.
    let screen = render(&app);

    // act
    // assert
    assert!(status_row(&screen, "Waiting for response…").is_some());
    assert!(dock_status_text(&app)
        .as_deref()
        .is_some_and(|row| row.contains("Waiting for response…")));
    assert_eq!(
        status_text_color(&app, "Waiting for response…", 0),
        Some(app.theme().terminal_colors.prompt_accent)
    );
}

#[test]
fn submitted_wait_matches_grok_phase_and_turn_timers() {
    // Given: a locally submitted turn is adopted before the provider's first token.
    let mut app = submitted_app();
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(400));
    adopt_submitted_turn(&mut app, "req_waiting_timers");
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(100));

    // When: the waiting row is rendered after half a second.
    let row = dock_status_text(&app).expect("waiting status row");

    // Then: Grok's phase timer follows the label and its turn timer stays right-aligned.
    assert!(
        row.contains("Waiting for response… 0.5s"),
        "status row: {row:?}"
    );
    assert!(
        row.trim_end().ends_with("0.5s [stop]"),
        "status row: {row:?}"
    );
    assert_eq!(
        status_text_color(&app, "0.5s", 0),
        Some(app.theme().terminal_colors.secondary)
    );
    assert_eq!(
        status_text_color(&app, "0.5s", 1),
        Some(app.theme().terminal_colors.secondary)
    );
}

#[test]
fn narrow_waiting_row_truncates_label_before_dropping_timers() {
    // Given: an active model wait rendered in a narrow status row.
    let mut app = submitted_app();
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(400));
    adopt_submitted_turn(&mut app, "req_narrow_waiting");
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(100));
    let area = Rect::new(0, 0, 30, HEIGHT);
    let status = FrameLayoutPlan::for_app(&app, area)
        .status
        .expect("narrow waiting status row");

    // When: the shell renders the constrained row.
    let buffer = render_to_buffer(&app, area, |app, frame, _area| {
        ui::render_app(frame, app);
    });
    let row_start = usize::from(status.y) * usize::from(area.width) + usize::from(status.x);
    let row: String = buffer.content[row_start..row_start + usize::from(status.width)]
        .iter()
        .map(|cell| cell.symbol())
        .collect();

    // Then: both non-truncating Grok timers survive and only the activity label yields.
    assert_eq!(row.matches("0.5s").count(), 2, "status row: {row:?}");
    assert!(
        row.trim_end().ends_with("0.5s [stop]"),
        "status row: {row:?}"
    );
}

#[test]
fn pre_provider_runtime_events_keep_the_dock_waiting_status_visible() {
    // arrange
    // Given: a locally submitted turn is adopted by the runtime before the provider starts.
    let request_id = "req_pre_provider";
    let mut app = submitted_app();
    app.ingest_event(envelope(
        1,
        request_id,
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "show working state".to_string(),
        }),
    ));

    // When: coordinator setup schedules the provider task without a provider-start event yet.
    app.ingest_event(envelope(
        2,
        request_id,
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_pre_provider".into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:mock:model-pre-response".to_string()),
            metadata: None,
        }),
    ));

    // act
    // Then: the active turn still owns the bottom-left dock status.
    // assert
    assert!(dock_status_text(&app)
        .as_deref()
        .is_some_and(|row| row.contains("Waiting for response…")));
}

#[test]
fn cancelling_turn_keeps_spinner_and_uses_error_accent() {
    // arrange
    // Given: a cancellable provider turn is waiting for its first response.
    let request_id = "req_cancelling";
    let mut app = AppState::new_live(None, false, Some(std::sync::Arc::new(|_| {})));
    app.ingest_event(envelope(
        1,
        request_id,
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_cancelling".into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:mock:model-pre-response".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        request_id,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-pre-response".to_string(),
            prompt_summary: "cancel this turn".to_string(),
            request_digest: "digest-cancelling".to_string(),
            metadata: None,
        }),
    ));

    // When: the operator cancels the active turn before a response arrives.
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('c'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    // act
    let screen = render(&app);
    let row = status_row(&screen, "Cancelling…")
        .unwrap_or_else(|| panic!("expected cancelling status row in {screen:?}"));
    // assert
    assert!(row.contains("0.0s"), "status row: {row:?}");
    assert!(row.contains("[stop]"), "status row: {row:?}");
    assert!(app.has_active_animations());
    assert_eq!(
        status_text_color(&app, "Cancelling…", 0),
        Some(app.theme().status.error)
    );
    assert_eq!(
        status_glyph_color_before_label(&app, "Cancelling…"),
        Some(app.theme().status.error)
    );
}
