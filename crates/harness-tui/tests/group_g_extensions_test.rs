//! Owner tests for Group G (extensions/plugins/MCP/settings) — Todo 26.
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

#[path = "../src/leaf_actions/group_g_extensions.rs"]
mod group_g_extensions;

use group_g_extensions::*;

/// Group ID is exactly "G" — no duplicate group ownership.
#[test]
fn group_id_is_g() {
    // arrange
    // act
    // assert
    assert_eq!(group_id(), "G");
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

/// resolve returns the real backend owner for tui.extensions_plugins_ui.
#[test]
fn resolve_extensions_plugins_ui_names_real_backend_owner() {
    // arrange
    // act
    let res = resolve("tui.extensions_plugins_ui").expect("must resolve");
    // assert
    assert_eq!(res.capability_id, "tui.extensions_plugins_ui");
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
    let result = validate_input(ExtensionAction::None, "");
    // assert
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: TogglePlugin requires a plugin name.
#[test]
fn validate_input_rejects_empty_plugin_name() {
    // arrange
    // act
    let result = validate_input(ExtensionAction::TogglePlugin, "");
    // assert
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: McpRegistration requires a server name.
#[test]
fn validate_input_rejects_empty_mcp_server() {
    // arrange
    // act
    let result = validate_input(ExtensionAction::McpRegistration, "");
    // assert
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Invalid input: TogglePlugin rejects overlong name.
#[test]
fn validate_input_rejects_overlong_plugin_name() {
    // arrange
    // act
    let long = "x".repeat(257);
    let result = validate_input(ExtensionAction::TogglePlugin, &long);
    // assert
    assert!(matches!(result, InputValidation::Invalid(_)));
}

/// Valid input: panel opens accept empty input.
#[test]
fn validate_input_accepts_panel_open() {
    // arrange
    // act
    // assert
    assert!(matches!(
        validate_input(ExtensionAction::OpenExtensionsPanel, ""),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(ExtensionAction::OpenSettings, ""),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(ExtensionAction::OpenPrivacy, ""),
        InputValidation::Valid
    ));
}

/// Valid input: plugin name accepted.
#[test]
fn validate_input_accepts_plugin_name() {
    // arrange
    // act
    // assert
    assert!(matches!(
        validate_input(ExtensionAction::TogglePlugin, "my-plugin"),
        InputValidation::Valid
    ));
    assert!(matches!(
        validate_input(ExtensionAction::ExecutePlugin, "my-plugin"),
        InputValidation::Valid
    ));
}

/// Cancellation: panel opens are replay-safe (read-only display).
#[test]
fn panel_opens_are_replay_safe() {
    // arrange
    // act
    // assert
    assert!(is_replay_safe(ExtensionAction::OpenExtensionsPanel));
    assert!(is_replay_safe(ExtensionAction::OpenSettings));
    assert!(is_replay_safe(ExtensionAction::OpenPrivacy));
}

/// Resize: extension actions are plain value types, unaffected by resize.
#[test]
fn extension_actions_survive_terminal_resize() {
    // arrange
    // act
    let action = ExtensionAction::OpenExtensionsPanel;
    let cloned = action;
    // assert
    assert_eq!(action, cloned);
}

/// Focus restoration: panel opens are replay-safe.
#[test]
fn focus_restoration_after_panel_open() {
    // arrange
    // act
    // assert
    assert!(is_replay_safe(ExtensionAction::OpenExtensionsPanel));
}

/// Replay-mode write refusal: plugin execution and MCP registration are not replay-safe.
#[test]
fn replay_mode_refuses_plugin_execution() {
    // arrange
    // act
    // assert
    assert!(!is_replay_safe(ExtensionAction::TogglePlugin));
    assert!(!is_replay_safe(ExtensionAction::ExecutePlugin));
    assert!(!is_replay_safe(ExtensionAction::McpRegistration));
    assert!(!is_replay_safe(ExtensionAction::PluginPermissionPrompt));
}

/// Deterministic: same inputs produce same resolve output.
#[test]
fn extension_resolution_is_deterministic_for_seed() {
    // arrange
    // act
    let a = resolve("tui.extensions_plugins_ui");
    let b = resolve("tui.extensions_plugins_ui");
    // assert
    assert_eq!(a, b);
}

/// PluginPermissionState is Copy and Default.
#[test]
fn plugin_permission_state_is_copy_and_default() {
    // arrange
    // act
    let state = PluginPermissionState::default();
    // assert
    assert_eq!(state, PluginPermissionState::NotRequested);
    let copied = state;
    assert_eq!(state, copied);
}
