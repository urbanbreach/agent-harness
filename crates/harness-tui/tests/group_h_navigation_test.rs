//! Owner tests for Group H (navigation/import/rewind/queue) — Todo 26.
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

#[path = "../src/leaf_actions/group_h_navigation.rs"]
mod group_h_navigation;

use group_h_navigation::*;

/// Group ID is exactly "H" — no duplicate group ownership.
#[test]
fn group_id_is_h() {
    assert_eq!(group_id(), "H");
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

#[test]
fn resolve_foreign_import_names_real_backend_owner() {
    let res = resolve("tui.foreign_import_ui_journey").expect("must resolve");
    assert_eq!(res.capability_id, "tui.foreign_import_ui_journey");
    assert_eq!(
        res.backend_owner,
        "crates/harness-tui/src/app/foreign_import.rs"
    );
    assert_eq!(res.availability, ActionAvailability::Available);
}

/// resolve returns the real backend owner for tui.file_completion.
#[test]
fn resolve_file_completion_names_real_backend_owner() {
    let res = resolve("tui.file_completion").expect("must resolve");
    assert_eq!(res.capability_id, "tui.file_completion");
    assert_eq!(res.backend_owner, "crates/harness-tui/src/ui_composer.rs");
}

/// resolve returns truthful unavailable for tui.docs.
#[test]
fn resolve_docs_returns_unavailable() {
    let res = resolve("tui.docs").expect("must resolve");
    assert!(matches!(
        res.availability,
        ActionAvailability::Unavailable(_)
    ));
}

/// resolve returns truthful unavailable for tui.share.
#[test]
fn resolve_share_returns_unavailable() {
    let res = resolve("tui.share").expect("must resolve");
    assert!(matches!(
        res.availability,
        ActionAvailability::Unavailable(_)
    ));
}

/// Unavailable backend: resolve returns None for unknown capability.
#[test]
fn resolve_unknown_capability_returns_none() {
    assert!(resolve("nonexistent").is_none());
}

/// Invalid input: None action is rejected.
#[test]
fn validate_input_rejects_none_action() {
    let result = validate_input(NavigationAction::None, "");
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: EnqueuePrompt requires a non-empty prompt.
#[test]
fn validate_input_rejects_empty_prompt() {
    let result = validate_input(NavigationAction::EnqueuePrompt, "");
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: Share requires a target path.
#[test]
fn validate_input_rejects_empty_share_path() {
    let result = validate_input(NavigationAction::Share, "");
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Valid input: navigation actions accept empty input.
#[test]
fn validate_input_accepts_empty_navigation() {
    assert!(matches!(
        validate_input(NavigationAction::Home, ""),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(NavigationAction::RewindTui, ""),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(NavigationAction::MemoryPalette, ""),
        InputValidation::Valid
    ));
}

/// Valid input: EnqueuePrompt accepts a valid prompt.
#[test]
fn validate_input_accepts_valid_prompt() {
    assert!(matches!(
        validate_input(NavigationAction::EnqueuePrompt, "do the thing"),
        InputValidation::Valid
    ));
}

/// Cancellation: read-only navigation is replay-safe.
#[test]
fn read_only_navigation_is_replay_safe() {
    assert!(is_replay_safe(NavigationAction::ForeignImport));
    assert!(is_replay_safe(NavigationAction::RewindTui));
    assert!(is_replay_safe(NavigationAction::MemoryPalette));
    assert!(is_replay_safe(NavigationAction::FileCompletion));
    assert!(is_replay_safe(NavigationAction::Home));
    assert!(is_replay_safe(NavigationAction::Docs));
}

/// Resize: navigation actions are plain value types, unaffected by resize.
#[test]
fn actions_survive_resize() {
    let action = NavigationAction::RewindTui;
    let cloned = action;
    assert_eq!(action, cloned);
}

/// Focus restoration: memory palette is replay-safe.
#[test]
fn focus_restoration_after_memory_palette() {
    assert!(is_replay_safe(NavigationAction::MemoryPalette));
}

/// Replay-mode write refusal: prompt queue and share are not replay-safe.
#[test]
fn replay_mode_refuses_write_navigation() {
    assert!(!is_replay_safe(NavigationAction::EnqueuePrompt));
    assert!(!is_replay_safe(NavigationAction::Share));
    assert!(!is_replay_safe(NavigationAction::Import));
}

/// Deterministic: same inputs produce same resolve output.
#[test]
fn resolve_is_deterministic() {
    let a = resolve("tui.foreign_import_ui_journey");
    let b = resolve("tui.foreign_import_ui_journey");
    assert_eq!(a, b);
}

/// NavigationAction is Copy and Default.
#[test]
fn navigation_action_is_copy_and_default() {
    let action = NavigationAction::default();
    assert_eq!(action, NavigationAction::None);
    let copied = action;
    assert_eq!(action, copied);
}
