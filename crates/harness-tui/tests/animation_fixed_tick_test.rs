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
    ProviderRequestStartedEvent, ToolCallRequestedEvent, ToolCallStartedEvent,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_tui::animation_evidence::{
    assert_sequences_equal, capture_fixed_tick_sequence, read_sequence_artifact,
    spinner_glyphs_in_cells, write_sequence_artifact, AnimationEvidenceError,
    AnimationFrameSequence, FixedTickPlan, ANIMATION_FRAME_SEQUENCE_SCHEMA,
};
use harness_tui::app::AppState;
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

fn tool_running_app() -> AppState {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/run_anim_tool")), false, None);
    let request_id = "req-anim-tool";
    let tool_call_id = "tool-anim-read";
    app.ingest_event(envelope(
        1,
        Some(request_id),
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: "Read a file while tools run".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        Some(request_id),
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "mock-model".to_string(),
            prompt_summary: "Read a file while tools run".to_string(),
            request_digest: "digest-anim-tool".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.into(),
            tool_id: "read".to_string(),
            args_summary: r#"{"path":"src/main.rs"}"#.to_string(),
            args_digest: "digest-anim-tool-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some(request_id),
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: tool_call_id.into(),
        }),
    ));
    assert!(
        app.has_active_animations_for_evidence(),
        "tool-running turn must request animation ticks"
    );
    app
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
        app.has_active_animations_for_evidence(),
        "permission-wait during an active turn must request animation ticks"
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
        "startup idle must request animation ticks for the welcome shimmer"
    );
    app
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
    assert_ne!(
        phase_glyphs[0], phase_glyphs[1],
        "spinner glyph must change across fixed ticks: {phase_glyphs:?}"
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
    assert_ne!(
        phase_glyphs[0], phase_glyphs[1],
        "tool-running spinner glyph must change across fixed ticks: {phase_glyphs:?}"
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
fn startup_logo_remains_static_across_fixed_ticks() {
    // Given: a startup shell whose decorative content is fully settled.
    let plan = FixedTickPlan::new("startup-logo", 100, 30, 5).with_tick_ms(100);

    // When: two independent fixed-tick captures advance the scheduler.
    let (sequence_a, sequence_b, clock_a) = capture_pair(&plan, startup_idle_app);

    // Then: the complete startup surface remains static.
    assert_eq!(sequence_a.schema_version, ANIMATION_FRAME_SEQUENCE_SCHEMA);
    assert_eq!(sequence_a.surface_id, "startup-logo");
    assert_eq!(sequence_a.frames.len(), 5);
    assert_eq!(sequence_a.frames[0].animation_phase, 0);
    assert_eq!(sequence_a.frames[4].animation_phase, 4);
    assert_eq!(sequence_a.frames[4].mono_ms, 400);
    assert_eq!(clock_a.mono_ms(), 400);

    for frame in &sequence_a.frames {
        assert!(
            spinner_glyphs_in_cells(&frame.cells).is_empty(),
            "startup logo must not paint braille spinner motion\n{}",
            frame.cells
        );
        assert!(frame.cells.contains(" ██╗  ██╗"));
        assert!(frame.cells.contains(" ╚═╝  ╚═╝"));
    }
    assert!(sequence_a
        .frames
        .windows(2)
        .all(|frames| frames[0].cells == frames[1].cells));

    assert_sequences_equal(&sequence_a, &sequence_b)
        .expect("independent startup-logo captures must be byte-stable");
}

#[test]
fn permission_wait_fixed_tick_sequence_is_deterministic() {
    // arrange
    // act
    // assert
    // Given: permission dock open during an active turn (spinner stays static while waiting).
    // Modal/waiting chrome is static under phase ticks; capture is still fail-closed deterministic.
    let plan = FixedTickPlan::new("permission-wait", 100, 28, 6).with_tick_ms(100);

    // When: two independent fixed-tick captures
    let (sequence_a, sequence_b, clock_a) = capture_pair(&plan, permission_wait_app);

    // Then: schema, clock, permission chrome, stable cells, equality
    assert_eq!(sequence_a.schema_version, ANIMATION_FRAME_SEQUENCE_SCHEMA);
    assert_eq!(sequence_a.surface_id, "permission-wait");
    assert_eq!(sequence_a.frames.len(), 6);
    assert_eq!(sequence_a.frames[0].animation_phase, 0);
    assert_eq!(sequence_a.frames[5].animation_phase, 5);
    assert_eq!(sequence_a.frames[5].mono_ms, 500);
    assert_eq!(clock_a.mono_ms(), 500);

    let first = &sequence_a.frames[0].cells;
    assert!(
        first.contains("Allow") || first.contains("Edit") || first.contains("demo.txt"),
        "permission-wait surface must paint permission dock chrome\n{first}"
    );

    for frame in &sequence_a.frames {
        assert!(
            spinner_glyphs_in_cells(&frame.cells).is_empty(),
            "permission-wait freezes braille spinner motion\n{}",
            frame.cells
        );
    }
    assert_eq!(
        sequence_a.frames[0].cells, sequence_a.frames[1].cells,
        "permission-wait cells stay stable across phase ticks (no dedicated wait animation yet)"
    );
    assert_eq!(
        sequence_a.frames[0].cells, sequence_a.frames[5].cells,
        "permission-wait must remain stable for the full fixed-tick plan"
    );

    assert_sequences_equal(&sequence_a, &sequence_b)
        .expect("independent permission-wait captures must be byte-stable");

    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("permission-wait.frames.json");
    write_sequence_artifact(&path, &sequence_a).expect("write artifact");
    let loaded = read_sequence_artifact(&path).expect("read artifact");
    assert_eq!(loaded, sequence_a);
}

#[test]
fn empty_fixed_tick_plan_fails_closed() {
    // arrange
    // act
    // assert
    let mut app = streaming_wait_app();
    let clock = FakeClock::new();
    let plan = FixedTickPlan::new("empty", 40, 12, 0);
    let err = capture_fixed_tick_sequence(&mut app, &clock, &plan).expect_err("empty plan");
    assert_eq!(err, AnimationEvidenceError::EmptyPlan);
}

/// Trace ordering: fixed-tick frame traces must be strictly ordered and
/// duplicate-free (monotonic mono_ms and animation phase, every consecutive
/// frame differs from its predecessor).
#[test]
fn fixed_tick_trace_frames_are_strictly_ordered_and_unique() {
    // arrange — fixed-tick plan for an animated spinner surface
    let plan = FixedTickPlan::new("trace-ordering", 100, 24, 8).with_tick_ms(100);

    // act
    let (sequence, _second, _clock) = capture_pair(&plan, streaming_wait_app);

    // assert — strictly increasing clock + phase; no duplicate frames
    assert_eq!(sequence.frames.len(), 8);
    for pair in sequence.frames.windows(2) {
        assert!(
            pair[1].mono_ms > pair[0].mono_ms,
            "trace mono_ms must strictly increase: {} then {}",
            pair[0].mono_ms,
            pair[1].mono_ms
        );
        assert!(
            pair[1].animation_phase > pair[0].animation_phase,
            "animation phase must strictly increase: {} then {}",
            pair[0].animation_phase,
            pair[1].animation_phase
        );
        assert_ne!(
            pair[0].cells, pair[1].cells,
            "consecutive fixed-tick frames must differ (animation must advance)"
        );
    }
}
