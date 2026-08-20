#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "contract-owner fixtures fail fast"
)]

use std::fs;
use std::path::{Path, PathBuf};

use harness_testkit::tui_fidelity_matrix::{
    validate_coverage_documents, validate_scenario_registry, CoverageManifest,
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn active_documents() -> (serde_json::Value, serde_json::Value) {
    let root = workspace_root();
    let inventory =
        fs::read_to_string(root.join("configs/tui-fidelity-requirement-inventory.json"))
            .expect("active inventory");
    let manifest = fs::read_to_string(root.join("configs/tui-fidelity-coverage-manifest.json"))
        .expect("active manifest");
    (
        serde_json::from_str(&inventory).expect("inventory JSON"),
        serde_json::from_str(&manifest).expect("manifest JSON"),
    )
}

fn validate(inventory: &serde_json::Value, manifest: &serde_json::Value) -> Result<(), String> {
    validate_coverage_documents(&inventory.to_string(), &manifest.to_string())
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[test]
fn active_coverage_contract_accepts_complete_scenario_set() {
    // arrange
    let (inventory, manifest) = active_documents();
    // act
    let result = validate(&inventory, &manifest);
    // assert
    result.expect("active coverage documents satisfy the Rust contract");
}

#[test]
fn active_coverage_contract_rejects_missing_requirement_row() {
    // arrange
    let (inventory, mut manifest) = active_documents();
    manifest["rows"].as_array_mut().expect("rows").remove(0);
    // act
    let error = validate(&inventory, &manifest).expect_err("missing row must fail closed");
    // assert
    assert!(error.contains("missing requirement_id"), "{error}");
}

#[test]
fn active_coverage_contract_rejects_duplicate_coverage_row() {
    // arrange
    let (inventory, mut manifest) = active_documents();
    let row = manifest["rows"][0].clone();
    manifest["rows"].as_array_mut().expect("rows").push(row);
    // act
    let error = validate(&inventory, &manifest).expect_err("duplicate row must fail closed");
    // assert
    assert!(error.contains("duplicate row_id"), "{error}");
}

#[test]
fn active_coverage_contract_rejects_unmapped_requirement() {
    // arrange
    let (inventory, mut manifest) = active_documents();
    manifest["rows"][0]["requirement_id"] = serde_json::json!("missing.requirement");
    // act
    let error = validate(&inventory, &manifest).expect_err("unmapped row must fail closed");
    // assert
    assert!(error.contains("unmapped requirement_id"), "{error}");
}

#[test]
fn active_coverage_contract_rejects_grouped_wildcard_scenario() {
    // arrange
    let (inventory, mut manifest) = active_documents();
    manifest["rows"][0]["scenario_id"] = serde_json::json!("*");
    // act
    let error = validate(&inventory, &manifest).expect_err("wildcard must fail closed");
    // assert
    assert!(error.contains("grouped wildcard"), "{error}");
}

#[test]
fn active_coverage_contract_rejects_unbound_plan_digest() {
    // arrange
    let (inventory, mut manifest) = active_documents();
    manifest["reviewed_plan_sha256"] = serde_json::json!("0".repeat(64));
    // act
    let error = validate(&inventory, &manifest).expect_err("unbound digest must fail closed");
    // assert
    assert!(error.contains("plan digest differs"), "{error}");
}

#[test]
fn active_coverage_contract_rejects_absent_failure_dimension() {
    // arrange
    let (inventory, mut manifest) = active_documents();
    manifest["rows"][0]["failure_path"] = serde_json::json!("");
    // act
    let error = validate(&inventory, &manifest).expect_err("empty dimension must fail closed");
    // assert
    assert!(error.contains("empty coverage dimension"), "{error}");
}

#[test]
fn active_coverage_contract_rejects_wrong_trial_count() {
    // arrange
    let (inventory, mut manifest) = active_documents();
    manifest["rows"][0]["trials"] = serde_json::json!(4);
    // act
    let error = validate(&inventory, &manifest).expect_err("wrong trials must fail closed");
    // assert
    assert!(error.contains("exactly 5 trials"), "{error}");
}

#[test]
fn active_registry_binds_every_manifest_scenario() {
    // arrange
    let root = workspace_root();
    let (_, manifest_value) = active_documents();
    let manifest: CoverageManifest =
        serde_json::from_value(manifest_value).expect("typed manifest");
    let registry = fs::read_to_string(
        root.join("crates/harness-testkit/src/tui_fidelity_scenarios/baseline/registry.json"),
    )
    .expect("active registry");
    // act
    let report = validate_scenario_registry(&registry, &manifest).expect("registered scenarios");
    // assert
    assert!(!report.active_families.is_empty());
    assert!(!report.registered_non_acceptance_families.is_empty());
}

#[test]
fn retired_python_validator_and_reference_inventory_stay_absent() {
    // arrange
    let root = workspace_root();
    let retired = [
        root.join("scripts/parity_task_qa.py"),
        root.join("docs/grok-reference-interaction-inventory.v1.json"),
    ];
    // act
    let all_absent = retired.iter().all(|path| !path.exists());
    // assert
    assert!(all_absent);
}

#[test]
fn cancellation_contract_orders_cancel_before_recovery() {
    // arrange
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/parity/motion.rs"))
            .expect("motion contract source");
    // act
    let cancellation = source
        .find("MotionPhase::Cancellation")
        .expect("cancellation");
    let recovery = source
        .find("MotionPhase::CancelRecovered")
        .expect("recovery");
    // assert
    assert!(cancellation < recovery);
}
