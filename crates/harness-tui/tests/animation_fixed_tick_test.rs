//! Owner tests for fixed-tick A-ANIMATION infrastructure (corpus expansion).
//!
//! Does **not** claim A-ANIMATION parity. Proves deterministic capture of
//! functionally complete animated surfaces via FakeClock + animation-phase ticks.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "integration owner tests use fail-fast asserts"
)]

use std::path::PathBuf;

use harness_core::clock::{Clock, FakeClock};
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionDecision, PermissionRequestedEvent,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, ToolCallFinishedEvent,
    ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use harness_tui::animation_evidence::{
    assert_sequences_equal, capture_fixed_tick_sequence, read_sequence_artifact,
    spinner_glyphs_in_cells, write_sequence_artifact, AnimationEvidenceError,
    AnimationFrameSequence, FixedTickPlan, ANIMATION_FRAME_SEQUENCE_SCHEMA,
};
use harness_tui::app::AppState;
use harness_tui::design_contract::{MotionKind, DESIGN_TOKENS};
use harness_tui::render_test::{buffer_to_string, render_to_buffer};
use harness_tui::theme::Theme;
use harness_tui::ui;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use tempfile::tempdir;

fn envelope(seq: u64, correlation_id: Option<&str>, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-anim-{seq:04}"),
        seq,
        run_id: "run_anim_stream".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("animation-fixed-tick".to_string())),
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: Some("run:run_anim_stream".to_string()),
        payload,
    }
}

fn streaming_wait_app() -> AppState {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/run_anim_stream")), false, None);
    let request_id = "req-anim-owner";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "Stream a wait indicator".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "mock-model".to_string(),
            prompt_summary: "Stream a wait indicator".to_string(),
            request_digest: "digest-anim-owner".to_string(),
            metadata: None,
        }),
    ));
    assert!(
        app.has_active_animations_for_evidence(),
        "streaming wait must request animation ticks"
    );
    app
}

fn tool_running_events(path: &str) -> Vec<EventEnvelopeV1> {
    let request_id = "req-anim-tool";
    let tool_call_id = "tool-anim-read";
    vec![
        envelope(
            1,
            Some(request_id),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.into(),
                text: format!("Read {path} while tools run"),
            }),
        ),
        envelope(
            2,
            Some(request_id),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "mock".to_string(),
                model_id: "mock-model".to_string(),
                prompt_summary: format!("Read {path} while tools run"),
                request_digest: "digest-anim-tool".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            3,
            Some(request_id),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: tool_call_id.into(),
                tool_id: "read".to_string(),
                args_summary: serde_json::json!({ "path": path }).to_string(),
                args_digest: "digest-anim-tool-args".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            4,
            Some(request_id),
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: tool_call_id.into(),
            }),
        ),
    ]
}

fn tool_running_app() -> AppState {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/run_anim_tool")), false, None);
    for event in tool_running_events("src/main.rs") {
        app.ingest_event(event);
    }
    assert!(
        app.has_active_animations_for_evidence(),
        "tool-running turn must request animation ticks"
    );
    app
}

fn tool_queued_app() -> AppState {
    let mut events = tool_running_events("src/queued.rs");
    events.pop();
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/run_anim_queued")), false, None);
    for event in events {
        app.ingest_event(event);
    }
    app
}

fn tool_replay_app() -> AppState {
    AppState::new_replay(
        PathBuf::from("/tmp/run_anim_tool_replay"),
        tool_running_events("src/replay.rs"),
    )
}

fn tool_finished_app() -> AppState {
    let mut app = tool_running_app();
    app.ingest_event(envelope(
        5,
        Some("req-anim-tool"),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tool-anim-read".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("fn main() {}".to_string()),
            output_digest: Some("digest-anim-tool-output".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        6,
        Some("req-anim-tool"),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req-anim-tool".into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-anim-tool-response".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
    app
}

fn grouped_mixed_app() -> AppState {
    let request_id = "req-anim-group";
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/run_anim_group")), false, None);
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "Run two grouped commands".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "mock-model".to_string(),
            prompt_summary: "Run two grouped commands".to_string(),
            request_digest: "digest-anim-group".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tool-anim-group-ok".into(),
            tool_id: "bash".to_string(),
            args_summary: serde_json::json!({ "command": "true" }).to_string(),
            args_digest: "digest-anim-group-ok".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tool-anim-group-ok".into(),
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some(request_id),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tool-anim-group-ok".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("ok".to_string()),
            output_digest: Some("digest-anim-group-ok-output".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));
    for _ in 0..8 {
        app.advance_animation_tick_for_evidence();
    }
    app.ingest_event(envelope(
        6,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tool-anim-group-fail".into(),
            tool_id: "bash".to_string(),
            args_summary: serde_json::json!({ "command": "false" }).to_string(),
            args_digest: "digest-anim-group-fail".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        7,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tool-anim-group-fail".into(),
        }),
    ));
    app.ingest_event(envelope(
        8,
        Some(request_id),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tool-anim-group-fail".into(),
            status: ToolCallStatus::Failed,
            output_summary: Some("failed".to_string()),
            output_digest: Some("digest-anim-group-fail-output".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));
    app
}

fn render_buffer(app: &AppState, width: u16, height: u16) -> Buffer {
    render_to_buffer(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        ui::render_app(frame, app);
    })
}

fn rail_colors(buffer: &Buffer) -> Vec<Color> {
    buffer
        .content
        .iter()
        .filter(|cell| cell.symbol() == "┃")
        .map(|cell| cell.fg)
        .collect()
}

fn finish_flash_frames() -> u8 {
    DESIGN_TOKENS
        .motion_tokens
        .all
        .iter()
        .find(|token| token.kind == MotionKind::ToolFinishFlash)
        .map_or(0, |token| token.frames)
}

fn permission_wait_app() -> AppState {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/run_anim_perm")), false, None);
    let request_id = "req-anim-perm";
    let tool_call_id = "tool-anim-edit";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "Apply a gated edit".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "mock-model".to_string(),
            prompt_summary: "Apply a gated edit".to_string(),
            request_digest: "digest-anim-perm".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.into(),
            tool_id: "edit".to_string(),
            args_summary: r#"{"path":"demo.txt"}"#.to_string(),
            args_digest: "digest-anim-perm-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some(tool_call_id),
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm-anim-edit".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some(tool_call_id.into()),
            summary: "Apply hashline edit to demo.txt".to_string(),
            request_digest: "digest-anim-perm-req".to_string(),
            timeout_ms: 30_000,
            default_decision: PermissionDecision::Deny,
        }),
    ));
    assert!(
        app.active_permission().is_some(),
        "permission-wait fixture must surface an active permission"
    );
    assert!(
        !app.has_active_animations_for_evidence(),
        "permission-wait freezes tool motion and must not request redraw ticks"
    );
    app
}

fn startup_idle_app() -> AppState {
    let app = AppState::new_startup(Vec::new(), None);
    assert!(
        app.startup_shell_visible(),
        "startup idle fixture must paint the startup shell"
    );
    assert!(
        app.has_active_animations_for_evidence(),
        "visible startup logo must request slow animation ticks"
    );
    app
}

#[test]
fn startup_logo_requests_the_reference_slow_tick() {
    // arrange
    // Given: the visible startup welcome logo.
    let app = AppState::new_startup(Vec::new(), None);

    // When: the two runtime animation authorities inspect the same state.
    let interval = app.animation_tick_interval_with_motion_for_evidence(true);
    let animation_active = app.has_active_animations_with_motion_for_evidence(true);

    // act
    // Then: both authorities arm Grok's 83 ms slow redraw cadence.
    // assert
    assert_eq!(interval, Some(std::time::Duration::from_millis(83)));
    assert!(animation_active);
}

fn capture_pair(
    plan: &FixedTickPlan,
    build: impl Fn() -> AppState,
) -> (AnimationFrameSequence, AnimationFrameSequence, FakeClock) {
    let mut app_a = build();
    let clock_a = FakeClock::new();
    let sequence_a = capture_fixed_tick_sequence(&mut app_a, &clock_a, plan).expect("capture A");

    let mut app_b = build();
    let clock_b = FakeClock::new();
    let sequence_b = capture_fixed_tick_sequence(&mut app_b, &clock_b, plan).expect("capture B");

    (sequence_a, sequence_b, clock_a)
}

#[test]
fn streaming_wait_spinner_fixed_tick_sequence_is_deterministic() {
    // arrange
    // act
    // assert
    // Given: functionally complete streaming wait indicator surface
    let plan = FixedTickPlan::new("streaming-wait-spinner", 100, 24, 6).with_tick_ms(100);

    // When: two independent fixed-tick captures
    let (sequence_a, sequence_b, clock_a) = capture_pair(&plan, streaming_wait_app);

    // Then: schema, clock, phase, spinner motion, and cross-run equality
    assert_eq!(sequence_a.schema_version, ANIMATION_FRAME_SEQUENCE_SCHEMA);
    assert_eq!(sequence_a.frames.len(), 6);
    assert_eq!(sequence_a.frames[0].animation_phase, 0);
    assert_eq!(sequence_a.frames[5].animation_phase, 5);
    assert_eq!(sequence_a.frames[5].mono_ms, 500);
    assert_eq!(clock_a.mono_ms(), 500);

    let phase_glyphs: Vec<char> = sequence_a
        .frames
        .iter()
        .map(|frame| {
            spinner_glyphs_in_cells(&frame.cells)
                .into_iter()
                .next()
                .expect("each streaming frame paints a spinner glyph")
        })
        .collect();
    assert_eq!(phase_glyphs[0], phase_glyphs[1]);
    assert_ne!(
        phase_glyphs[0], phase_glyphs[4],
        "spinner glyph must change at the four-root-tick cadence: {phase_glyphs:?}"
    );

    assert_sequences_equal(&sequence_a, &sequence_b)
        .expect("independent captures must be byte-stable");

    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("streaming-wait-spinner.frames.json");
    write_sequence_artifact(&path, &sequence_a).expect("write artifact");
    assert!(path.is_file());
    let loaded = read_sequence_artifact(&path).expect("read artifact");
    assert_eq!(loaded, sequence_a);
}

#[test]
fn tool_running_spinner_fixed_tick_sequence_is_deterministic() {
    // arrange
    // act
    // assert
    // Given: running read tool paints a braille spinner distinct from pure streaming wait
    let plan = FixedTickPlan::new("tool-running-spinner", 100, 24, 6).with_tick_ms(100);

    // When: two independent fixed-tick captures
    let (sequence_a, sequence_b, clock_a) = capture_pair(&plan, tool_running_app);

    // Then: schema, FakeClock, phase advance, tool chrome, spinner motion, equality
    assert_eq!(sequence_a.schema_version, ANIMATION_FRAME_SEQUENCE_SCHEMA);
    assert_eq!(sequence_a.surface_id, "tool-running-spinner");
    assert_eq!(sequence_a.frames.len(), 6);
    assert_eq!(sequence_a.frames[0].animation_phase, 0);
    assert_eq!(sequence_a.frames[5].animation_phase, 5);
    assert_eq!(sequence_a.frames[5].mono_ms, 500);
    assert_eq!(clock_a.mono_ms(), 500);

    assert!(
        sequence_a.frames[0].cells.contains("Read")
            || sequence_a.frames[0].cells.contains("src/main.rs"),
        "tool-running surface must paint the running read tool row\n{}",
        sequence_a.frames[0].cells
    );

    let phase_glyphs: Vec<char> = sequence_a
        .frames
        .iter()
        .map(|frame| {
            spinner_glyphs_in_cells(&frame.cells)
                .into_iter()
                .next()
                .expect("each tool-running frame paints a spinner glyph")
        })
        .collect();
    assert_eq!(phase_glyphs[0], phase_glyphs[1]);
    assert_ne!(
        phase_glyphs[0], phase_glyphs[4],
        "tool-running spinner must change at the four-root-tick cadence: {phase_glyphs:?}"
    );

    assert_sequences_equal(&sequence_a, &sequence_b)
        .expect("independent tool-running captures must be byte-stable");

    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("tool-running-spinner.frames.json");
    write_sequence_artifact(&path, &sequence_a).expect("write artifact");
    let loaded = read_sequence_artifact(&path).expect("read artifact");
    assert_eq!(loaded, sequence_a);
}

#[test]
fn running_tool_rail_advances_while_waiting_rail_stays_paused() {
    // arrange
    // Given: otherwise equivalent running and permission-waiting tool rows.
    let mut running = tool_running_app();
    let mut waiting = permission_wait_app();

    // When: both surfaces advance by one deterministic root tick.
    let running_before_buffer = render_buffer(&running, 100, 24);
    let running_before = rail_colors(&running_before_buffer);
    let waiting_before = rail_colors(&render_buffer(&waiting, 100, 28));
    running.advance_animation_tick_for_evidence();
    waiting.advance_animation_tick_for_evidence();
    let running_after_buffer = render_buffer(&running, 100, 24);
    let running_after = rail_colors(&running_after_buffer);
    let waiting_after = rail_colors(&render_buffer(&waiting, 100, 28));

    // act
    // Then: running travels, while waiting uses a distinct frozen semantic cue.
    // assert
    assert!(
        !running_before.is_empty(),
        "running tool must paint an accent rail"
    );
    assert_ne!(
        running_before, running_after,
        "running rail must advance across ticks"
    );
    assert!(
        !waiting_before.is_empty(),
        "waiting tool must paint a paused rail"
    );
    assert_eq!(
        waiting_before, waiting_after,
        "waiting rail must remain frozen"
    );
    assert_ne!(
        running_before, waiting_before,
        "waiting must not reuse running semantics"
    );
    let running_before_text = buffer_to_string(&running_before_buffer, 100);
    let running_after_text = buffer_to_string(&running_after_buffer, 100);
    let stable_before = running_before_text
        .lines()
        .filter(|line| !line.contains("Run read"))
        .collect::<Vec<_>>();
    let stable_after = running_after_text
        .lines()
        .filter(|line| !line.contains("Run read"))
        .collect::<Vec<_>>();
    assert_eq!(
        stable_before, stable_after,
        "rail-only motion must preserve unaffected glyph and cell positions"
    );
}

#[test]
fn running_context_group_animates_marker_but_keeps_label_muted() {
    // arrange
    // act
    let buffer = render_buffer(&tool_running_app(), 100, 24);
    let width = usize::from(buffer.area.width);
    let marker_index = buffer
        .content
        .iter()
        .position(|cell| cell.symbol() == "◈")
        .expect("running context group marker");
    let row_start = marker_index / width * width;
    let rail = buffer.content[row_start..row_start + width]
        .iter()
        .find(|cell| cell.symbol() == "┃")
        .expect("running tool rail");
    let marker = &buffer.content[marker_index];
    let label = &buffer.content[marker_index + 2];

    // assert
    assert_eq!(marker.fg, rail.fg);
    assert_ne!(label.fg, rail.fg);
}

#[test]
fn running_tool_wave_keeps_continuous_time_across_the_legacy_frame_cycle() {
    // arrange
    // Given: a running rail and the authoritative ToolPulse cadence token.
    let mut app = tool_running_app();
    let token = DESIGN_TOKENS
        .motion_tokens
        .all
        .iter()
        .find(|token| token.kind == MotionKind::ToolPulse)
        .copied()
        .expect("ToolPulse token");
    let initial = rail_colors(&render_buffer(&app, 100, 24));

    // When: the former discrete frame cycle elapses.
    for _ in 0..token.frames {
        app.advance_animation_tick_for_evidence();
    }
    let wrapped = rail_colors(&render_buffer(&app, 100, 24));

    // act
    // Then: cadence remains token-owned, while continuous motion does not snap back.
    // assert
    assert_ne!(initial, wrapped);
    assert_eq!(
        app.animation_tick_interval_with_motion_for_evidence(true),
        Some(std::time::Duration::from_millis(u64::from(
            token.interval_ms
        )))
    );
}

#[test]
fn queued_and_replayed_tools_remain_static_without_redraw_demand() {
    // arrange
    // Given: a queued live tool and an active-looking tool reconstructed in replay.
    let mut queued = tool_queued_app();
    let mut replay = tool_replay_app();
    let queued_before = rail_colors(&render_buffer(&queued, 120, 40));
    let replay_before = rail_colors(&render_buffer(&replay, 120, 40));

    // When: both states receive an otherwise valid animation tick.
    queued.advance_animation_tick_for_evidence();
    replay.advance_animation_tick_for_evidence();
    let queued_after = rail_colors(&render_buffer(&queued, 120, 40));
    let replay_after = rail_colors(&render_buffer(&replay, 120, 40));

    // act
    // Then: neither state animates or requests another redraw.
    // assert
    assert_eq!(queued_before, queued_after);
    assert_eq!(replay_before, replay_after);
    assert!(!queued.has_active_animations_for_evidence());
    assert!(!replay.has_active_animations_for_evidence());
}

#[test]
fn restored_terminal_tools_remain_static() {
    // arrange
    // Given: a completed tool reconstructed as live-session history.
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/run_anim_restore")), false, None);
    for event in tool_running_events("src/restored.rs") {
        app.ingest_historical_event(event);
    }

    // When: the terminal events are restored through the historical boundary.
    app.ingest_historical_event(envelope(
        5,
        Some("req-anim-tool"),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tool-anim-read".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("restored output".to_string()),
            output_digest: Some("digest-anim-restored-output".to_string()),
            output_json: None,
            metadata: None,
        }),
    ));
    app.ingest_historical_event(envelope(
        6,
        Some("req-anim-tool"),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req-anim-tool".into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-anim-restored-response".to_string()),
            usage: None,
            metadata: None,
        }),
    ));

    // act
    // Then: restored terminal state is immediately static and arms no redraw timer.
    // assert
    assert!(!app.has_active_animations_for_evidence());
    assert_eq!(
        app.animation_tick_interval_with_motion_for_evidence(true),
        None
    );
}

#[test]
fn replacing_history_clears_motion_state_and_allows_reused_tool_ids_to_run() {
    // arrange
    // Given: a live tool with active running motion.
    let mut app = tool_running_app();
    assert!(app.has_active_animations_for_evidence());

    // When: the event history is replaced and the same tool id starts again live.
    app.replace_events(Vec::new());

    // act
    // Then: replacement parks stale motion and the reused id can run normally.
    // assert
    assert!(!app.has_active_animations_for_evidence());
    for event in tool_running_events("src/reused.rs") {
        app.ingest_event(event);
    }
    app.record_visible_running_tool_motion_for_evidence(true);
    assert!(app.has_active_animations_for_evidence());
}

include!("support/animation_fixed_tick_test_part2_test.rs");
