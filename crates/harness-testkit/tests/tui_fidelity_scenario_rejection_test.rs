#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner rejection tests use fail-fast fixture mutation assertions"
)]

use harness_testkit::tui_fidelity::{Scenario, ScenarioError, SubstitutionError};

const STARTUP_SMOKE: &str = include_str!("fixtures/tui_fidelity/startup-smoke.json");
const TYPED_STARTUP: &str = include_str!("../src/tui_fidelity_scenarios/baseline/startup.json");

fn mutated_scenario(
    mutate: impl FnOnce(&mut serde_json::Value),
) -> Result<Scenario, ScenarioError> {
    let mut value: serde_json::Value = serde_json::from_str(STARTUP_SMOKE).expect("fixture JSON");
    mutate(&mut value);
    Scenario::from_json(&value.to_string())
}

fn mutated_substitution_scenario(
    mutate: impl FnOnce(&mut serde_json::Value),
) -> Result<Scenario, ScenarioError> {
    let mut value: serde_json::Value = serde_json::from_str(TYPED_STARTUP).expect("fixture JSON");
    mutate(&mut value);
    Scenario::from_json(&value.to_string())
}

#[test]
fn unknown_action_kind_is_rejected_as_deserialization_error() {
    // arrange: an action object with an unsupported closed-enum variant.
    let result = mutated_scenario(|value| {
        let action = value["actions"][0].as_object_mut().expect("action object");
        let payload = action.remove("timed_key").expect("timed key");
        action.insert("unsupported_action".to_owned(), payload);
    });

    // act: the malformed scenario crosses the serde boundary.
    let error = result.expect_err("unknown action must fail");

    // assert: it fails as a typed parse error without executing the payload.
    assert!(matches!(error, ScenarioError::Deserialize(_)));
}

#[test]
fn unknown_fields_are_rejected_at_the_scenario_boundary() {
    // arrange: a valid fixture with an unrecognized top-level field.
    let result = mutated_scenario(|value| {
        value["unexpected"] = serde_json::json!(true);
    });

    // act: the scenario is parsed.
    let error = result.expect_err("unknown fields must fail");

    // assert: serde reports a typed deserialization error.
    assert!(matches!(error, ScenarioError::Deserialize(_)));
}

#[test]
fn unknown_checkpoint_name_is_rejected_at_the_scenario_boundary() {
    // arrange: a checkpoint name outside the exact closed set.
    let result = mutated_scenario(|value| {
        value["checkpoints"][0]["name"] = serde_json::json!("early");
    });

    // act: the scenario is parsed.
    let error = result.expect_err("unknown checkpoint must fail");

    // assert: the name cannot bypass the typed checkpoint enum.
    assert!(matches!(error, ScenarioError::Deserialize(_)));
}

#[test]
fn duplicate_checkpoint_names_are_rejected() {
    // arrange: two captures with the same checkpoint name.
    let result = mutated_scenario(|value| {
        value["checkpoints"][1]["name"] = serde_json::json!("rest");
    });

    // act: the checkpoint sequence is validated.
    let error = result.expect_err("duplicate checkpoint must fail");

    // assert: duplicate identity is a typed checkpoint error.
    assert!(matches!(error, ScenarioError::InvalidCheckpoint(_)));
}

#[test]
fn missing_mid_checkpoint_is_rejected() {
    // arrange: the required mid capture is absent.
    let result = mutated_scenario(|value| {
        value["checkpoints"]
            .as_array_mut()
            .expect("checkpoints")
            .retain(|checkpoint| checkpoint["name"] != "mid");
    });

    // act: the exact capture set is validated.
    let error = result.expect_err("missing mid checkpoint must fail");

    // assert: the missing capture is reported as a typed checkpoint error.
    assert!(matches!(error, ScenarioError::InvalidCheckpoint(_)));
}

#[test]
fn missing_settled_frame_is_rejected() {
    // arrange: the settled checkpoint has no frame payload.
    let result = mutated_scenario(|value| {
        value["checkpoints"][2]
            .as_object_mut()
            .expect("settled checkpoint")
            .remove("frame");
    });

    // act: the malformed capture crosses the serde boundary.
    let error = result.expect_err("missing settled frame must fail");

    // assert: a frame cannot be omitted from a typed checkpoint.
    assert!(matches!(error, ScenarioError::Deserialize(_)));
}

#[test]
fn broad_identity_rectangle_is_rejected() {
    // arrange: an identity rectangle that covers the full checkpoint row.
    let result = mutated_substitution_scenario(|value| {
        value["substitutions"][0]["rectangle"]["col"] = serde_json::json!(0);
        value["substitutions"][0]["rectangle"]["cols"] = serde_json::json!(100);
        value["substitutions"][0]["reference"]["padding_right"] = serde_json::json!(99);
        value["substitutions"][0]["candidate"]["padding_right"] = serde_json::json!(81);
    });

    // act: substitution geometry is validated.
    let error = result.expect_err("broad identity rectangle must fail");

    // assert: broad masks are rejected by the typed substitution contract.
    assert!(matches!(error, ScenarioError::InvalidSubstitution(_)));
}

#[test]
fn whole_height_identity_rectangle_is_rejected() {
    // arrange: an identity rectangle spanning the full checkpoint height.
    let result = mutated_substitution_scenario(|value| {
        value["substitutions"][0]["rectangle"]["col"] = serde_json::json!(0);
        value["substitutions"][0]["rectangle"]["rows"] = serde_json::json!(30);
    });

    // act: substitution geometry is validated.
    let error = result.expect_err("whole-height identity rectangle must fail");

    // assert: whole-region masks are rejected fail-closed.
    assert!(matches!(error, ScenarioError::InvalidSubstitution(_)));
}

#[test]
fn noncanonical_field_placeholder_is_rejected() {
    // arrange: a typed substitution with a functional canonical placeholder.
    let result = mutated_substitution_scenario(|value| {
        value["substitutions"][1]["canonical_placeholder"] = serde_json::json!("send");
    });

    // act: the field placeholder is validated.
    let error = result.expect_err("functional placeholder must fail");

    // assert: only the canonical field placeholder is accepted.
    assert!(matches!(error, ScenarioError::InvalidSubstitution(_)));
}

#[test]
fn unknown_substitution_field_is_rejected_at_the_serde_boundary() {
    // arrange
    let result = mutated_substitution_scenario(|value| {
        value["substitutions"][0]["field"] = serde_json::json!("home_directory");
    });

    // act
    // assert
    assert!(matches!(result, Err(ScenarioError::Deserialize(_))));
}

#[test]
fn duplicate_home_path_field_is_rejected_per_checkpoint() {
    // arrange
    let result = mutated_substitution_scenario(|value| {
        let mut home = value["substitutions"][0].clone();
        home["field"] = serde_json::json!("home_path");
        home["canonical_placeholder"] = serde_json::json!("[HOME]");
        value["substitutions"]
            .as_array_mut()
            .expect("substitutions array")
            .extend([home.clone(), home]);
    });

    // act
    // assert
    assert!(matches!(
        result,
        Err(ScenarioError::InvalidSubstitution(
            SubstitutionError::DuplicateField { .. }
        ))
    ));
}

#[test]
fn zero_action_tick_is_rejected() {
    // arrange: a timed action with an invalid zero tick.
    let result = mutated_scenario(|value| {
        value["actions"][0]["timed_key"]["at_tick"] = serde_json::json!(0);
    });

    // act: action timing is validated.
    let error = result.expect_err("zero tick must fail");

    // assert: timing failure is typed.
    assert!(matches!(error, ScenarioError::InvalidTiming(_)));
}

#[test]
fn out_of_order_action_ticks_are_rejected() {
    // arrange: the second action is moved before the first action's tick.
    let result = mutated_scenario(|value| {
        value["actions"][1]["paste"]["at_tick"] = serde_json::json!(1);
    });

    // act: action ordering is validated.
    let error = result.expect_err("out-of-order ticks must fail");

    // assert: the timeline cannot contain ambiguous ordering.
    assert!(matches!(error, ScenarioError::InvalidTiming(_)));
}

#[test]
fn action_and_checkpoint_tick_streams_may_interleave() {
    // arrange: rest is captured between action ticks while both streams remain ordered.
    let result = mutated_scenario(|value| {
        value["checkpoints"][0]["at_tick"] = serde_json::json!(3);
    });

    // act
    // assert: independent ordered streams form one deterministic timeline.
    assert!(
        result.is_ok(),
        "interleaved streams must validate: {result:?}"
    );
}

#[test]
fn equal_action_and_checkpoint_ticks_are_valid_action_first_boundaries() {
    // arrange: the final action and rest checkpoint share tick 26.
    let result = mutated_scenario(|value| {
        value["checkpoints"][0]["at_tick"] = serde_json::json!(26);
    });

    // act
    // assert: the runner contract orders the action before the checkpoint.
    assert!(
        result.is_ok(),
        "equal tick boundary must validate: {result:?}"
    );
}

#[test]
fn out_of_order_checkpoint_ticks_are_rejected_with_interleaved_actions() {
    // arrange: checkpoint ordering regresses even though action order is valid.
    let result = mutated_scenario(|value| {
        value["checkpoints"][0]["at_tick"] = serde_json::json!(7);
        value["checkpoints"][1]["at_tick"] = serde_json::json!(6);
    });

    // act
    let error = result.expect_err("out-of-order checkpoint ticks must fail");

    // assert
    assert!(matches!(error, ScenarioError::InvalidTiming(_)));
}

#[test]
fn zero_viewport_dimension_is_rejected() {
    // arrange: a scenario viewport with no columns.
    let result = mutated_scenario(|value| {
        value["viewport"]["cols"] = serde_json::json!(0);
    });

    // act: scenario geometry is validated.
    let error = result.expect_err("zero columns must fail");

    // assert: geometry failure is typed.
    assert!(matches!(error, ScenarioError::InvalidGeometry(_)));
}

#[test]
fn out_of_bounds_mouse_point_is_rejected() {
    // arrange: a mouse point outside the pre-resize viewport.
    let result = mutated_scenario(|value| {
        value["actions"][2]["mouse"]["point"]["col"] = serde_json::json!(80);
    });

    // act: action geometry is validated.
    let error = result.expect_err("out-of-bounds mouse point must fail");

    // assert: the point cannot escape the declared viewport.
    assert!(matches!(error, ScenarioError::InvalidGeometry(_)));
}

#[test]
fn negative_expected_exit_code_is_rejected() {
    // arrange: an impossible negative process exit code.
    let result = mutated_scenario(|value| {
        value["expected_exit"]["code"] = serde_json::json!(-1);
    });

    // act: expected exit metadata is validated.
    let error = result.expect_err("negative exit code must fail");

    // assert: exit failure is typed.
    assert!(matches!(error, ScenarioError::InvalidExitCode(_)));
}

#[test]
fn cleanup_must_restore_workspace_and_preserve_evidence() {
    // arrange: cleanup expectations that discard the workspace.
    let result = mutated_scenario(|value| {
        value["cleanup"]["restore_workspace"] = serde_json::json!(false);
    });

    // act: cleanup metadata is validated.
    let error = result.expect_err("non-restoring cleanup must fail");

    // assert: cleanup failure is typed and fail-closed.
    assert!(matches!(error, ScenarioError::InvalidCleanup(_)));
}

#[test]
fn cleanup_paths_must_be_relative_and_traversal_free() {
    // arrange: a cleanup path that escapes the scenario workspace.
    let result = mutated_scenario(|value| {
        value["cleanup"]["temporary_paths"][0] = serde_json::json!("../outside");
    });

    // act: cleanup paths are validated.
    let error = result.expect_err("escaping cleanup path must fail");

    // assert: path failure is typed.
    assert!(matches!(error, ScenarioError::InvalidCleanup(_)));
}

#[test]
fn rejects_invalid_motion_capture_contract() {
    // arrange
    let source = include_str!("../src/tui_fidelity_scenarios/baseline/cancel.json");
    let mut value: serde_json::Value = serde_json::from_str(source).expect("fixture JSON");
    value["motion_capture"]["markers"][1]["boundary"]["after_action"]["ordinal"] = 99.into();
    // act
    let error = harness_testkit::tui_fidelity::Scenario::from_json(&value.to_string())
        .expect_err("out-of-range marker rejected");
    // assert
    assert!(matches!(
        error,
        harness_testkit::tui_fidelity::ScenarioError::InvalidMotionCapture(_)
    ));
}

#[test]
fn empty_adapter_selection_is_rejected() {
    // arrange: a scenario with no selected adapter.
    let result = mutated_scenario(|value| {
        value["adapters"] = serde_json::json!([]);
    });

    // act: adapter selection is validated.
    let error = result.expect_err("empty adapter selection must fail");

    // assert: malformed selection is typed.
    assert!(matches!(error, ScenarioError::NoAdapters));
}

#[test]
fn unknown_adapter_kind_is_rejected_at_the_scenario_boundary() {
    // arrange: an adapter value outside the closed enum.
    let result = mutated_scenario(|value| {
        value["adapters"] = serde_json::json!(["unknown"]);
    });

    // act: the scenario is parsed.
    let error = result.expect_err("unknown adapter must fail");

    // assert: serde rejects unsupported adapter input as typed parse failure.
    assert!(matches!(error, ScenarioError::Deserialize(_)));
}
