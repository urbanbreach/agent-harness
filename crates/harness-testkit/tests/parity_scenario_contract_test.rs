//! Todo 33 contract: differential scenario + holdout driver validator.
//!
//! These tests spawn `python3 scripts/parity_task_qa.py validate-scenarios`
//! against hermetic inventory fixtures and assert on the observable JSON
//! verdict. They lock the Wave 4 Todo 33 contract: the validator MUST accept
//! a complete scenario set with `coverage=100% missing=0
//! copied_reference_assets=0` and MUST reject the seven failure mutations
//! named in the plan (missing row, duplicate coverage, grouped wildcard,
//! missing teardown, absent failure mutation, copied-reference fingerprint,
//! unbound epoch).
//!
//! Behavior is asserted, never prose. Tests read only the structured
//! `scenario-validation.json` artifact that the validator emits.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "test fixtures fail fast when the scheduler source or python are unavailable"
)]

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn scheduler_path() -> PathBuf {
    workspace_root().join("scripts/parity_task_qa.py")
}

/// A hermetic inventory fixture with two rows covering P0 and P1.
/// `row_key` lets mutation tests relocate the second row so coverage stays
/// computable. Built deliberately without referencing any reference source byte.
fn write_inventory(dir: &Path, second_symbol: &str) -> PathBuf {
    let row_a = r#"{
  "category": "action",
  "source_path": "crates/codegen/xai-grok-pager/src/actions/mod.rs",
  "source_symbol": "ActionId::SendPrompt",
  "line": 31,
  "trigger": "key_event_or_command_palette",
  "focus_owner": "prompt",
  "state_transition": "dispatches_SendPrompt",
  "rendered_effect": "prompt_submission",
  "side_effect": "provider_request",
  "persistence": "session_event",
  "viewport_capability_conditions": "none",
  "approved_disposition": "pending",
  "p0_p9_applicability": "P0",
  "notes": "hermetic fixture row A"
}"#
    .to_string();
    let row_b = format!(
        r#"{{
  "category": "action",
  "source_path": "crates/codegen/xai-grok-pager/src/actions/mod.rs",
  "source_symbol": "{second_symbol}",
  "line": 42,
  "trigger": "key_event_or_command_palette",
  "focus_owner": "scrollback",
  "state_transition": "dispatches_{second_symbol}_tail",
  "rendered_effect": "scroll_movement",
  "side_effect": "none",
  "persistence": "none",
  "viewport_capability_conditions": "alternate_screen",
  "approved_disposition": "pending",
  "p0_p9_applicability": "P1",
  "notes": "hermetic fixture row B"
}}"#
    );
    let inventory = format!(
        r#"{{
  "schema_version": "grok-reference-interaction-inventory-v1",
  "document_id": "grok-reference-interaction-inventory",
  "program": "grok-build-clean-room-parity",
  "contract": "test fixture",
  "metadata": {{
    "reference_revision": "f",
    "reference_source_root": "hermetic",
    "reference_binary": "hermetic",
    "generated_by": "parity_scenario_contract_test",
    "count_targets": {{"actions": 2}},
    "actual_counts": {{"action": 2}},
    "approved_disposition_policy": "all_rows_pending",
    "reference_epoch": "test-epoch-bound"
  }},
  "rows": [{row_a}, {row_b}]
}}"#
    );
    let path = dir.join("inventory.json");
    fs::write(&path, inventory).expect("inventory writable");
    path
}

#[derive(Debug)]
struct RunResult {
    exit: i32,
    stdout: String,
    stderr: String,
    validation_json: Option<serde_json::Value>,
}

fn parse_stdout_json(stdout: &str) -> Option<serde_json::Value> {
    // The validator prints a single JSON object on the last non-empty line.
    let last = stdout.lines().rev().find(|line| !line.trim().is_empty())?;
    serde_json::from_str::<serde_json::Value>(last).ok()
}

fn run_validator(inventory: &Path, dir: &Path, extra_env: &[(&str, &str)]) -> RunResult {
    let scenario_root = dir.join("scenarios");
    let holdout_index = dir.join("holdout-index.json");
    let output_path = dir.join("scenario-validation.json");
    let mut cmd = Command::new("python3");
    cmd.arg(scheduler_path())
        .arg("validate-scenarios")
        .arg("--inventory")
        .arg(inventory)
        .arg("--scenario-root")
        .arg(&scenario_root)
        .arg("--holdout-index")
        .arg(&holdout_index)
        .arg("--output")
        .arg(&output_path)
        .arg("--require-happy")
        .arg("1")
        .arg("--require-failure")
        .arg("1")
        .arg("--require-mutation")
        .arg("1")
        .arg("--require-coverage")
        .arg("100");
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
    let produced = cmd.output().expect("python3 spawn succeeds");
    let stdout = String::from_utf8_lossy(&produced.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&produced.stderr).into_owned();
    let validation_json = fs::read_to_string(&output_path)
        .ok()
        .and_then(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
        .or_else(|| parse_stdout_json(&stdout));
    RunResult {
        exit: produced.status.code().unwrap_or(-1),
        stdout,
        stderr,
        validation_json,
    }
}

/// Read the validation JSON the validator wrote to `--output`.
fn read_validation_artifact(dir: &Path) -> serde_json::Value {
    let path = dir.join("scenario-validation.json");
    let body = fs::read_to_string(&path).expect("scenario-validation.json is written");
    serde_json::from_str(&body).expect("scenario-validation.json is valid JSON")
}

fn assert_pass(result: RunResult, label: &str) -> serde_json::Value {
    assert_eq!(
        result.exit, 0,
        "{label}: validator exited {} stderr={}",
        result.exit, result.stderr
    );
    assert!(
        !result.stderr.contains("rejected"),
        "{label}: validator wrote rejected verdict: {stderr}",
        stderr = result.stderr
    );
    result
        .validation_json
        .unwrap_or_else(|| panic!("{label}: validator emitted no JSON verdict"))
}

fn assert_reject(result: RunResult, label: &str, needle: &str) {
    assert_ne!(
        result.exit, 0,
        "{label}: validator unexpectedly accepted (exit 0)"
    );
    let combined = format!("{} {}", result.stdout, result.stderr);
    assert!(
        combined.contains(needle),
        "{label}: validator rejected for the wrong reason (expected needle {needle:?}); combined={combined}",
    );
}

#[test]
fn validate_scenarios_accepts_complete_scenario_set_with_full_coverage() {
    // arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let inventory = write_inventory(dir.path(), "ActionId::ScrollDown");

    // act
    let result = run_validator(&inventory, dir.path(), &[]);

    // assert
    let value = assert_pass(result, "happy-path-complete-set");
    assert_eq!(value["coverage_percent"], 100);
    assert_eq!(value["missing_count"], 0);
    assert_eq!(value["copied_reference_assets"], 0);
    assert_eq!(value["verdict"], "pass");

    // The validator must split published conformance from undisclosed holdouts.
    let holdout_index = dir.path().join("holdout-index.json");
    let holdout = serde_json::from_str::<serde_json::Value>(
        &fs::read_to_string(&holdout_index).expect("holdout-index.json exists"),
    )
    .expect("holdout index parses");
    assert!(
        holdout["holdouts"].is_array(),
        "holdout index must list undisclosed holdouts"
    );
    let scenario_root = dir.path().join("scenarios");
    let scenario_files = fs::read_dir(&scenario_root)
        .expect("scenario-root exists")
        .count();
    assert!(
        scenario_root.is_dir() && scenario_files > 0,
        "published conformance scenarios must exist under scenario-root"
    );
}

#[test]
fn validate_scenarios_rejects_missing_row() {
    // arrange: second row points to a symbol never materialized as a scenario.
    // We exercise missing coverage by feeding the validator a pre-baked
    // scenario-root that intentionally omits one row.
    let dir = tempfile::tempdir().expect("temp dir");
    let inventory = write_inventory(dir.path(), "ActionId::ScrollUp");
    // Run once to materialize scenarios, then corrupt the row->scenario map
    // by deleting the scenario files for one row key. We do this by running
    // with an env override the self-test honours to drop one row.
    let result = run_validator(
        &inventory,
        dir.path(),
        &[("PARITY_MUTATION", "missing-row")],
    );

    // assert
    assert_reject(result, "missing-row", "missing");
}

#[test]
fn validate_scenarios_rejects_duplicate_coverage() {
    // arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let inventory = write_inventory(dir.path(), "ActionId::Duplicate");

    // act
    let result = run_validator(
        &inventory,
        dir.path(),
        &[("PARITY_MUTATION", "duplicate-coverage")],
    );

    // assert
    assert_reject(result, "duplicate-coverage", "duplicate");
}

#[test]
fn validate_scenarios_rejects_grouped_wildcard_scenario() {
    // arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let inventory = write_inventory(dir.path(), "ActionId::Wild");

    // act
    let result = run_validator(
        &inventory,
        dir.path(),
        &[("PARITY_MUTATION", "grouped-wildcard")],
    );

    // assert
    assert_reject(result, "grouped-wildcard", "wildcard");
}

#[test]
fn validate_scenarios_rejects_missing_teardown() {
    // arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let inventory = write_inventory(dir.path(), "ActionId::NoTeardown");

    // act
    let result = run_validator(
        &inventory,
        dir.path(),
        &[("PARITY_MUTATION", "missing-teardown")],
    );

    // assert
    assert_reject(result, "missing-teardown", "teardown");
}

#[test]
fn validate_scenarios_rejects_absent_failure_mutation() {
    // arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let inventory = write_inventory(dir.path(), "ActionId::NoMutation");

    // act
    let result = run_validator(
        &inventory,
        dir.path(),
        &[("PARITY_MUTATION", "absent-mutation")],
    );

    // assert
    assert_reject(result, "absent-mutation", "mutation");
}

#[test]
fn validate_scenarios_rejects_copied_reference_fingerprint() {
    // arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let inventory = write_inventory(dir.path(), "ActionId::CopiedHash");

    // act
    let result = run_validator(
        &inventory,
        dir.path(),
        &[("PARITY_MUTATION", "copied-reference-fingerprint")],
    );

    // assert
    assert_reject(result, "copied-reference-fingerprint", "copied_reference");
}

#[test]
fn validate_scenarios_rejects_unbound_epoch() {
    // arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let inventory = write_inventory(dir.path(), "ActionId::UnboundEpoch");

    // act
    let result = run_validator(
        &inventory,
        dir.path(),
        &[("PARITY_MUTATION", "unbound-epoch")],
    );

    // assert
    assert_reject(result, "unbound-epoch", "epoch");
}

#[test]
fn validate_scenarios_covers_every_inventory_row() {
    // arrange: use the published canonical inventory.
    let inventory = workspace_root().join("docs/grok-reference-interaction-inventory.v1.json");
    if !inventory.is_file() {
        panic!("canonical inventory missing: {inventory:?}");
    }

    // act
    let dir = tempfile::tempdir().expect("temp dir");
    let result = run_validator(&inventory, dir.path(), &[]);

    // assert
    let value = assert_pass(result, "canonical-inventory-full-coverage");
    assert_eq!(value["coverage_percent"], 100);
    assert_eq!(value["missing_count"], 0);
    assert_eq!(value["copied_reference_assets"], 0);

    // Every row in the inventory must appear in the published conformance set.
    let validation = read_validation_artifact(dir.path());
    let inventory_rows: HashSet<String> = serde_json::from_str::<serde_json::Value>(
        &fs::read_to_string(&inventory).expect("inventory readable"),
    )
    .expect("inventory parses")["rows"]
        .as_array()
        .expect("rows is an array")
        .iter()
        .map(|row| {
            format!(
                "{}::{}::{}",
                row["category"].as_str().unwrap_or(""),
                row["source_path"].as_str().unwrap_or(""),
                row["source_symbol"].as_str().unwrap_or("")
            )
        })
        .collect();
    let covered: HashSet<String> = validation["row_coverage"]
        .as_array()
        .expect("row_coverage is an array")
        .iter()
        .map(|entry| {
            format!(
                "{}::{}::{}",
                entry["category"].as_str().unwrap_or(""),
                entry["source_path"].as_str().unwrap_or(""),
                entry["source_symbol"].as_str().unwrap_or("")
            )
        })
        .collect();
    let uncovered: Vec<&String> = inventory_rows.difference(&covered).collect();
    assert!(
        uncovered.is_empty(),
        "validator left {} inventory rows uncovered: {uncovered:?}",
        uncovered.len()
    );
}

#[test]
fn validate_scenarios_self_test_passes() {
    // arrange: --self-test on the validate-scenarios surface must exercise
    // every mutation internally and exit 0.
    let output = Command::new("python3")
        .arg(scheduler_path())
        .arg("--self-test")
        .output()
        .expect("python3 spawn succeeds");

    // assert
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "--self-test failed: stdout={stdout} stderr={stderr}"
    );
    assert!(
        stdout.contains("self-test") || stderr.contains("self-test"),
        "--self-test must report its own pass"
    );
}

#[test]
fn cancellation_contract_orders_cancel_before_recovery() {
    use harness_testkit::parity::MotionPhase;
    use harness_testkit::tui_fidelity::Scenario;

    let scenario = Scenario::from_json(include_str!(
        "../src/tui_fidelity_scenarios/baseline/cancel.json"
    ))
    .expect("cancellation scenario");
    let phases = scenario
        .motion_capture
        .markers
        .iter()
        .map(|marker| marker.phase)
        .collect::<Vec<_>>();
    let cancellation = phases
        .iter()
        .position(|phase| *phase == MotionPhase::Cancellation)
        .expect("cancellation marker");
    let recovery = phases
        .iter()
        .position(|phase| *phase == MotionPhase::CancelRecovered)
        .expect("recovery marker");
    assert!(cancellation < recovery);
    assert_eq!(phases.last(), Some(&MotionPhase::SettleRepeat));
}
