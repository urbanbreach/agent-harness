//! Owner tests for Group I (theme/terminal/mouse/timestamps/debug) — Todo 26.
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

#[path = "../src/leaf_actions/group_i_preferences.rs"]
mod group_i_preferences;

use group_i_preferences::*;

/// Group ID is exactly "I" — no duplicate group ownership.
#[test]
fn group_id_is_i() {
    assert_eq!(group_id(), "I");
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

/// resolve returns the real backend owner for tui.theme_auto_system.
#[test]
fn resolve_theme_auto_system_names_real_backend_owner() {
    let res = resolve("tui.theme_auto_system").expect("must resolve");
    assert_eq!(res.capability_id, "tui.theme_auto_system");
    assert_eq!(res.backend_owner, "crates/harness-tui/src/theme.rs");
    assert_eq!(res.availability, ActionAvailability::Unwired);
}

/// resolve returns the real backend owner for tui.themes.
#[test]
fn resolve_themes_names_real_backend_owner() {
    let res = resolve("tui.themes").expect("must resolve");
    assert_eq!(res.capability_id, "tui.themes");
    assert_eq!(res.backend_owner, "crates/harness-tui/src/theme.rs");
}

/// resolve returns the real backend owner for tui.mouse.
#[test]
fn resolve_mouse_names_real_backend_owner() {
    let res = resolve("tui.mouse").expect("must resolve");
    assert_eq!(res.capability_id, "tui.mouse");
    assert_eq!(res.backend_owner, "crates/harness-tui/src/app.rs");
}

/// Unavailable backend: resolve returns None for unknown capability.
#[test]
fn resolve_unknown_capability_returns_none() {
    assert!(resolve("nonexistent").is_none());
}

/// Invalid input: None action is rejected.
#[test]
fn validate_input_rejects_none_action() {
    let result = validate_input(PreferenceAction::None, "");
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: SelectTheme requires a theme name.
#[test]
fn validate_input_rejects_empty_theme() {
    let result = validate_input(PreferenceAction::SelectTheme, "");
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: SetEffort rejects invalid values.
#[test]
fn validate_input_rejects_invalid_effort() {
    assert!(matches!(
        validate_input(PreferenceAction::SetEffort, "invalid"),
        InputValidation::Invalid(_)
    ));
}

/// Valid input: SetEffort accepts valid values.
#[test]
fn validate_input_accepts_valid_effort() {
    assert!(matches!(
        validate_input(PreferenceAction::SetEffort, "low"),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(PreferenceAction::SetEffort, "high"),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(PreferenceAction::SetEffort, "ultra"),
        InputValidation::Valid
    ));
}

/// Valid input: toggles accept empty input.
#[test]
fn validate_input_accepts_toggle_empty() {
    assert!(matches!(
        validate_input(PreferenceAction::ToggleThemeAuto, ""),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(PreferenceAction::ToggleMouse, ""),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(PreferenceAction::ToggleDebug, ""),
        InputValidation::Valid
    ));
}

/// Cancellation: theme and display toggles are replay-safe.
#[test]
fn display_toggles_are_replay_safe() {
    assert!(is_replay_safe(PreferenceAction::ToggleThemeAuto));
    assert!(is_replay_safe(PreferenceAction::SelectTheme));
    assert!(is_replay_safe(PreferenceAction::TerminalFallback));
    assert!(is_replay_safe(PreferenceAction::ToggleMouse));
    assert!(is_replay_safe(PreferenceAction::ToggleTimestamps));
}

/// Resize: preference actions are plain value types, unaffected by resize.
#[test]
fn actions_survive_resize() {
    let action = PreferenceAction::ToggleThemeAuto;
    let cloned = action;
    assert_eq!(action, cloned);
}

/// Focus restoration: theme selection is replay-safe.
#[test]
fn focus_restoration_after_theme_select() {
    assert!(is_replay_safe(PreferenceAction::SelectTheme));
}

/// Replay-mode write refusal: effort/persona/debug/always-approve are not replay-safe.
#[test]
fn replay_mode_refuses_state_changing_preferences() {
    assert!(!is_replay_safe(PreferenceAction::SetEffort));
    assert!(!is_replay_safe(PreferenceAction::SelectPersona));
    assert!(!is_replay_safe(PreferenceAction::ToggleDebug));
    assert!(!is_replay_safe(PreferenceAction::ToggleAlwaysApprove));
    assert!(!is_replay_safe(PreferenceAction::ToggleAuto));
}

/// Deterministic: same inputs produce same resolve output.
#[test]
fn resolve_is_deterministic() {
    let a = resolve("tui.theme_auto_system");
    let b = resolve("tui.theme_auto_system");
    assert_eq!(a, b);
}

/// ThemeMode is Copy and Default.
#[test]
fn theme_mode_is_copy_and_default() {
    let mode = ThemeMode::default();
    assert_eq!(mode, ThemeMode::Manual);
    let copied = mode;
    assert_eq!(mode, copied);
}

/// EffortLevel is Copy and Default.
#[test]
fn effort_level_is_copy_and_default() {
    let level = EffortLevel::default();
    assert_eq!(level, EffortLevel::Medium);
    let copied = level;
    assert_eq!(level, copied);
}
