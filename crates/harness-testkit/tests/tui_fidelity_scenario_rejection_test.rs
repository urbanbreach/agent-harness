#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner rejection tests use fail-fast fixture mutation assertions"
)]

use harness_testkit::tui_fidelity::{Scenario, ScenarioError};

const STARTUP_SMOKE: &str = include_str!("fixtures/tui_fidelity/startup-smoke.json");

fn mutated_scenario(
    mutate: impl FnOnce(&mut serde_json::Value),
) -> Result<Scenario, ScenarioError> {
    let mut value: serde_json::Value = serde_json::from_str(STARTUP_SMOKE).expect("fixture JSON");
    mutate(&mut value);
    Scenario::from_json(&value.to_string())
}

#[test]
fn unknown_action_kind_is_rejected_as_deserialization_error() {
    // Given: an action object with an unsupported closed-enum variant.
    let result = mutated_scenario(|value| {
        let action = value["actions"][0].as_object_mut().expect("action object");
        let payload = action.remove("timed_key").expect("timed key");
        action.insert("unsupported_action".to_owned(), payload);
    });

    // When: the malformed scenario crosses the serde boundary.
    let error = result.expect_err("unknown action must fail");

    // Then: it fails as a typed parse error without executing the payload.
    assert!(matches!(error, ScenarioError::Deserialize(_)));
}

#[test]
fn unknown_fields_are_rejected_at_the_scenario_boundary() {
    // Given: a valid fixture with an unrecognized top-level field.
    let result = mutated_scenario(|value| {
        value["unexpected"] = serde_json::json!(true);
    });

    // When: the scenario is parsed.
    let error = result.expect_err("unknown fields must fail");

    // Then: serde reports a typed deserialization error.
    assert!(matches!(error, ScenarioError::Deserialize(_)));
}

#[test]
fn unknown_checkpoint_name_is_rejected_at_the_scenario_boundary() {
    // Given: a checkpoint name outside the exact closed set.
    let result = mutated_scenario(|value| {
        value["checkpoints"][0]["name"] = serde_json::json!("early");
    });

    // When: the scenario is parsed.
    let error = result.expect_err("unknown checkpoint must fail");

    // Then: the name cannot bypass the typed checkpoint enum.
    assert!(matches!(error, ScenarioError::Deserialize(_)));
}

#[test]
fn duplicate_checkpoint_names_are_rejected() {
    // Given: two captures with the same checkpoint name.
    let result = mutated_scenario(|value| {
        value["checkpoints"][1]["name"] = serde_json::json!("rest");
    });

    // When: the checkpoint sequence is validated.
    let error = result.expect_err("duplicate checkpoint must fail");

    // Then: duplicate identity is a typed checkpoint error.
    assert!(matches!(error, ScenarioError::InvalidCheckpoint(_)));
}

#[test]
fn missing_mid_checkpoint_is_rejected() {
    // Given: the required mid capture is absent.
    let result = mutated_scenario(|value| {
        value["checkpoints"]
            .as_array_mut()
            .expect("checkpoints")
            .retain(|checkpoint| checkpoint["name"] != "mid");
    });

    // When: the exact capture set is validated.
    let error = result.expect_err("missing mid checkpoint must fail");

    // Then: the missing capture is reported as a typed checkpoint error.
    assert!(matches!(error, ScenarioError::InvalidCheckpoint(_)));
}

#[test]
fn missing_settled_frame_is_rejected() {
    // Given: the settled checkpoint has no frame payload.
    let result = mutated_scenario(|value| {
        value["checkpoints"][2]
            .as_object_mut()
            .expect("settled checkpoint")
            .remove("frame");
    });

    // When: the malformed capture crosses the serde boundary.
    let error = result.expect_err("missing settled frame must fail");

    // Then: a frame cannot be omitted from a typed checkpoint.
    assert!(matches!(error, ScenarioError::Deserialize(_)));
}

#[test]
fn broad_identity_rectangle_is_rejected() {
    // Given: an identity rectangle that covers the full checkpoint row.
    let result = mutated_scenario(|value| {
        value["substitutions"][0]["rectangle"]["col"] = serde_json::json!(0);
        value["substitutions"][0]["rectangle"]["cols"] = serde_json::json!(100);
        value["substitutions"][0]["source"]["padding_right"] = serde_json::json!(99);
        value["substitutions"][0]["target"]["padding_right"] = serde_json::json!(94);
    });

    // When: substitution geometry is validated.
    let error = result.expect_err("broad identity rectangle must fail");

    // Then: broad masks are rejected by the typed substitution contract.
    assert!(matches!(error, ScenarioError::InvalidSubstitution(_)));
}

#[test]
fn whole_height_identity_rectangle_is_rejected() {
    // Given: an identity rectangle spanning the full checkpoint height.
    let result = mutated_scenario(|value| {
        value["substitutions"][0]["rectangle"]["col"] = serde_json::json!(0);
        value["substitutions"][0]["rectangle"]["rows"] = serde_json::json!(30);
    });

    // When: substitution geometry is validated.
    let error = result.expect_err("whole-height identity rectangle must fail");

    // Then: whole-region masks are rejected fail-closed.
    assert!(matches!(error, ScenarioError::InvalidSubstitution(_)));
}

#[test]
fn non_identity_replacement_is_rejected() {
    // Given: a product-title substitution with functional replacement text.
    let result = mutated_scenario(|value| {
        value["substitutions"][1]["target"]["text"] = serde_json::json!("send");
    });

    // When: identity scope is validated.
    let error = result.expect_err("functional replacement must fail");

    // Then: only the canonical identity placeholder is accepted.
    assert!(matches!(error, ScenarioError::InvalidSubstitution(_)));
}

#[test]
fn zero_action_tick_is_rejected() {
    // Given: a timed action with an invalid zero tick.
    let result = mutated_scenario(|value| {
        value["actions"][0]["timed_key"]["at_tick"] = serde_json::json!(0);
    });

    // When: action timing is validated.
    let error = result.expect_err("zero tick must fail");

    // Then: timing failure is typed.
    assert!(matches!(error, ScenarioError::InvalidTiming(_)));
}

#[test]
fn out_of_order_action_ticks_are_rejected() {
    // Given: the second action is moved before the first action's tick.
    let result = mutated_scenario(|value| {
        value["actions"][1]["paste"]["at_tick"] = serde_json::json!(1);
    });

    // When: action ordering is validated.
    let error = result.expect_err("out-of-order ticks must fail");

    // Then: the timeline cannot contain ambiguous ordering.
    assert!(matches!(error, ScenarioError::InvalidTiming(_)));
}

#[test]
fn zero_viewport_dimension_is_rejected() {
    // Given: a scenario viewport with no columns.
    let result = mutated_scenario(|value| {
        value["viewport"]["cols"] = serde_json::json!(0);
    });

    // When: scenario geometry is validated.
    let error = result.expect_err("zero columns must fail");

    // Then: geometry failure is typed.
    assert!(matches!(error, ScenarioError::InvalidGeometry(_)));
}

#[test]
fn out_of_bounds_mouse_point_is_rejected() {
    // Given: a mouse point outside the pre-resize viewport.
    let result = mutated_scenario(|value| {
        value["actions"][2]["mouse"]["point"]["col"] = serde_json::json!(80);
    });

    // When: action geometry is validated.
    let error = result.expect_err("out-of-bounds mouse point must fail");

    // Then: the point cannot escape the declared viewport.
    assert!(matches!(error, ScenarioError::InvalidGeometry(_)));
}

#[test]
fn negative_expected_exit_code_is_rejected() {
    // Given: an impossible negative process exit code.
    let result = mutated_scenario(|value| {
        value["expected_exit"]["code"] = serde_json::json!(-1);
    });

    // When: expected exit metadata is validated.
    let error = result.expect_err("negative exit code must fail");

    // Then: exit failure is typed.
    assert!(matches!(error, ScenarioError::InvalidExitCode(_)));
}

#[test]
fn cleanup_must_restore_workspace_and_preserve_evidence() {
    // Given: cleanup expectations that discard the workspace.
    let result = mutated_scenario(|value| {
        value["cleanup"]["restore_workspace"] = serde_json::json!(false);
    });

    // When: cleanup metadata is validated.
    let error = result.expect_err("non-restoring cleanup must fail");

    // Then: cleanup failure is typed and fail-closed.
    assert!(matches!(error, ScenarioError::InvalidCleanup(_)));
}

#[test]
fn cleanup_paths_must_be_relative_and_traversal_free() {
    // Given: a cleanup path that escapes the scenario workspace.
    let result = mutated_scenario(|value| {
        value["cleanup"]["temporary_paths"][0] = serde_json::json!("../outside");
    });

    // When: cleanup paths are validated.
    let error = result.expect_err("escaping cleanup path must fail");

    // Then: path failure is typed.
    assert!(matches!(error, ScenarioError::InvalidCleanup(_)));
}

#[test]
fn empty_adapter_selection_is_rejected() {
    // Given: a scenario with no selected adapter.
    let result = mutated_scenario(|value| {
        value["adapters"] = serde_json::json!([]);
    });

    // When: adapter selection is validated.
    let error = result.expect_err("empty adapter selection must fail");

    // Then: malformed selection is typed.
    assert!(matches!(error, ScenarioError::NoAdapters));
}

#[test]
fn unknown_adapter_kind_is_rejected_at_the_scenario_boundary() {
    // Given: an adapter value outside the closed enum.
    let result = mutated_scenario(|value| {
        value["adapters"] = serde_json::json!(["unknown"]);
    });

    // When: the scenario is parsed.
    let error = result.expect_err("unknown adapter must fail");

    // Then: serde rejects unsupported adapter input as typed parse failure.
    assert!(matches!(error, ScenarioError::Deserialize(_)));
}
