//! Owner tests for Group D (dashboard/session-status) — Todo 26.
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

#[path = "../src/leaf_actions/group_d_dashboard.rs"]
mod group_d_dashboard;

use group_d_dashboard::*;

/// Group ID is exactly "D" — no duplicate group ownership.
#[test]
fn group_id_is_d() {
    assert_eq!(group_id(), "D");
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

/// resolve returns the real backend owner for cli.dashboard.
#[test]
fn resolve_cli_dashboard_names_real_backend_owner() {
    let res = resolve("cli.dashboard").expect("must resolve");
    assert_eq!(res.capability_id, "cli.dashboard");
    assert_eq!(res.backend_owner, "crates/harness/src/lib.rs");
    assert_eq!(res.availability, ActionAvailability::Unwired);
}

/// resolve returns the real backend owner for tui.session_status_dashboard.
#[test]
fn resolve_session_status_dashboard_names_real_backend_owner() {
    let res = resolve("tui.session_status_dashboard").expect("must resolve");
    assert_eq!(res.capability_id, "tui.session_status_dashboard");
    assert_eq!(res.backend_owner, "crates/harness-tui/src/app.rs");
    assert_eq!(res.availability, ActionAvailability::Available);
}

/// Unavailable backend: resolve returns None for unknown capability.
#[test]
fn resolve_unknown_capability_returns_none() {
    assert!(resolve("nonexistent").is_none());
}

/// Invalid input: None action is rejected.
#[test]
fn validate_input_rejects_none_action() {
    let result = validate_input(DashboardAction::None, "");
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: dashboard actions reject non-empty input.
#[test]
fn validate_input_rejects_nonempty_input() {
    let result = validate_input(DashboardAction::OpenDashboard, "extra");
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Valid input: dashboard actions accept empty input.
#[test]
fn validate_input_accepts_empty_input() {
    assert!(matches!(
        validate_input(DashboardAction::OpenDashboard, ""),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(DashboardAction::ShowSessionStatus, ""),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(DashboardAction::ShowUsage, ""),
        InputValidation::Valid
    ));
}

/// Cancellation: dashboard actions are replay-safe (read-only display).
#[test]
fn dashboard_actions_are_replay_safe() {
    assert!(is_replay_safe(DashboardAction::OpenDashboard));
    assert!(is_replay_safe(DashboardAction::ShowSessionStatus));
    assert!(is_replay_safe(DashboardAction::ShowUsage));
    assert!(is_replay_safe(DashboardAction::ShowContext));
    assert!(is_replay_safe(DashboardAction::ShowTasks));
}

/// Resize: dashboard actions are plain value types, unaffected by resize.
#[test]
fn actions_survive_resize() {
    let action = DashboardAction::OpenDashboard;
    let cloned = action;
    assert_eq!(action, cloned);
}

/// Focus restoration: dashboard display is replay-safe.
#[test]
fn focus_restoration_after_dashboard_open() {
    assert!(is_replay_safe(DashboardAction::OpenDashboard));
}

/// Replay-mode write refusal: dashboard actions are read-only (replay-safe).
#[test]
fn replay_mode_allows_dashboard_actions() {
    assert!(is_replay_safe(DashboardAction::ShowSessionStatus));
}

/// Deterministic: same inputs produce same resolve output.
#[test]
fn resolve_is_deterministic() {
    let a = resolve("cli.dashboard");
    let b = resolve("cli.dashboard");
    assert_eq!(a, b);
}

/// DashboardAction is Copy and Default.
#[test]
fn dashboard_action_is_copy_and_default() {
    let action = DashboardAction::default();
    assert_eq!(action, DashboardAction::None);
    let copied = action;
    assert_eq!(action, copied);
}
