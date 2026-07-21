//! Fail-closed disk evidence provenance tests for the reference-parity manifest.
//!
//! Contract: docs/grok-build-tui-implementation-prompt.md Wave 1 Packets 1.1/1.3.
//! Manifest: docs/tui-reference-parity-manifest.v1.json
//!
//! The strict validator is opt-in (`validate_manifest_evidence` against an
//! evidence root) because gitignored capture artifacts are absent in a clean
//! checkout. The `signoff-parity` lane sets `HARNESS_TUI_PARITY_STRICT=1` and
//! `HARNESS_TUI_PARITY_ARTIFACT_DIR` to drive the env-gated provenance test
//! against the lane's fresh evidence root.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration manifest tests use fail-fast asserts"
)]

use std::path::{Path, PathBuf};

use harness_tui::UnwrapOrAbort;
use serde_json::{json, Value};

#[path = "support/reference_parity_seed.rs"]
mod seed;
#[path = "support/reference_parity_status.rs"]
mod status;
#[path = "support/reference_parity_manifest.rs"]
mod support;

use seed::seed_claimed_row_evidence;
use status::{resolve_evidence_path, validate_manifest_evidence};
use support::{divergence_receipt_path, ValidateResult};

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

fn row_value<'a>(manifest: &'a Value, behavior_id: &str) -> &'a Value {
    manifest["rows"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .find(|row| row["behavior_id"].as_str() == Some(behavior_id))
        .unwrap_or_abort()
}

fn row_mut<'a>(manifest: &'a mut Value, behavior_id: &str) -> &'a mut Value {
    manifest["rows"]
        .as_array_mut()
        .unwrap_or_abort()
        .iter_mut()
        .find(|row| row["behavior_id"].as_str() == Some(behavior_id))
        .unwrap_or_abort()
}

fn seeded_evidence_root() -> (tempfile::TempDir, Value) {
    let root = tempfile::tempdir().unwrap_or_abort();
    let mut manifest = checked_in_manifest();
    seed_claimed_row_evidence(root.path(), &mut manifest);
    (root, manifest)
}

fn resolved_row_path(root: &Path, manifest: &Value, behavior_id: &str, field: &str) -> PathBuf {
    let declared = row_value(manifest, behavior_id)["evidence_paths"][field]
        .as_str()
        .unwrap_or_abort();
    resolve_evidence_path(manifest, root, declared)
}

fn freeze_receipt_path(root: &Path, manifest: &Value) -> PathBuf {
    let declared = manifest["reference"]["receipt_path"]
        .as_str()
        .unwrap_or_abort();
    resolve_evidence_path(manifest, root, declared)
}

fn overwrite_json(path: &Path, mutate: impl FnOnce(&mut Value)) {
    let mut parsed: Value =
        serde_json::from_slice(&std::fs::read(path).unwrap_or_abort()).unwrap_or_abort();
    mutate(&mut parsed);
    std::fs::write(
        path,
        serde_json::to_string_pretty(&parsed).unwrap_or_abort(),
    )
    .unwrap_or_abort();
}

#[test]
fn evidence_validator_passes_with_seeded_evidence_root() {
    // arrange
    let (root, manifest) = seeded_evidence_root();

    // act
    let result = validate_manifest_evidence(&manifest, root.path());

    // assert
    result.unwrap_or_else(|failures| {
        panic!("seeded evidence root failed validation: {failures:?}");
    });
}

#[test]
fn evidence_validator_rejects_missing_layer_file() {
    // arrange
    let (root, manifest) = seeded_evidence_root();
    let layer_file = resolved_row_path(root.path(), &manifest, "OVL-PALETTE", "L4");
    std::fs::remove_file(layer_file).unwrap_or_abort();

    // act
    let result = validate_manifest_evidence(&manifest, root.path());

    // assert
    assert_control(result, "missing-evidence-file");
}

#[test]
fn evidence_validator_rejects_stale_capture_digest() {
    // arrange
    let (root, mut manifest) = seeded_evidence_root();
    let artifact = row_mut(&mut manifest, "P0-START-01")["expected_semantic_cell_artifact"]
        .as_str()
        .unwrap_or_abort()
        .to_owned();
    let artifact_path = resolve_evidence_path(&manifest, root.path(), &artifact);
    std::fs::write(artifact_path, b"stale capture content").unwrap_or_abort();

    // act
    let result = validate_manifest_evidence(&manifest, root.path());

    // assert
    assert_control(result, "stale-evidence-digest");
}

#[test]
fn evidence_validator_rejects_missing_divergence_receipt_file() {
    // arrange
    let (root, manifest) = seeded_evidence_root();
    let note = manifest["identity_policy"]["approved_divergence_notes"]["DIV-AA-PALETTE"]
        .as_str()
        .unwrap_or_abort();
    let receipt_rel = divergence_receipt_path(note).unwrap_or_abort();
    std::fs::remove_file(root.path().join(receipt_rel)).unwrap_or_abort();

    // act
    let result = validate_manifest_evidence(&manifest, root.path());

    // assert
    assert_control(result, "missing-divergence-receipt");
}

#[test]
fn evidence_validator_rejects_copied_artifact_with_wrong_behavior_id() {
    // arrange
    let (root, manifest) = seeded_evidence_root();
    let metadata =
        resolved_row_path(root.path(), &manifest, "P0-START-01", "L3").join("metadata.json");
    std::fs::write(
        &metadata,
        br#"{ "behavior_id": "OVL-PALETTE", "viewport": { "cols": 120, "rows": 32 } }"#,
    )
    .unwrap_or_abort();

    // act
    let result = validate_manifest_evidence(&manifest, root.path());

    // assert
    assert_control(result, "copied-evidence-artifact");
}

#[test]
fn evidence_validator_rejects_copied_artifact_with_wrong_viewport() {
    // arrange
    let (root, manifest) = seeded_evidence_root();
    let metadata =
        resolved_row_path(root.path(), &manifest, "P0-START-01", "L3").join("metadata.json");
    std::fs::write(
        &metadata,
        br#"{ "behavior_id": "P0-START-01", "viewport": { "cols": 80, "rows": 24 } }"#,
    )
    .unwrap_or_abort();

    // act
    let result = validate_manifest_evidence(&manifest, root.path());

    // assert
    assert_control(result, "copied-evidence-artifact");
}

#[test]
fn evidence_validator_rejects_stale_embedded_receipt_digest() {
    // arrange
    let (root, manifest) = seeded_evidence_root();
    let receipt = resolved_row_path(root.path(), &manifest, "P0-START-01", "L4");
    let stale_digest = "0".repeat(64);
    overwrite_json(&receipt, |parsed| {
        parsed["reference"]["sha256"] = json!(stale_digest);
    });

    // act
    let result = validate_manifest_evidence(&manifest, root.path());

    // assert
    assert_control(result, "stale-evidence-digest");
}

#[test]
fn evidence_validator_rejects_freeze_receipt_binary_digest_mismatch() {
    // arrange
    let (root, manifest) = seeded_evidence_root();
    let receipt = freeze_receipt_path(root.path(), &manifest);
    let wrong_binary_digest = "0".repeat(64);
    overwrite_json(&receipt, |parsed| {
        parsed["global_pinned_reference"]["binary_sha256"] = json!(wrong_binary_digest);
    });

    // act
    let result = validate_manifest_evidence(&manifest, root.path());

    // assert
    assert_control(result, "reference-block-mismatch");
}

#[test]
fn evidence_validator_rejects_freeze_receipt_viewport_mismatch() {
    // arrange
    let (root, manifest) = seeded_evidence_root();
    let receipt = freeze_receipt_path(root.path(), &manifest);
    overwrite_json(&receipt, |parsed| {
        parsed["viewport"] = json!({ "cols": 99, "rows": 99 });
    });

    // act
    let result = validate_manifest_evidence(&manifest, root.path());

    // assert
    assert_control(result, "reference-block-mismatch");
}

#[test]
fn strict_evidence_provenance_under_signoff_environment() {
    // arrange
    if std::env::var("HARNESS_TUI_PARITY_STRICT").as_deref() != Ok("1") {
        return;
    }
    let root = std::env::var_os("HARNESS_TUI_PARITY_ARTIFACT_DIR")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .expect("HARNESS_TUI_PARITY_STRICT=1 requires HARNESS_TUI_PARITY_ARTIFACT_DIR");
    let manifest = checked_in_manifest();

    // act
    let result = validate_manifest_evidence(&manifest, &root);

    // assert
    result.unwrap_or_else(|failures| {
        panic!("fresh signoff evidence root failed strict provenance validation: {failures:?}");
    });
}
