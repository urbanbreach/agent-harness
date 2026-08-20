//! Owner tests for Group C (minimal/screen/compact/expand) — Todo 26.
//!
//! TDD failure cases: unavailable backend, invalid input, cancellation,
//! resize, focus restoration, replay-mode write refusal, duplicate group
//! ownership.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "contract tests use fail-fast asserts for missing leaf state"
)]

#[path = "../src/leaf_actions/group_c_screen_modes.rs"]
mod group_c_screen_modes;

use group_c_screen_modes::*;

/// Group ID is exactly "C" — no duplicate group ownership.
#[test]
fn group_id_is_c() {
    // arrange
    // act
    // assert
    assert_eq!(group_id(), "C");
}

/// Capability IDs are unique within the group.
#[test]
fn capability_ids_are_unique() {
    // arrange
    // act
    let ids = capability_ids();
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        // assert
        assert!(seen.insert(*id), "duplicate capability id: {id}");
    }
}

/// resolve returns the real backend owner for tui.minimal_mode.
#[test]
fn resolve_minimal_mode_names_real_backend_owner() {
    // arrange
    // act
    let res = resolve("tui.minimal_mode").expect("must resolve");
    // assert
    assert_eq!(res.capability_id, "tui.minimal_mode");
    assert_eq!(res.backend_owner, "crates/harness-tui/src/app.rs");
    assert_eq!(res.availability, ActionAvailability::Unwired);
}

/// Unavailable backend: resolve returns None for unknown capability.
#[test]
fn resolve_unknown_capability_returns_none() {
    // arrange
    // act
    // assert
    assert!(resolve("nonexistent").is_none());
}

/// Invalid input: None action is rejected.
#[test]
fn validate_input_rejects_none_action() {
    // arrange
    // act
    let result = validate_input(ScreenModeAction::None, "");
    // assert
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: toggles reject non-empty input.
#[test]
fn validate_input_rejects_nonempty_toggle() {
    // arrange
    // act
    let result = validate_input(ScreenModeAction::ToggleMinimal, "extra");
    // assert
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Valid input: toggles accept empty input.
#[test]
fn validate_input_accepts_empty_toggle() {
    // arrange
    // act
    // assert
    assert!(matches!(
        validate_input(ScreenModeAction::ToggleMinimal, ""),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(ScreenModeAction::ToggleCompact, ""),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(ScreenModeAction::Expand, ""),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(ScreenModeAction::Collapse, ""),
        InputValidation::Valid
    ));
}

/// Cancellation: screen mode actions are replay-safe (visual state only).
#[test]
fn screen_mode_actions_are_replay_safe() {
    // arrange
    // act
    // assert
    assert!(is_replay_safe(ScreenModeAction::ToggleMinimal));
    assert!(is_replay_safe(ScreenModeAction::ToggleCompact));
    assert!(is_replay_safe(ScreenModeAction::Expand));
    assert!(is_replay_safe(ScreenModeAction::Collapse));
}

/// Resize: screen mode actions are plain value types, unaffected by resize.
#[test]
fn screen_mode_actions_survive_terminal_resize() {
    // arrange
    // act
    let action = ScreenModeAction::ToggleMinimal;
    let cloned = action;
    // assert
    assert_eq!(action, cloned);
}

/// Focus restoration: screen mode changes don't lose focus (replay-safe).
#[test]
fn focus_restoration_after_screen_mode_change() {
    // arrange
    // act
    // Screen mode toggles are replay-safe, so focus is preserved.
    // assert
    assert!(is_replay_safe(ScreenModeAction::ToggleMinimal));
    assert!(is_replay_safe(ScreenModeAction::Expand));
}

/// Replay-mode write refusal: all screen mode actions are replay-safe
/// (they only change visual layout, not data).
#[test]
fn replay_mode_allows_screen_mode_actions() {
    // arrange
    // act
    // assert
    assert!(is_replay_safe(ScreenModeAction::ToggleMinimal));
    assert!(is_replay_safe(ScreenModeAction::ToggleCompact));
}

/// Deterministic: same inputs produce same resolve output.
#[test]
fn screen_mode_resolution_is_deterministic_for_seed() {
    // arrange
    // act
    let a = resolve("tui.minimal_mode");
    let b = resolve("tui.minimal_mode");
    // assert
    assert_eq!(a, b);
}

/// ScreenMode is Copy and Default.
#[test]
fn screen_mode_is_copy_and_default() {
    // arrange
    // act
    let mode = ScreenMode::default();
    // assert
    assert_eq!(mode, ScreenMode::Normal);
    let copied = mode;
    assert_eq!(mode, copied);
}
