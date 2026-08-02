//! Reference-parity animation behavior tests.
//!
//! These tests exercise the real AppState animation seams directly. The root
//! clock is 30 Hz; individual surfaces may dwell on a frame for more than one
//! root tick, as the reference does.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast assertions"
)]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestStartedEvent,
    TaskScheduleState, TaskScheduledEvent, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::animation_evidence::{
    capture_fixed_tick_sequence, spinner_glyphs_in_cells, FixedTickPlan,
};
use harness_tui::app::AppState;
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-reference-animation-{seq:04}"),
        seq,
        run_id: "run_reference_animation".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("reference-animation".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_reference_animation".to_string()),
        payload,
    }
}

fn streaming_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    let request_id = "req-reference-animation";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "stream a response".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "reference-model".to_string(),
            prompt_summary: "stream a response".to_string(),
            request_digest: "reference-animation-digest".to_string(),
            metadata: None,
        }),
    ));
    app
}

fn render(app: &AppState) -> String {
    render_to_string(app, Rect::new(0, 0, 120, 32), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn monitor_line(rendered: &str) -> &str {
    rendered
        .lines()
        .find(|line| line.contains("background") && line.contains("still running"))
        .expect("monitor disclosure row")
}

#[test]
fn mode_banner_fade_is_frame_based() {
    // Given: a live shell whose session mode can be changed.
    let mut app = AppState::new_live(None, false, None);

    // When: the mode banner is opened and the 30 Hz root clock advances.
    app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::NONE));
    assert_eq!(app.mode_banner_alpha_for_evidence(), Some(1.0));
    for _ in 0..64 {
        app.advance_animation_tick_for_evidence();
    }

    // Then: the banner fades during its final reference frames and then clears.
    let alpha = app
        .mode_banner_alpha_for_evidence()
        .expect("mode banner remains during fade");
    assert!(
        alpha > 0.0 && alpha < 1.0,
        "expected fade alpha, got {alpha}"
    );
    for _ in 0..5 {
        app.advance_animation_tick_for_evidence();
    }
    assert_eq!(app.mode_banner_alpha_for_evidence(), None);
}

#[test]
fn ambient_toast_lasts_ninety_ticks_and_pauses_behind_overlays() {
    // Given: an informational toast raised by the normal prompt-clear affordance.
    let mut app = AppState::new_live(None, false, None);
    app.composer.prompt_buffer = "draft".to_string();
    app.composer.prompt_cursor = 5;
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.has_active_animations_for_evidence());

    // When: 45 ticks run, then a palette overlay occludes the toast for 30 ticks.
    for _ in 0..45 {
        app.advance_animation_tick_for_evidence();
    }
    app.palette_visible = true;
    for _ in 0..30 {
        app.advance_animation_tick_for_evidence();
    }
    assert!(app.has_active_animations_for_evidence());

    // Then: the paused 45-frame remainder expires only after the overlay closes.
    app.palette_visible = false;
    for _ in 0..44 {
        app.advance_animation_tick_for_evidence();
    }
    assert!(app.has_active_animations_for_evidence());
    app.advance_animation_tick_for_evidence();
    assert!(!app.has_active_animations_for_evidence());
}

#[test]
fn monitor_indicator_uses_reference_pulse_dwell() {
    // Given: one running background task in an otherwise idle live shell.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        None,
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "monitor-task".into(),
            state: TaskScheduleState::Started,
            queue_key: Some("agent:running".to_string()),
        }),
    ));
    assert_eq!(app.orchestration_summary().running, 1);

    // When: the root animation phase advances through one monitor dwell.
    let at_zero = monitor_line(&render(&app)).to_string();
    for _ in 0..7 {
        app.advance_animation_tick_for_evidence();
    }
    let at_seven = monitor_line(&render(&app)).to_string();
    app.advance_animation_tick_for_evidence();
    let at_eight = monitor_line(&render(&app)).to_string();

    // Then: the reference monitor glyph holds for eight 30 Hz ticks.
    assert_eq!(at_zero, at_seven);
    assert_ne!(at_seven, at_eight);
    assert!(at_eight.contains("1 background task still running"));
}

#[test]
fn starting_session_seed_uses_reference_spinner_dwell() {
    // Given: the same streaming wait surface used by the starting-session seed.
    let mut app = streaming_app();
    let plan = FixedTickPlan::new("starting-session-seed", 120, 32, 5);
    let clock = harness_core::clock::FakeClock::new();

    // When: deterministic 30 Hz evidence captures five root frames.
    let sequence = capture_fixed_tick_sequence(&mut app, &clock, &plan)
        .expect("capture starting-session seed sequence");

    // Then: the spinner dwells for four root ticks before moving.
    let glyphs: Vec<char> = sequence
        .frames
        .iter()
        .map(|frame| {
            spinner_glyphs_in_cells(&frame.cells)
                .into_iter()
                .next()
                .expect("streaming seed paints a spinner")
        })
        .collect();
    assert_eq!(glyphs[0], glyphs[3]);
    assert_ne!(glyphs[3], glyphs[4]);
    assert_eq!(sequence.frames[4].mono_ms, 4 * (1_000 / 30));
}
