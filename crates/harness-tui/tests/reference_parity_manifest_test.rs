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

#[path = "support/reference_parity_status.rs"]
mod status;
#[path = "support/reference_parity_manifest.rs"]
mod support;

use status::derive_status;
use support::{
    divergence_policy, rollup_status, validate_manifest, ValidateResult, ACCEPTANCE_GATES,
    FIRST_SLICE_IDS, FREEZE_PNG_SHA256, FREEZE_TXT_SHA256, REFERENCE_BINARY_SHA256,
    REQUIRED_SCAFFOLD_IDS, SCHEMA_VERSION,
};

const MANIFEST_SRC: &str =
    include_str!("../../../docs/reference/tui-reference-parity-manifest.v1.json");

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

fn row_mut<'a>(manifest: &'a mut Value, behavior_id: &str) -> &'a mut Value {
    manifest["rows"]
        .as_array_mut()
        .unwrap_or_abort()
        .iter_mut()
        .find(|row| row["behavior_id"].as_str() == Some(behavior_id))
        .unwrap_or_abort()
}

fn first_row_with_status_mut<'a>(manifest: &'a mut Value, status: &str) -> &'a mut Value {
    manifest["rows"]
        .as_array_mut()
        .unwrap_or_abort()
        .iter_mut()
        .find(|row| row["status"].as_str() == Some(status))
        .unwrap_or_abort()
}

fn make_p0_start_01_pass(manifest: &mut Value) {
    let row = row_mut(manifest, "P0-START-01");
    row["status"] = json!("pass");
    row["expected_semantic_cell_artifact"] =
        json!("target/test-lanes/latest/signoff-parity/evidence/reference/freeze/run1-startup/terminal.txt");
    row["expected_png_artifact"] =
        json!("target/test-lanes/latest/signoff-parity/evidence/reference/freeze/run1-startup/terminal.png");
    row["expected_frame_sequence"] =
        json!("target/test-lanes/latest/signoff-parity/evidence/reference/freeze/run1-startup/");
    row["evidence_paths"] = json!({
        "L1": "target/test-lanes/latest/signoff-parity/evidence/reference/freeze/run1-startup/",
        "L2": "target/test-lanes/latest/signoff-parity/evidence/harness/P0-START-01/cells/",
        "L3": "target/test-lanes/latest/signoff-parity/evidence/actual/harness-startup-v24/",
        "L4": "target/test-lanes/latest/signoff-parity/evidence/receipts/startup-pixel-diff-v24-precise-identity.json",
        "L5": "target/test-lanes/latest/signoff-parity/evidence/receipts/startup-pixel-diff-v24-masked.json",
        "L6": "target/test-lanes/latest/signoff-parity/evidence/receipts/startup-identity-field-mask.precise-v3.json"
    });
    row["owners"]["differential_evaluator"] =
        json!("target/test-lanes/latest/signoff-parity/evidence/receipts/startup-pixel-diff-v24-masked.json");
    row["reference_freeze_txt_sha256"] = json!(FREEZE_TXT_SHA256);
    row["reference_freeze_png_sha256"] = json!(FREEZE_PNG_SHA256);
    row["reference_capture_path"] = json!("reference/freeze/run1-startup");
    row["reference_txt_sha256"] = json!(FREEZE_TXT_SHA256);
    row["reference_png_sha256"] = json!(FREEZE_PNG_SHA256);
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
    // act
    // assert
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
        let status = row["status"].as_str().unwrap_or("");
        // First-slice rows are "incomplete": the startup/draft reference
        // freeze captures (run1-startup, run1-draft) and their harness
        // counterparts (harness-startup-v24, harness-draft-v23) are not
        // present in the evidence store. The rows are structurally valid
        // and remain required; evidence promotion requires real captures.
        assert!(
            status == "pass" || status == "incomplete",
            "first-slice {id} must be pass or incomplete; got {status:?}"
        );
        if status == "pass" {
            assert!(!row["expected_semantic_cell_artifact"]
                .as_str()
                .unwrap_or("")
                .is_empty());
            for layer in ["L1", "L2", "L3", "L4", "L5", "L6"] {
                assert!(
                    !row["evidence_paths"][layer]
                        .as_str()
                        .unwrap_or("")
                        .is_empty(),
                    "first-slice {id} evidence_paths.{layer} must be non-empty for pass"
                );
            }
        }
    }
    let user_approved_scaffold_diverged: [&str; 0] = [];
    // Clean-room v5 (lane 20260729-180826): the 7 responsive scaffold rows were
    // promoted to status=pass against fresh L1-L6 signoff-parity evidence, so
    // they are no longer demoted here. SHELL-QUESTION, SHELL-SCROLL, and
    // OVL-QUESTION were promoted in the same wave but were never in this list
    // (they fell through to the status=="pass" branch directly).
    let demoted_to_incomplete: [&str; 0] = [];
    // 2026-07-30: TX-TOOL, TX-DIFF, OVL-PALETTE, and OVL-SESSION were formally
    // excluded by approved scope. The tool/diff divergence is intentional
    // product chrome; palette/session captures are blocked on deterministic
    // scaffolding that is not in V1 scope.
    for id in REQUIRED_SCAFFOLD_IDS {
        assert!(ids.contains(*id), "missing scaffold id {id}");
        let row = rows
            .iter()
            .find(|row| row["behavior_id"].as_str() == Some(*id))
            .unwrap_or_abort();
        assert_eq!(row["slice"].as_str(), Some("scaffold"));
        let status = row["status"].as_str().unwrap_or("");
        let l5 = row["evidence_paths"]["L5"].as_str().unwrap_or("");
        if user_approved_scaffold_diverged.contains(id) {
            assert_eq!(
                status, "diverged",
                "scaffold {id} has user-approved AA divergence"
            );
            assert!(
                row["deliberate_divergence_id"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("DIV-AA-"),
                "scaffold {id} missing DIV-AA-* id"
            );
        } else if demoted_to_incomplete.contains(id) {
            assert_eq!(
                status, "incomplete",
                "scaffold {id} demoted: missing approved divergence or required evidence"
            );
        } else if status == "excluded" {
            // Formally approved scope exclusion; no evidence required.
        } else if status == "pass" {
            assert!(
                !l5.is_empty(),
                "scaffold {id} pass requires L5 evidence path"
            );
        } else if l5.contains("blocked") {
            assert_eq!(
                status, "blocked",
                "scaffold {id} freeze blocked/invalid pair"
            );
        } else {
            assert_eq!(status, "incomplete", "scaffold {id} still incomplete");
        }
    }

    assert_eq!(
        manifest["identity_policy"]["rejected_divergences"][0].as_str(),
        Some("DIV-004")
    );
    let rejected: BTreeSet<&str> = manifest["identity_policy"]["rejected_divergences"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        rejected.contains("DIV-AA-PALETTE"),
        "DIV-AA-PALETTE must remain in rejected_divergences after Wave 4.7 invalidation"
    );
    let approved: BTreeSet<&str> = manifest["identity_policy"]["approved_divergences"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert!(
        !approved.contains("DIV-AA-PALETTE"),
        "DIV-AA-PALETTE must NOT be in approved_divergences after Wave 4.7 invalidation"
    );
    assert_eq!(
        manifest["reference"]["binary_sha256"].as_str(),
        Some(REFERENCE_BINARY_SHA256)
    );
}

#[test]
fn validator_rejects_missing_required_field() {
    // arrange
    // act
    // assert
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
    // act
    // assert
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
    // act
    // assert
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
    // act
    // assert
    // arrange
    let mut manifest = checked_in_manifest();
    manifest["rows"][0]["owners"]["pty_test"] = json!("");

    // act / assert
    assert_control(validate_manifest(&manifest), "missing-owners");
}

#[test]
fn validator_rejects_invalid_status() {
    // arrange
    // act
    // assert
    // arrange
    let mut manifest = checked_in_manifest();
    manifest["rows"][0]["status"] = json!("accepted");

    // act / assert
    assert_control(validate_manifest(&manifest), "invalid-status");
}

#[test]
fn validator_rejects_conflicting_declared_and_evidence_statuses() {
    // arrange
    let mut manifest = checked_in_manifest();
    let row = row_mut(&mut manifest, "SHELL-IDLE");
    row["status"] = json!("incomplete");
    row["evidence"]["status"] = json!("pass");

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "status-evidence-mismatch");
}

#[test]
fn validator_rejects_invalid_acceptance_gate() {
    // arrange
    // act
    // assert
    // arrange
    let mut manifest = checked_in_manifest();
    manifest["rows"][0]["acceptance_gate_ids"] = json!(["A-MANIFEST", "A-NOT-A-GATE"]);

    // act / assert
    assert_control(validate_manifest(&manifest), "invalid-gates");
}

#[test]
fn validator_rejects_div_004_as_deliberate_divergence() {
    // arrange
    // act
    // assert
    // arrange
    let mut manifest = checked_in_manifest();
    manifest["rows"][0]["deliberate_divergence_id"] = json!("DIV-004");

    // act / assert
    assert_control(validate_manifest(&manifest), "div-004-rejected");
}

#[test]
fn validator_rejects_missing_div_004_rejection_policy() {
    // arrange
    // act
    // assert
    // arrange
    let mut manifest = checked_in_manifest();
    manifest["identity_policy"]["rejected_divergences"] = json!([]);

    // act / assert
    assert_control(validate_manifest(&manifest), "div-004-rejected");
}

#[test]
fn coexists_with_signoff_manifest_without_requiring_reference_images() {
    // arrange
    // act
    // assert
    // arrange / act
    let signoff: Value = serde_json::from_str(include_str!(
        "../../../docs/testing/tui-signoff-manifest.v1.json"
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

#[test]
fn checked_in_manifest_lists_expanded_acceptance_gates() {
    // arrange
    // act
    // assert
    // arrange
    let manifest = checked_in_manifest();
    let listed = manifest["acceptance_gate_ids"]
        .as_array()
        .unwrap_or_abort()
        .iter()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    let required = ACCEPTANCE_GATES.iter().copied().collect::<BTreeSet<_>>();

    // assert — top-level must include every §4.1 gate (fail-closed allowlist)
    assert_eq!(listed, required);
    for gate in [
        "A-CAPABILITIES",
        "A-CORE-AUDIT",
        "A-CONFIG-SCHEMA",
        "A-FUNCTIONAL",
        "A-JOURNEYS",
        "A-ANIMATION",
    ] {
        assert!(listed.contains(gate), "missing expanded gate {gate}");
    }
}

#[test]
fn validator_rejects_missing_expanded_top_level_gate() {
    // arrange
    // act
    // assert
    // arrange
    let mut manifest = checked_in_manifest();
    let gates = manifest["acceptance_gate_ids"]
        .as_array_mut()
        .unwrap_or_abort();
    gates.retain(|gate| gate.as_str() != Some("A-CAPABILITIES"));

    // act / assert
    assert_control(validate_manifest(&manifest), "missing-acceptance-gates");
}

#[test]
fn checked_in_manifest_status_rollup_is_truthful_and_not_complete() {
    // arrange
    let manifest = checked_in_manifest();

    // act
    let rollup = rollup_status(&manifest);

    // assert — structural consistency only; no hard-coded pass/divergence counts.
    assert!(rollup.required > 0, "manifest must contain rows");
    assert_eq!(
        rollup.pass + rollup.incomplete + rollup.blocked + rollup.diverged + rollup.excluded,
        rollup.required,
        "status counts must sum to required"
    );
    assert_eq!(rollup.unknown, 0, "no unknown statuses allowed");
    // Visual rows remain required but unclaimed until fresh paired evidence
    // exists; incomplete is the truthful fail-closed rollup for this fixture.
    assert!(
        !rollup.a_manifest_complete(),
        "A-MANIFEST must remain incomplete while visual evidence is unclaimed"
    );
}

#[test]
#[allow(
    clippy::panic,
    clippy::unreachable,
    reason = "fail-closed on unexpected journey id"
)]
fn checked_in_journey_rows_are_promoted_pass_with_paired_evidence() {
    // arrange
    let manifest = checked_in_manifest();
    let rows = manifest["rows"].as_array().unwrap_or_abort();
    let l1_prefix = "target/test-lanes/latest/signoff-parity/evidence/reference/freeze/journey-";
    let l4_prefix = "target/test-lanes/latest/signoff-parity/evidence/receipts/journey-";
    let journey_ids = [
        "JOURNEY-WORKTREE-CTRL-W",
        "JOURNEY-CONFIG-SHOW-EFFECTIVE",
        "JOURNEY-CONFIG-SOURCES-EXPLAIN",
        "JOURNEY-WAIT-ANY-ALL",
        "JOURNEY-FOLDER-TRUST-DENY",
        "JOURNEY-MEMORY-CLI",
        "JOURNEY-ALWAYS-APPROVE-MODE",
        "JOURNEY-SETTINGS-EDITOR",
    ];

    // assert — Wave 6 (2026-07-30 signoff-parity clean room): the lane
    // regenerates fresh L3 captures, copies the pinned L1 reference-CLI
    // freezes and L4 nonvisual differential receipts into the fresh evidence
    // root, and promotes every journey row to pass with its honesty demotion
    // restored (Contract §4).
    for journey_id in journey_ids {
        let row = rows
            .iter()
            .find(|row| row["behavior_id"].as_str() == Some(journey_id))
            .unwrap_or_else(|| panic!("missing journey template {journey_id}"));
        assert_eq!(row["row_kind"].as_str(), Some("journey"));
        assert_eq!(
            row["status"].as_str(),
            Some("pass"),
            "{journey_id} must be promoted to pass"
        );
        assert_eq!(row["journey_id"].as_str(), Some(journey_id));
        assert!(
            !row["capability_id"].as_str().unwrap_or("").is_empty(),
            "{journey_id} capability_id must not be empty"
        );
        assert!(
            !row["backend_owner"].as_str().unwrap_or("").is_empty(),
            "{journey_id} backend_owner must not be empty"
        );
        assert_eq!(
            row["honesty_demotion"]["restored"].as_bool(),
            Some(true),
            "{journey_id} honesty_demotion.restored must be true after promotion"
        );
        let l1 = row["evidence_paths"]["L1"].as_str().unwrap_or("");
        assert!(
            l1.starts_with(l1_prefix) && l1.ends_with("-l1-ref-v1/"),
            "{journey_id} L1 must be a canonical reference-CLI freeze path, got {l1}"
        );
        let l4 = row["evidence_paths"]["L4"].as_str().unwrap_or("");
        assert!(
            l4.starts_with(l4_prefix) && l4.ends_with("-l4-differential-v1.json"),
            "{journey_id} L4 must be a canonical differential receipt path, got {l4}"
        );
        for layer in ["L3", "L6"] {
            let value = row["evidence_paths"][layer].as_str().unwrap_or("");
            assert!(!value.is_empty(), "{journey_id} {layer} must not be empty");
        }
        assert!(
            row["expected_semantic_cell_artifact"]
                .as_str()
                .unwrap_or("")
                .is_empty(),
            "{journey_id} nonvisual journey must not declare semantic-cell artifacts"
        );
    }
}

#[test]
fn validator_rejects_journey_row_missing_capability_join() {
    // arrange
    // act
    // assert
    // arrange
    let mut manifest = checked_in_manifest();
    let rows = manifest["rows"].as_array_mut().unwrap_or_abort();
    let journey = rows
        .iter_mut()
        .find(|row| row["behavior_id"].as_str() == Some("JOURNEY-WORKTREE-CTRL-W"))
        .unwrap_or_abort();
    journey
        .as_object_mut()
        .unwrap_or_abort()
        .remove("capability_id");

    // act / assert
    assert_control(validate_manifest(&manifest), "missing-journey-join");
}

#[test]
fn validator_rejects_diverged_with_null_divergence_id() {
    // arrange — no checked-in row is diverged (Wave 4.7), so synthesize one
    let mut manifest = checked_in_manifest();
    make_p0_start_01_pass(&mut manifest);
    let diverged_row = row_mut(&mut manifest, "P0-START-01");
    diverged_row["status"] = json!("diverged");
    diverged_row["deliberate_divergence_id"] = json!(null);

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "missing-divergence-id");
}

#[test]
fn validator_rejects_diverged_with_absent_divergence_id() {
    // arrange — no checked-in row is diverged (Wave 4.7), so synthesize one
    let mut manifest = checked_in_manifest();
    make_p0_start_01_pass(&mut manifest);
    let diverged_row = row_mut(&mut manifest, "P0-START-01");
    diverged_row["status"] = json!("diverged");
    diverged_row
        .as_object_mut()
        .unwrap_or_abort()
        .remove("deliberate_divergence_id");

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "missing-divergence-id");
}

#[test]
fn validator_rejects_diverged_with_unauthorized_divergence_id() {
    // arrange — no checked-in row is diverged (Wave 4.7), so synthesize one
    let mut manifest = checked_in_manifest();
    make_p0_start_01_pass(&mut manifest);
    let diverged_row = row_mut(&mut manifest, "P0-START-01");
    diverged_row["status"] = json!("diverged");
    diverged_row["deliberate_divergence_id"] = json!("DIV-NOT-APPROVED");

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "unauthorized-divergence");
}

#[test]
fn validator_rejects_pending_owner_on_pass_row() {
    // arrange
    // act
    // assert
    // arrange
    let mut manifest = checked_in_manifest();
    make_p0_start_01_pass(&mut manifest);
    let pass_row = row_mut(&mut manifest, "P0-START-01");
    pass_row["owners"]["render_test"] = json!("pending");

    // act / assert
    assert_control(validate_manifest(&manifest), "pending-owner");
}

#[test]
fn validator_rejects_diverged_with_empty_divergence_id() {
    // arrange — no checked-in row is diverged (Wave 4.7), so synthesize one
    let mut manifest = checked_in_manifest();
    make_p0_start_01_pass(&mut manifest);
    let diverged_row = row_mut(&mut manifest, "P0-START-01");
    diverged_row["status"] = json!("diverged");
    diverged_row["deliberate_divergence_id"] = json!("");

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "missing-divergence-id");
}

#[test]
fn validator_rejects_pending_owner_on_diverged_row() {
    // arrange — no checked-in row is diverged (Wave 4.7), so synthesize one
    let mut manifest = checked_in_manifest();
    make_p0_start_01_pass(&mut manifest);
    let diverged_row = row_mut(&mut manifest, "P0-START-01");
    diverged_row["status"] = json!("diverged");
    diverged_row["deliberate_divergence_id"] = json!("DIV-AA-SHELL-FAIL");
    diverged_row["owners"]["render_test"] = json!("pending");

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "pending-owner");
}

#[test]
fn validator_rejects_pass_claim_with_missing_applicable_layer() {
    // arrange — promote SHELL-STREAM to pass with all layers; clear L2 to test rejection.
    let mut manifest = checked_in_manifest();
    let row = row_mut(&mut manifest, "SHELL-STREAM");
    row["status"] = json!("pass");
    row["evidence_paths"] = json!({
        "L1": "target/test-lanes/latest/signoff-parity/evidence/reference/freeze/run2-shell-stream-pinned-v2/",
        "L2": "",
        "L3": "target/test-lanes/latest/signoff-parity/evidence/actual/harness-shell-stream-pinned-v1/",
        "L4": "target/test-lanes/latest/signoff-parity/evidence/receipts/shell-stream-pixel-diff-v16-masked.json",
        "L5": "target/test-lanes/latest/signoff-parity/evidence/receipts/shell-stream-divergence-receipt-v16.json",
        "L6": "target/test-lanes/latest/signoff-parity/evidence/receipts/shell-stream-identity-field-mask-v16.json"
    });

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "missing-evidence-layer");
}

#[test]
fn validator_rejects_pass_claim_with_empty_artifact_declaration() {
    // arrange
    let mut manifest = checked_in_manifest();
    make_p0_start_01_pass(&mut manifest);
    row_mut(&mut manifest, "P0-START-01")["expected_png_artifact"] = json!("");

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "missing-evidence-layer");
}

#[test]
fn validator_rejects_pass_row_with_stale_freeze_digest() {
    // arrange
    let mut manifest = checked_in_manifest();
    make_p0_start_01_pass(&mut manifest);
    row_mut(&mut manifest, "P0-START-01")["reference_freeze_txt_sha256"] = json!(FREEZE_PNG_SHA256);

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "stale-evidence-digest");
}

#[test]
fn validator_rejects_pass_row_with_malformed_digest_field() {
    // arrange
    let mut manifest = checked_in_manifest();
    make_p0_start_01_pass(&mut manifest);
    row_mut(&mut manifest, "P0-START-01")["reference_txt_sha256"] = json!("not-a-sha256-digest");

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "invalid-evidence-digest");
}

#[test]
fn validator_rejects_viewport_inconsistent_with_responsive_behavior_id() {
    // arrange
    let mut manifest = checked_in_manifest();
    row_mut(&mut manifest, "RESP-80x24")["viewport"]["cols"] = json!(120);

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "state-viewport-mismatch");
}

#[test]
fn validator_rejects_viewport_inconsistent_with_reference_freeze() {
    // arrange
    let mut manifest = checked_in_manifest();
    row_mut(&mut manifest, "P0-START-01")["viewport"] = json!({ "cols": 80, "rows": 24 });

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "state-viewport-mismatch");
}

#[test]
fn validator_rejects_diverged_without_declared_receipt_note() {
    // arrange — no checked-in row is diverged (Wave 4.7), so synthesize one
    let mut manifest = checked_in_manifest();
    make_p0_start_01_pass(&mut manifest);
    let diverged_row = row_mut(&mut manifest, "P0-START-01");
    diverged_row["status"] = json!("diverged");
    diverged_row["deliberate_divergence_id"] = json!("DIV-AA-SHELL-FAIL");
    manifest["identity_policy"]["approved_divergence_notes"]["DIV-AA-SHELL-FAIL"] =
        json!("User-approved pure AA residual. Receipt marker removed.");

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "missing-divergence-receipt");
}

#[test]
fn validator_rejects_diverged_evidence_backed_status() {
    // arrange
    // The "evidence-backed divergence" category is forbidden; only
    // incomplete/blocked/pass/diverged statuses are allowed.
    let mut manifest = checked_in_manifest();
    row_mut(&mut manifest, "P0-START-01")["status"] = json!("diverged_evidence_backed");

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "invalid-status");
}

#[test]
fn derive_status_demotes_claims_with_evidence_gaps() {
    // arrange
    let mut manifest = checked_in_manifest();
    make_p0_start_01_pass(&mut manifest);
    let pass_row = row_mut(&mut manifest, "P0-START-01").clone();
    // No row in the checked-in manifest is diverged anymore (DIV-AA-PALETTE
    // was rejected in Wave 4.7), so synthesize the diverged and blocked cases
    // from a promoted pass row.
    let diverged_row = {
        let mut row = pass_row.clone();
        row["status"] = json!("incomplete");
        row["deliberate_divergence_id"] = json!("DIV-AA-SHELL-FAIL");
        row
    };
    let gap_row = {
        let mut row = pass_row.clone();
        row["evidence_paths"]["L4"] = json!("");
        row
    };
    let blocked_row = {
        let mut row = pass_row.clone();
        row["status"] = json!("incomplete");
        row["deliberate_divergence_id"] = json!("DIV-NOT-APPROVED");
        row
    };
    let policy = divergence_policy(&manifest);

    // act
    let pass_derived = derive_status(&pass_row, &policy);
    let diverged_derived = derive_status(&diverged_row, &policy);
    let gap_derived = derive_status(&gap_row, &policy);
    let blocked_derived = derive_status(&blocked_row, &policy);

    // assert
    assert_eq!(pass_derived, "pass");
    assert_eq!(diverged_derived, "diverged");
    assert_eq!(
        gap_derived, "incomplete",
        "evidence gaps must derive incomplete"
    );
    assert_eq!(
        blocked_derived, "blocked",
        "unapproved divergences must derive blocked"
    );
}
