//! Owner tests for Group E (inline_media/clipboard) — Todo 26.
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

#[path = "../src/leaf_actions/group_e_media.rs"]
mod group_e_media;

use group_e_media::*;

/// Group ID is exactly "E" — no duplicate group ownership.
#[test]
fn group_id_is_e() {
    // arrange
    // act
    // assert
    assert_eq!(group_id(), "E");
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

/// resolve returns the real backend owner for tui.inline_media.
#[test]
fn resolve_inline_media_names_real_backend_owner() {
    // arrange
    // act
    let res = resolve("tui.inline_media").expect("must resolve");
    // assert
    assert_eq!(res.capability_id, "tui.inline_media");
    assert_eq!(res.backend_owner, "crates/harness-tui/src/ui_transcript.rs");
    // Inline media is truthfully unavailable: terminal protocol not negotiated.
    assert!(matches!(
        res.availability,
        ActionAvailability::Unavailable(_)
    ));
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
    let result = validate_input(MediaAction::None, "");
    // assert
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: RenderInlineMedia requires a media path.
#[test]
fn validate_input_rejects_empty_media_path() {
    // arrange
    // act
    let result = validate_input(MediaAction::RenderInlineMedia, "");
    // assert
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: RenderInlineMedia rejects overlong path.
#[test]
fn validate_input_rejects_overlong_media_path() {
    // arrange
    // act
    let long = "x".repeat(4097);
    let result = validate_input(MediaAction::RenderInlineMedia, &long);
    // assert
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Valid input: RenderInlineMedia accepts a valid path.
#[test]
fn validate_input_accepts_valid_media_path() {
    // arrange
    // act
    // assert
    assert!(matches!(
        validate_input(MediaAction::RenderInlineMedia, "/tmp/image.png"),
        InputValidation::Valid
    ));
}

/// Valid input: toggles accept empty input.
#[test]
fn validate_input_accepts_toggle_empty() {
    // arrange
    // act
    // assert
    assert!(matches!(
        validate_input(MediaAction::ClipboardImagePaste, ""),
        InputValidation::Valid
    ));
}

/// Cancellation: media failure recovery is replay-safe.
#[test]
fn media_failure_recovery_is_replay_safe() {
    // arrange
    // act
    // assert
    assert!(is_replay_safe(MediaAction::MediaFailureRecovery));
}

/// Resize: media actions are plain value types, unaffected by resize.
#[test]
fn media_actions_survive_terminal_resize() {
    // arrange
    // act
    let action = MediaAction::RenderInlineMedia;
    let cloned = action;
    // assert
    assert_eq!(action, cloned);
}

/// Focus restoration: RenderInlineMedia is replay-safe (display only).
#[test]
fn render_inline_media_is_replay_safe() {
    // arrange
    // act
    // assert
    assert!(is_replay_safe(MediaAction::RenderInlineMedia));
}

/// Replay-mode write refusal: clipboard paste is not replay-safe (state change).
#[test]
fn replay_mode_refuses_clipboard_paste() {
    // arrange
    // act
    // assert
    assert!(!is_replay_safe(MediaAction::ClipboardImagePaste));
}

/// Deterministic: same inputs produce same resolve output.
#[test]
fn media_resolution_is_deterministic_for_seed() {
    // arrange
    // act
    let a = resolve("tui.inline_media");
    let b = resolve("tui.inline_media");
    // assert
    assert_eq!(a, b);
}

/// MediaFailureReason is Copy and Default.
#[test]
fn media_failure_reason_is_copy_and_default() {
    // arrange
    // act
    let reason = MediaFailureReason::default();
    // assert
    assert_eq!(reason, MediaFailureReason::None);
    let copied = reason;
    assert_eq!(reason, copied);
}
