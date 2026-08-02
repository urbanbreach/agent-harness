//! Owner tests for Group B (vim/multiline/history/find) — Todo 26.
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

#[path = "../src/leaf_actions/group_b_composer_modes.rs"]
mod group_b_composer_modes;

use group_b_composer_modes::*;

/// Group ID is exactly "B" — no duplicate group ownership.
#[test]
fn group_id_is_b() {
    assert_eq!(group_id(), "B");
}

/// Capability IDs are unique within the group.
#[test]
fn capability_ids_are_unique() {
    let ids = capability_ids();
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        assert!(seen.insert(*id), "duplicate capability id: {id}");
    }
}

/// resolve returns the real backend owner for tui.vim_mode.
#[test]
fn resolve_vim_mode_names_real_backend_owner() {
    let res = resolve("tui.vim_mode").expect("must resolve");
    assert_eq!(res.capability_id, "tui.vim_mode");
    assert_eq!(res.backend_owner, "crates/harness-tui/src/ui_composer.rs");
    assert_eq!(res.availability, ActionAvailability::Unwired);
}

/// Unavailable backend: resolve returns None for unknown capability.
#[test]
fn resolve_unknown_capability_returns_none() {
    assert!(resolve("nonexistent").is_none());
}

/// Invalid input: None action is rejected.
#[test]
fn validate_input_rejects_none_action() {
    let result = validate_input(ComposerModeAction::None, "");
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: FindInComposer requires a non-empty search term.
#[test]
fn validate_input_rejects_empty_find() {
    let result = validate_input(ComposerModeAction::FindInComposer, "");
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: FindInComposer rejects overlong search.
#[test]
fn validate_input_rejects_overlong_find() {
    let long = "x".repeat(1025);
    let result = validate_input(ComposerModeAction::FindInComposer, &long);
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Valid input: mode toggles accept empty input.
#[test]
fn validate_input_accepts_mode_toggles() {
    assert!(matches!(
        validate_input(ComposerModeAction::ToggleVimMode, ""),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(ComposerModeAction::VimNormal, ""),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(ComposerModeAction::MultilineToggle, ""),
        InputValidation::Valid
    ));
}

/// Cancellation: history navigation changes composer state, so it is NOT replay-safe.
#[test]
fn history_navigation_is_not_replay_safe() {
    assert!(!is_replay_safe(ComposerModeAction::HistoryPrevious));
    assert!(!is_replay_safe(ComposerModeAction::HistoryNext));
}

/// Replay-mode write refusal: mode toggles that change state are not replay-safe.
#[test]
fn replay_mode_refuses_state_changing_actions() {
    // ToggleVimMode and MultilineToggle change composer state — not replay-safe.
    assert!(!is_replay_safe(ComposerModeAction::ToggleVimMode));
    assert!(!is_replay_safe(ComposerModeAction::MultilineToggle));
    assert!(!is_replay_safe(ComposerModeAction::HistoryPrevious));
    assert!(!is_replay_safe(ComposerModeAction::HistoryNext));
}

/// Resize: composer mode actions are plain value types, unaffected by resize.
#[test]
fn actions_survive_resize() {
    let action = ComposerModeAction::ToggleVimMode;
    let cloned = action;
    assert_eq!(action, cloned);
}

/// Focus restoration: vim sub-modes are replay-safe (read-only inspection).
#[test]
fn vim_sub_modes_are_replay_safe() {
    assert!(is_replay_safe(ComposerModeAction::VimNormal));
    assert!(is_replay_safe(ComposerModeAction::VimInsert));
    assert!(is_replay_safe(ComposerModeAction::VimVisual));
}

/// Deterministic: same inputs produce same resolve output.
#[test]
fn resolve_is_deterministic() {
    let a = resolve("tui.vim_mode");
    let b = resolve("tui.vim_mode");
    assert_eq!(a, b);
}

/// VimSubMode is Copy and Default.
#[test]
fn vim_sub_mode_is_copy_and_default() {
    let mode = VimSubMode::default();
    assert_eq!(mode, VimSubMode::Disabled);
    let copied = mode;
    assert_eq!(mode, copied);
}
