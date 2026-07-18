//! Independent Harness-owned TUI reference-parity manifest validator.
//!
//! Contract: docs/grok-build-tui-implementation-prompt.md §4.2 and §9.
//! Manifest: docs/tui-reference-parity-manifest.v1.json
//!
//! Does not replace docs/tui-signoff-manifest.v1.json.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration manifest tests use fail-fast asserts"
)]

use std::collections::BTreeSet;

use harness_tui::UnwrapOrAbort;
use serde_json::{json, Value};

#[path = "support/reference_parity_manifest.rs"]
mod support;

use support::{
    validate_manifest, ValidateResult, FIRST_SLICE_IDS, REFERENCE_BINARY_SHA256,
    REQUIRED_SCAFFOLD_IDS, SCHEMA_VERSION,
};

const MANIFEST_SRC: &str = include_str!("../../../docs/tui-reference-parity-manifest.v1.json");

fn checked_in_manifest() -> Value {
    serde_json::from_str(MANIFEST_SRC).unwrap_or_abort()
}

fn assert_control(result: ValidateResult, control: &str) {
    let failures = result.expect_err("expected validation failure");
    assert!(
        failures.iter().any(|failure| failure.control == control),
        "expected control {control}, got {failures:?}"
    );
}

#[test]
fn checked_in_reference_parity_manifest_is_valid() {
    // arrange
    let manifest = checked_in_manifest();

    // act
    let result = validate_manifest(&manifest);

    // assert
    result.unwrap_or_else(|failures| {
        panic!("checked-in manifest failed validation: {failures:?}");
    });
}

#[test]
fn checked_in_manifest_covers_first_slice_and_scaffolds() {
    // arrange
    let manifest = checked_in_manifest();
    let rows = manifest["rows"].as_array().unwrap_or_abort();
    let ids = rows
        .iter()
        .filter_map(|row| row["behavior_id"].as_str())
        .collect::<BTreeSet<_>>();

    // assert
    for id in FIRST_SLICE_IDS {
        assert!(ids.contains(*id), "missing first-slice id {id}");
        let row = rows
            .iter()
            .find(|row| row["behavior_id"].as_str() == Some(*id))
            .unwrap_or_abort();
        assert_eq!(row["slice"].as_str(), Some("first"));
        assert!(
            !row["expected_semantic_cell_artifact"]
                .as_str()
                .unwrap_or("")
                .is_empty()
        );
        assert!(row["evidence_paths"]["L4"]
            .as_str()
            .unwrap_or("")
            .contains("artifacts/qa-evidence/20260717-tui-reference-parity"));
    }
    for id in REQUIRED_SCAFFOLD_IDS {
        assert!(ids.contains(*id), "missing scaffold id {id}");
        let row = rows
            .iter()
            .find(|row| row["behavior_id"].as_str() == Some(*id))
            .unwrap_or_abort();
        assert_eq!(row["status"].as_str(), Some("incomplete"));
        assert_eq!(row["slice"].as_str(), Some("scaffold"));
    }

    assert_eq!(
        manifest["identity_policy"]["rejected_divergences"][0].as_str(),
        Some("DIV-004")
    );
    assert_eq!(
        manifest["reference"]["binary_sha256"].as_str(),
        Some(REFERENCE_BINARY_SHA256)
    );
}

#[test]
fn validator_rejects_missing_required_field() {
    // arrange
    let mut manifest = checked_in_manifest();
    manifest["rows"][0]
        .as_object_mut()
        .unwrap_or_abort()
        .remove("expected_focus_owner");

    // act / assert
    assert_control(validate_manifest(&manifest), "missing-required-field");
}

#[test]
fn validator_rejects_duplicate_behavior_ids() {
    // arrange
    let mut manifest = checked_in_manifest();
    let duplicate = manifest["rows"][0].clone();
    manifest["rows"]
        .as_array_mut()
        .unwrap_or_abort()
        .push(duplicate);

    // act / assert
    assert_control(validate_manifest(&manifest), "duplicate-id");
}

#[test]
fn validator_rejects_missing_owners() {
    // arrange
    let mut manifest = checked_in_manifest();
    manifest["rows"][0]["owners"]
        .as_object_mut()
        .unwrap_or_abort()
        .remove("render_test");

    // act / assert
    assert_control(validate_manifest(&manifest), "missing-owners");
}

#[test]
fn validator_rejects_empty_owner_string() {
    // arrange
    let mut manifest = checked_in_manifest();
    manifest["rows"][0]["owners"]["pty_test"] = json!("");

    // act / assert
    assert_control(validate_manifest(&manifest), "missing-owners");
}

#[test]
fn validator_rejects_invalid_status() {
    // arrange
    let mut manifest = checked_in_manifest();
    manifest["rows"][0]["status"] = json!("accepted");

    // act / assert
    assert_control(validate_manifest(&manifest), "invalid-status");
}

#[test]
fn validator_rejects_invalid_acceptance_gate() {
    // arrange
    let mut manifest = checked_in_manifest();
    manifest["rows"][0]["acceptance_gate_ids"] = json!(["A-MANIFEST", "A-NOT-A-GATE"]);

    // act / assert
    assert_control(validate_manifest(&manifest), "invalid-gates");
}

#[test]
fn validator_rejects_div_004_as_deliberate_divergence() {
    // arrange
    let mut manifest = checked_in_manifest();
    manifest["rows"][0]["deliberate_divergence_id"] = json!("DIV-004");

    // act / assert
    assert_control(validate_manifest(&manifest), "div-004-rejected");
}

#[test]
fn validator_rejects_missing_div_004_rejection_policy() {
    // arrange
    let mut manifest = checked_in_manifest();
    manifest["identity_policy"]["rejected_divergences"] = json!([]);

    // act / assert
    assert_control(validate_manifest(&manifest), "div-004-rejected");
}

#[test]
fn coexists_with_signoff_manifest_without_requiring_reference_images() {
    // arrange / act
    let signoff: Value = serde_json::from_str(include_str!(
        "../../../docs/tui-signoff-manifest.v1.json"
    ))
    .unwrap_or_abort();
    let parity = checked_in_manifest();

    // assert — leave signoff policy alone; parity is a separate contract
    assert_eq!(signoff["reference_image_policy"], "not_required");
    assert_eq!(parity["schema_version"].as_str(), Some(SCHEMA_VERSION));
    assert_ne!(
        parity["schema_version"].as_str(),
        signoff["schema_version"].as_str()
    );
}
