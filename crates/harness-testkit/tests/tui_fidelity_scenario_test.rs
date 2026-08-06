#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast fixture assertions"
)]

use harness_testkit::parity::catalog::CORE_SCENARIOS;
use harness_testkit::parity::{IdentityMaskRegistry, SEMANTIC_FRAME_SCHEMA_VERSION};
use harness_testkit::tui_fidelity::{
    AdapterKind, CaptureMode, CheckpointName, IdentityScope, Scenario, ScenarioAction,
    ScenarioError,
};

const STARTUP_SMOKE: &str = include_str!("fixtures/tui_fidelity/startup-smoke.json");
const CANARY_TERMINAL_QUERY: &str =
    include_str!("../src/tui_fidelity_scenarios/baseline/canary-terminal-query.json");

#[test]
fn baseline_current_parity_support_exposes_semantic_frames_and_identity_cells() {
    // Given: the current Harness parity surface before typed scenarios exist.
    // When: the existing catalog and identity mask API are inspected.
    // Then: the baseline remains semantic-frame based with explicit cell masks.
    assert_eq!(SEMANTIC_FRAME_SCHEMA_VERSION, "semantic-frame-v1");
    assert_eq!(CORE_SCENARIOS.len(), 8);

    let masks = IdentityMaskRegistry::new().with_field("product_title", [(1, 2)]);
    assert_eq!(masks.grapheme_mask_field(1, 2), Some("product_title"));
}

#[test]
fn startup_smoke_fixture_validates_for_both_adapter_kinds() {
    // Given: one independently authored startup-smoke scenario.
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("fixture parses");

    // When: the same scenario is checked against each supported adapter.
    let grok = scenario.validate_for_adapter(AdapterKind::Grok);
    let harness = scenario.validate_for_adapter(AdapterKind::Harness);

    // Then: both adapter paths accept the typed contract.
    assert!(grok.is_ok(), "grok validation failed: {grok:?}");
    assert!(harness.is_ok(), "harness validation failed: {harness:?}");
}

#[test]
fn canary_terminal_query_uses_action_tail_capture_without_changing_defaults() {
    // Given: the focused terminal-query canary and the existing startup fixture.
    let canary = Scenario::from_json(CANARY_TERMINAL_QUERY).expect("canary parses");
    let default = Scenario::from_json(STARTUP_SMOKE).expect("startup parses");

    // When: their capture contracts are inspected.
    // Then: only the canary opts into the narrow action-tail surface.
    assert_eq!(canary.capture_mode, CaptureMode::ActionTail);
    assert_eq!(default.capture_mode, CaptureMode::FullSession);
}

#[test]
fn startup_smoke_fixture_round_trips_byte_stably() {
    // Given: a valid typed scenario.
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("fixture parses");

    // When: it is serialized, parsed, and serialized again canonically.
    let first = scenario.to_json().expect("scenario serializes");
    let second = Scenario::from_json(&first)
        .expect("canonical scenario parses")
        .to_json()
        .expect("canonical scenario serializes");

    // Then: serde round-trip output is stable.
    assert_eq!(first, second);
}

#[test]
fn startup_smoke_fixture_covers_every_action_variant() {
    // Given: the independently authored action sequence.
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("fixture parses");

    // When: action variants are classified by the closed enum.
    let kinds: Vec<&str> = scenario
        .actions
        .iter()
        .map(ScenarioAction::kind_name)
        .collect();

    // Then: each supported action appears exactly once.
    assert_eq!(
        kinds,
        vec![
            "timed_key",
            "paste",
            "mouse",
            "drag",
            "wheel",
            "resize",
            "wait_for_semantic_state",
            "terminal_reply"
        ]
    );
}

#[test]
fn startup_smoke_fixture_covers_ordered_checkpoints_and_identity_scopes() {
    // Given: a valid scenario with captures and substitutions.
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("fixture parses");

    // When: checkpoint names and product-identity scopes are collected.
    let checkpoints: Vec<CheckpointName> = scenario
        .checkpoints
        .iter()
        .map(|checkpoint| checkpoint.name)
        .collect();
    let scopes: Vec<IdentityScope> = scenario
        .substitutions
        .iter()
        .map(|substitution| substitution.scope)
        .collect();

    // Then: the exact capture order and all identity scopes are present.
    assert_eq!(
        checkpoints,
        vec![
            CheckpointName::Rest,
            CheckpointName::Mid,
            CheckpointName::Settled
        ]
    );
    assert_eq!(
        scopes,
        vec![
            IdentityScope::WorkspacePath,
            IdentityScope::ProviderName,
            IdentityScope::WorkspacePath,
            IdentityScope::ProviderName,
            IdentityScope::WorkspacePath,
            IdentityScope::ProviderName
        ]
    );
}

#[test]
fn startup_smoke_fixture_preserves_terminal_data_as_data() {
    // Given: a valid scenario containing paste and terminal-reply payloads.
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("fixture parses");

    // When: the actions are inspected without executing them.
    let paste = scenario.actions.iter().find_map(|action| match action {
        ScenarioAction::Paste(action) => Some(action.text.as_str()),
        _ => None,
    });
    let reply = scenario.actions.iter().find_map(|action| match action {
        ScenarioAction::TerminalReply(action) => Some(action.response.as_str()),
        _ => None,
    });

    // Then: payloads remain literal typed data.
    assert_eq!(paste, Some("arness"));
    assert_eq!(reply, Some("\u{1b}[?1;2c"));
}

#[test]
fn unsupported_adapter_selection_is_typed_error() {
    // Given: a valid scenario that explicitly selects only Harness.
    let mut value: serde_json::Value = serde_json::from_str(STARTUP_SMOKE).expect("fixture JSON");
    value["adapters"] = serde_json::json!(["harness"]);
    let scenario = Scenario::from_json(&value.to_string()).expect("scenario parses");

    // When: the unselected adapter path is requested.
    let error = scenario
        .validate_for_adapter(AdapterKind::Grok)
        .expect_err("grok must be rejected");

    // Then: selection failure is represented by a typed scenario error.
    assert!(matches!(
        error,
        ScenarioError::AdapterNotSelected(AdapterKind::Grok)
    ));
}
