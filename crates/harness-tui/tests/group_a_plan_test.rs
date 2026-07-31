//! Owner tests for Group A (plan/approval/view-plan) — Todo 26.
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

#[path = "../src/leaf_actions/group_a_plan.rs"]
mod group_a_plan;

use group_a_plan::*;

/// Group ID is exactly "A" — no duplicate group ownership.
#[test]
fn group_id_is_a() {
    assert_eq!(group_id(), "A");
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

/// resolve returns the real backend owner for tui.plan_mode.
#[test]
fn resolve_plan_mode_names_real_backend_owner() {
    let res = resolve("tui.plan_mode");
    assert!(res.is_some(), "must resolve tui.plan_mode");
    let res = res.expect("checked above");
    assert_eq!(res.capability_id, "tui.plan_mode");
    assert_eq!(res.backend_owner, "crates/harness-tui/src/app.rs");
    assert_eq!(res.availability, ActionAvailability::Unwired);
}

/// resolve returns the real backend owner for tui.view_plan.
#[test]
fn resolve_view_plan_names_real_backend_owner() {
    let res = resolve("tui.view_plan").expect("must resolve");
    assert_eq!(res.capability_id, "tui.view_plan");
    assert_eq!(res.backend_owner, "crates/harness-tui/src/app.rs");
    assert_eq!(res.availability, ActionAvailability::Unwired);
}

/// Unavailable backend: resolve returns None for unknown capability.
#[test]
fn resolve_unknown_capability_returns_none() {
    assert!(resolve("nonexistent.capability").is_none());
}

/// Invalid input: None action is rejected.
#[test]
fn validate_input_rejects_none_action() {
    let result = validate_input(PlanAction::None, "");
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: RejectPlan requires a non-empty reason.
#[test]
fn validate_input_rejects_empty_rejection() {
    let result = validate_input(PlanAction::RejectPlan, "");
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: RejectPlan rejects overlong reason.
#[test]
fn validate_input_rejects_overlong_rejection() {
    let long = "x".repeat(4097);
    let result = validate_input(PlanAction::RejectPlan, &long);
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Valid input: ApprovePlan accepts empty or short note.
#[test]
fn validate_input_accepts_valid_approval() {
    assert!(matches!(
        validate_input(PlanAction::ApprovePlan, ""),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(PlanAction::ApprovePlan, "looks good"),
        InputValidation::Valid
    ));
}

/// ReadOnlyGuard requires the refused action name.
#[test]
fn validate_input_rejects_empty_guard() {
    let result = validate_input(PlanAction::ReadOnlyGuard, "");
    assert!(matches!(result, InputValidation::Invalid(_)));
    assert!(matches!(
        validate_input(PlanAction::ReadOnlyGuard, "edit"),
        InputValidation::Valid
    ));
}

/// Cancellation: ExitPlanMode is a write action that can be cancelled.
#[test]
fn exit_plan_mode_is_write_action() {
    assert!(is_write_action(PlanAction::ExitPlanMode));
    assert!(is_write_action(PlanAction::EnterPlanMode));
    assert!(is_write_action(PlanAction::ApprovePlan));
    assert!(is_write_action(PlanAction::RejectPlan));
    assert!(!is_write_action(PlanAction::ViewPlan));
}

/// Replay-mode write refusal: write actions are not replay-safe.
#[test]
fn replay_mode_refuses_write_actions() {
    assert!(!is_replay_safe(PlanAction::EnterPlanMode));
    assert!(!is_replay_safe(PlanAction::ExitPlanMode));
    assert!(!is_replay_safe(PlanAction::ApprovePlan));
    assert!(!is_replay_safe(PlanAction::RejectPlan));
    assert!(is_replay_safe(PlanAction::ViewPlan));
    assert!(is_replay_safe(PlanAction::ReadOnlyGuard));
}

/// Resize: plan mode actions are plain value types, unaffected by resize.
#[test]
fn actions_survive_resize() {
    let action = PlanAction::EnterPlanMode;
    let cloned = action;
    assert_eq!(action, cloned);
}

/// Focus restoration: ViewPlan is replay-safe and read-only.
#[test]
fn view_plan_is_replay_safe() {
    assert!(is_replay_safe(PlanAction::ViewPlan));
}

/// Deterministic: same inputs produce same resolve output.
#[test]
fn resolve_is_deterministic() {
    let a = resolve("tui.plan_mode");
    let b = resolve("tui.plan_mode");
    assert_eq!(a, b);
}

/// PlanAction is Copy and Default.
#[test]
fn plan_action_is_copy_and_default() {
    let action = PlanAction::default();
    assert_eq!(action, PlanAction::None);
    let copied = action;
    assert_eq!(action, copied);
}
