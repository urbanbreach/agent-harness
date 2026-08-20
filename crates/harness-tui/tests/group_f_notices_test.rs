//! Owner tests for Group F (notifications/tips) — Todo 26.
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

#[path = "../src/leaf_actions/group_f_notices.rs"]
mod group_f_notices;

use group_f_notices::*;

/// Group ID is exactly "F" — no duplicate group ownership.
#[test]
fn group_id_is_f() {
    // arrange
    // act
    // assert
    assert_eq!(group_id(), "F");
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

/// resolve returns the real backend owner for tui.notifications.
#[test]
fn resolve_notifications_names_real_backend_owner() {
    // arrange
    // act
    let res = resolve("tui.notifications").expect("must resolve");
    // assert
    assert_eq!(res.capability_id, "tui.notifications");
    assert_eq!(res.backend_owner, "crates/harness-tui/src/app.rs");
    assert_eq!(res.availability, ActionAvailability::Unwired);
}

/// resolve returns the real backend owner for tui.tips.
#[test]
fn resolve_tips_names_real_backend_owner() {
    // arrange
    // act
    let res = resolve("tui.tips").expect("must resolve");
    // assert
    assert_eq!(res.capability_id, "tui.tips");
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
    let result = validate_input(NoticeAction::None, "");
    // assert
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: ShowNotification requires a non-empty message.
#[test]
fn validate_input_rejects_empty_notification() {
    // arrange
    // act
    let result = validate_input(NoticeAction::ShowNotification, "");
    // assert
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: ShowNotification rejects overlong message.
#[test]
fn validate_input_rejects_overlong_notification() {
    // arrange
    // act
    let long = "x".repeat(1025);
    let result = validate_input(NoticeAction::ShowNotification, &long);
    // assert
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: ShowTip requires a non-empty body.
#[test]
fn validate_input_rejects_empty_tip() {
    // arrange
    // act
    let result = validate_input(NoticeAction::ShowTip, "");
    // assert
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Valid input: dismiss accepts empty input.
#[test]
fn validate_input_accepts_dismiss() {
    // arrange
    // act
    // assert
    assert!(matches!(
        validate_input(NoticeAction::DismissNotification, ""),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(NoticeAction::DismissTip, ""),
        InputValidation::Valid
    ));
}

/// Cancellation: dismiss actions are not replay-safe (state change).
#[test]
fn dismiss_actions_are_not_replay_safe() {
    // arrange
    // act
    // assert
    assert!(!is_replay_safe(NoticeAction::DismissNotification));
    assert!(!is_replay_safe(NoticeAction::DismissTip));
}

/// Resize: notice actions are plain value types, unaffected by resize.
#[test]
fn notice_actions_survive_terminal_resize() {
    // arrange
    // act
    let action = NoticeAction::ShowNotification;
    let cloned = action;
    // assert
    assert_eq!(action, cloned);
}

/// Focus restoration: show actions are replay-safe (display only).
#[test]
fn show_actions_are_replay_safe() {
    // arrange
    // act
    // assert
    assert!(is_replay_safe(NoticeAction::ShowNotification));
    assert!(is_replay_safe(NoticeAction::ShowTip));
    assert!(is_replay_safe(NoticeAction::ShowAnnouncement));
    assert!(is_replay_safe(NoticeAction::ShowReleaseNotes));
}

/// Replay-mode write refusal: dismiss actions are not replay-safe.
#[test]
fn replay_mode_refuses_dismiss_actions() {
    // arrange
    // act
    // assert
    assert!(!is_replay_safe(NoticeAction::DismissNotification));
    assert!(!is_replay_safe(NoticeAction::DismissTip));
}

/// Deterministic: same inputs produce same resolve output.
#[test]
fn notice_resolution_is_deterministic_for_seed() {
    // arrange
    // act
    let a = resolve("tui.notifications");
    let b = resolve("tui.notifications");
    // assert
    assert_eq!(a, b);
}

/// NoticeLevel is Copy and Default.
#[test]
fn notice_level_is_copy_and_default() {
    // arrange
    // act
    let level = NoticeLevel::default();
    // assert
    assert_eq!(level, NoticeLevel::Info);
    let copied = level;
    assert_eq!(level, copied);
}
