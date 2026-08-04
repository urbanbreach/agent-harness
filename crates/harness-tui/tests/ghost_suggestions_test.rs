#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner contract tests use direct fail-fast assertions"
)]

use harness_tui::composer_editing::ComposerEditor;
use harness_tui::ghost_suggestions::{
    Invalidation, SecretSuggestionSink, SuggestionController, SuggestionError,
};
use harness_tui::scheduling::DualClock;
use ratatui::style::{Color, Modifier};

fn ready_controller() -> (DualClock, SuggestionController) {
    let clock = DualClock::new();
    let mut controller = SuggestionController::new(100);
    let request = controller
        .on_edit(&clock, "prompt context")
        .expect("edit starts a request");
    clock.advance_flush(100);
    assert_eq!(controller.take_ready_request(&clock), Some(request.clone()));
    controller
        .apply_response(&request, "ghost text")
        .expect("current request accepts its response");
    (clock, controller)
}

#[test]
fn stale_responses_are_rejected_after_a_generation_change() {
    // Given: a request captured before the composer is edited again.
    let clock = DualClock::new();
    let mut controller = SuggestionController::new(50);
    let stale = controller
        .on_edit(&clock, "first context")
        .expect("first request is valid");
    controller
        .on_edit(&clock, "second context")
        .expect("second edit replaces the request");

    // When: the old provider response arrives late.
    let error = controller
        .apply_response(&stale, "old suggestion")
        .expect_err("the old generation must be rejected");

    // Then: no stale text becomes visible.
    assert!(matches!(error, SuggestionError::StaleGeneration { .. }));
    assert!(controller.current().is_none());
}

#[test]
fn generation_counter_bumps_for_edit_focus_and_state_changes() {
    // Given: a fresh generation counter owned by the suggestion controller.
    let clock = DualClock::new();
    let mut controller = SuggestionController::new(50);

    // When: each invalidating composer/lifecycle transition occurs once.
    let edit = controller
        .on_edit(&clock, "edit")
        .expect("edit request is valid");
    let focus = controller
        .on_focus_change()
        .expect("focus invalidation is valid");
    let state = controller
        .on_state_change()
        .expect("state invalidation is valid");

    // Then: each transition owns a strictly newer generation and old work is distinct.
    assert_eq!(edit.generation.value(), 1);
    assert_eq!(focus.value(), 2);
    assert_eq!(state.value(), 3);
}

#[test]
fn partial_acceptance_preserves_complete_grapheme_clusters() {
    // Given: a response containing ZWJ, combining-mark, and modifier clusters.
    let (clock, mut controller) = ready_controller();
    let response = "👨‍👩‍👧‍👦e\u{301}👍🏽";
    let request = controller
        .on_edit(&clock, "prompt context")
        .expect("replacement request is valid");
    clock.advance_flush(100);
    assert_eq!(controller.take_ready_request(&clock), Some(request.clone()));
    controller
        .apply_response(&request, response)
        .expect("response is current");
    let editor = ComposerEditor::from_text("prompt ");

    // When: exactly one documented partial-acceptance unit is accepted.
    let next = controller
        .accept_next_grapheme(&editor)
        .expect("one grapheme is a valid acceptance unit");

    // Then: the complete first cluster is inserted and the remainder is intact.
    assert_eq!(next.text(), "prompt 👨‍👩‍👧‍👦");
    assert_eq!(
        controller.current().map(|suggestion| suggestion.text()),
        Some("e\u{301}👍🏽")
    );
}

#[test]
fn partial_then_full_acceptance_produces_the_expected_composer_state() {
    // Given: a visible multi-unit suggestion at the end of a draft.
    let (_clock, mut controller) = ready_controller();
    let editor = ComposerEditor::from_text("inspect ");

    // When: one unit is accepted and then the remaining suggestion is accepted.
    let partial = controller
        .accept_next_grapheme(&editor)
        .expect("partial acceptance is valid");
    let full = controller
        .accept_full(&partial)
        .expect("full acceptance is valid");

    // Then: both operations preserve their documented composer insertion point.
    assert_eq!(partial.text(), "inspect g");
    assert_eq!(full.text(), "inspect ghost text");
    assert!(controller.current().is_none());
}

#[test]
fn edit_focus_and_state_transitions_clear_visible_suggestions() {
    // Given: a controller with a visible suggestion.
    let (clock, mut controller) = ready_controller();

    // When: a composer edit invalidates it, a fresh request may be pending but no old text remains.
    let edit_request = controller
        .on_edit(&clock, "edited context")
        .expect("edit request is valid");
    assert!(controller.current().is_none());
    assert_eq!(controller.pending(), Some(&edit_request));

    clock.advance_flush(100);
    assert_eq!(
        controller.take_ready_request(&clock),
        Some(edit_request.clone())
    );
    controller
        .apply_response(&edit_request, "focus-sensitive")
        .expect("fresh response is current");
    controller
        .invalidate(Invalidation::FocusChange)
        .expect("focus invalidation is valid");
    assert!(controller.current().is_none());
    assert!(controller.pending().is_none());

    controller
        .on_edit(&clock, "state context")
        .expect("state fixture request is valid");
    clock.advance_flush(100);
    let state_request = controller
        .take_ready_request(&clock)
        .expect("state fixture request is due");
    controller
        .apply_response(&state_request, "state-sensitive")
        .expect("state fixture response is current");
    controller
        .invalidate(Invalidation::StateChange)
        .expect("state invalidation is valid");
    assert!(controller.current().is_none());
    assert!(controller.pending().is_none());
}

#[test]
fn debounce_uses_the_deterministic_flush_clock() {
    // Given: a request with a fixed debounce delay and a zeroed fake clock.
    let clock = DualClock::new();
    let mut controller = SuggestionController::new(75);
    let request = controller
        .on_edit(&clock, "debounced")
        .expect("request is valid");

    // When: the fake clock advances once before and once at the deadline.
    clock.advance_flush(74);
    let early = controller.take_ready_request(&clock);
    clock.advance_flush(1);
    let due = controller.take_ready_request(&clock);

    // Then: readiness is deterministic and the same keyed request is returned at the boundary.
    assert!(early.is_none());
    assert_eq!(due, Some(request));
}

#[test]
fn suggestion_text_cannot_be_written_to_a_persistence_sink() {
    // Given: a visible suggestion and a sink probe representing events/disk/log persistence.
    let (_clock, controller) = ready_controller();
    let suggestion = controller.current().expect("suggestion is visible");
    let mut sink = ProbeSink::default();

    // When: persistence is attempted through the secret-safe boundary.
    let error = suggestion
        .try_persist_to(&mut sink)
        .expect_err("suggestions are memory-only");

    // Then: the typed rejection leaves the sink untouched and exposes no persistence path.
    assert_eq!(error, SuggestionError::PersistenceForbidden);
    assert!(sink.writes.is_empty());
    assert!(!format!("{suggestion:?}").contains("ghost text"));
    assert!(!format!("{controller:?}").contains("ghost text"));
}

#[test]
fn ghost_rendering_uses_exact_muted_design_tokens_without_changing_text() {
    // Given: a visible suggestion containing Unicode text.
    let (_clock, controller) = ready_controller();
    let suggestion = controller.current().expect("suggestion is visible");

    // When: the suggestion is converted to its ghost span.
    let span = harness_tui::ghost_suggestions::render_ghost(suggestion);

    // Then: content is exact and the design-contract muted style is applied.
    assert_eq!(span.content.as_ref(), "ghost text");
    assert_eq!(span.style.fg, Some(Color::Rgb(136, 139, 145)));
    assert!(span.style.add_modifier.contains(Modifier::DIM));
}

#[derive(Default)]
struct ProbeSink {
    writes: Vec<String>,
}

impl SecretSuggestionSink for ProbeSink {
    fn write_suggestion(&mut self, text: &str) {
        self.writes.push(text.to_owned());
    }
}
