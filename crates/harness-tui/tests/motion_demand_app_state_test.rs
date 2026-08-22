use std::time::Duration;

use harness_core::event::{
    EventV1, ProviderReasoningDeltaEvent, ProviderRequestStartedEvent, ProviderStreamDeltaEvent,
};
use harness_tui::app::{AppState, Focus};
use harness_tui::scheduling::{DualClock, MotionCadence, RuntimePacer};

#[path = "motion_demand/support_test.rs"]
mod support;

use support::{envelope, streaming_app};

#[test]
fn app_routes_startup_and_streaming_to_distinct_visible_cadences() {
    // arrange
    // Given: a startup indicator and a response-wait spinner.
    let mut startup = AppState::new_live(None, false, None);
    startup.set_starting_motion_for_evidence(true);
    let streaming = streaming_app();

    // When: their production AppState motion plans are resolved.
    let startup_plan = startup.motion_plan_for_evidence();
    let streaming_plan = streaming.motion_plan_for_evidence();

    // act
    // Then: startup follows the 12fps baseline and response glyphs wake at 133ms.
    // assert
    assert_eq!(
        startup_plan.cadence(),
        MotionCadence::Slow(Duration::from_millis(83))
    );
    assert_eq!(
        streaming_plan.cadence(),
        MotionCadence::Slow(Duration::from_millis(133))
    );
}

#[test]
fn app_visual_sample_changes_only_at_its_natural_cadence_boundary() {
    // arrange
    // Given: a response-wait spinner at the start of one semantic epoch.
    let mut app = streaming_app();
    let initial = app.motion_plan_for_evidence();

    // When: wall time advances to just before and then onto its 133ms boundary.
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(132));
    let before_boundary = app.motion_plan_for_evidence();
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(1));
    let at_boundary = app.motion_plan_for_evidence();

    // act
    // Then: semantic identity stays stable while only the natural visual sample advances.
    // assert
    assert_eq!(before_boundary.revision(), initial.revision());
    assert_eq!(before_boundary.visual_sample(), initial.visual_sample());
    assert_eq!(at_boundary.revision(), initial.revision());
    assert_eq!(at_boundary.visual_sample(), initial.visual_sample() + 1);
}

#[test]
fn reduced_motion_active_stream_has_no_periodic_deadline() {
    // arrange
    // Given: a live stream under the production reduced-motion authority.
    let mut app = streaming_app();
    app.set_reduced_motion_for_evidence(true);

    // When: AppState resolves its motion plan.
    let plan = app.motion_plan_for_evidence();

    // act
    // Then: visual motion is settled and the runtime can park.
    // assert
    assert!(plan.is_none());
    let pacer = RuntimePacer::with_reduced_motion(true);
    assert!(!pacer.needs_poll(DualClock::new().snapshot(), plan));
}

#[test]
fn wall_clock_sampling_is_independent_of_intermediate_polls() {
    // arrange
    // Given: two identical streaming transitions with different poll histories.
    let mut direct = streaming_app();
    let mut stepped = streaming_app();

    // When: one jumps to 264ms and the other reaches it through irregular samples.
    direct.advance_wall_clock_for_motion_evidence(Duration::from_millis(264));
    stepped.advance_wall_clock_for_motion_evidence(Duration::from_millis(31));
    stepped.advance_wall_clock_for_motion_evidence(Duration::from_millis(67));
    stepped.advance_wall_clock_for_motion_evidence(Duration::from_millis(166));

    // act
    // Then: both sample the same semantic epoch, regardless of scheduler polling.
    // assert
    assert_eq!(direct.animation_phase(), stepped.animation_phase());
}

#[test]
fn semantic_epoch_survives_same_phase_delta_and_restarts_on_transition() {
    // arrange
    // Given: a request has entered reasoning and accumulated wall-clock phase.
    let mut app = streaming_app();
    app.ingest_event(envelope(
        3,
        EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: "req-motion".into(),
            delta: "first".into(),
        }),
    ));
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(66));
    let reasoning_phase = app.animation_phase();
    let reasoning_revision = app.motion_revision_for_evidence();

    // When: another reasoning delta arrives, followed by the response transition.
    app.ingest_event(envelope(
        4,
        EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
            request_id: "req-motion".into(),
            delta: " second".into(),
        }),
    ));
    app.advance_wall_clock_for_motion_evidence(Duration::ZERO);
    let repeated_phase = app.animation_phase();
    let repeated_revision = app.motion_revision_for_evidence();
    app.ingest_event(envelope(
        5,
        EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
            request_id: "req-motion".into(),
            delta: "answer".into(),
        }),
    ));
    app.advance_wall_clock_for_motion_evidence(Duration::ZERO);

    // act
    // Then: same-phase input retains its epoch and the semantic transition resets it.
    // assert
    assert_eq!(repeated_phase, reasoning_phase);
    assert_eq!(repeated_revision, reasoning_revision);
    assert_eq!(app.animation_phase(), 0);
}

#[test]
fn new_request_restarts_epoch_and_focus_transition_is_immediate() {
    // arrange
    // Given: one request has accumulated motion and an idle shell changes focus.
    let mut active = streaming_app();
    active.advance_wall_clock_for_motion_evidence(Duration::from_millis(99));
    let mut idle = AppState::new_live(None, false, None);
    idle.focus = Focus::Details;

    // When: a distinct provider request starts.
    active.ingest_event(envelope(
        3,
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req-next".into(),
            provider_id: "mock".into(),
            model_id: "mock".into(),
            prompt_summary: "next".into(),
            request_digest: "digest-next".into(),
            metadata: None,
        }),
    ));
    active.advance_wall_clock_for_motion_evidence(Duration::ZERO);

    // act
    // Then: the new semantic identity restarts while focus owns no motion demand.
    // assert
    assert_eq!(active.animation_phase(), 0);
    assert!(idle.motion_plan_for_evidence().is_none());
}
