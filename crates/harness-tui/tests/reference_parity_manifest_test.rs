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
    FIRST_SLICE_IDS, FREEZE_PNG_SHA256, REFERENCE_BINARY_SHA256, REQUIRED_SCAFFOLD_IDS,
    SCHEMA_VERSION,
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
        assert!(!row["expected_semantic_cell_artifact"]
            .as_str()
            .unwrap_or("")
            .is_empty());
        assert!(row["evidence_paths"]["L4"]
            .as_str()
            .unwrap_or("")
            .contains("artifacts/qa-evidence/20260717-tui-reference-parity"));
    }
    let user_approved_scaffold_diverged: [&str; 0] = [];
    let demoted_to_incomplete: [&str; 0] = [];
    // Scaffolds blocked on external reference evidence (Wave 4 Packets 4.1-4.6):
    // shell-idle/stream freezes captured from the pinned binary but A-PIXELS
    // fails closed on unmasked chrome residuals; perm/question blocked because
    // the reference tool UIs are not reachable via black-box tool-call
    // injection; turn-lifecycle rows blocked on state/content divergence;
    // transcript primitive rows blocked on non-deterministic body content
    // (TX-USER/ASSISTANT), scenario-dependent tool chrome (TX-TOOL), and the
    // reference not projecting inline diff bodies (TX-DIFF).
    let blocked_on_reference_evidence = [
        "SHELL-IDLE",
        "SHELL-STREAM",
        "SHELL-PERM",
        "OVL-PERM",
        "SHELL-QUESTION",
        "OVL-QUESTION",
        "SHELL-CANCEL",
        "SHELL-FAIL",
        "SHELL-RECOVER",
        "SHELL-COMPLETE",
        "SHELL-SCROLL",
        "TX-USER",
        "TX-ASSISTANT",
        "TX-TOOL",
        "TX-DIFF",
        "OVL-PALETTE",
    ];
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
        } else if blocked_on_reference_evidence.contains(id) {
            assert_eq!(
                status, "blocked",
                "scaffold {id} blocked on reference pixel evidence"
            );
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
    let signoff: Value =
        serde_json::from_str(include_str!("../../../docs/tui-signoff-manifest.v1.json"))
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
        rollup.pass + rollup.incomplete + rollup.blocked + rollup.diverged,
        rollup.required,
        "status counts must sum to required"
    );
    assert_eq!(rollup.unknown, 0, "no unknown statuses allowed");
    assert!(
        !rollup.a_manifest_complete(),
        "A-MANIFEST must not be complete while product capability gaps remain"
    );
}

#[test]
#[allow(
    clippy::panic,
    clippy::unreachable,
    reason = "fail-closed on unexpected journey id"
)]
fn checked_in_journey_templates_join_inventory_without_fake_l1() {
    // arrange
    // act
    // assert
    // arrange
    let manifest = checked_in_manifest();
    let rows = manifest["rows"].as_array().unwrap_or_abort();
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

    // assert
    for journey_id in journey_ids {
        let row = rows
            .iter()
            .find(|row| row["behavior_id"].as_str() == Some(journey_id))
            .unwrap_or_else(|| panic!("missing journey template {journey_id}"));
        assert_eq!(row["row_kind"].as_str(), Some("journey"));
        let expected_status = match journey_id {
            "JOURNEY-WORKTREE-CTRL-W"
            | "JOURNEY-CONFIG-SHOW-EFFECTIVE"
            | "JOURNEY-CONFIG-SOURCES-EXPLAIN"
            | "JOURNEY-WAIT-ANY-ALL"
            | "JOURNEY-FOLDER-TRUST-DENY"
            | "JOURNEY-MEMORY-CLI"
            | "JOURNEY-ALWAYS-APPROVE-MODE"
            | "JOURNEY-SETTINGS-EDITOR" => "incomplete",
            _ => panic!("unexpected checked-in journey id: {journey_id}"),
        };
        assert_eq!(row["status"].as_str(), Some(expected_status));
        assert_eq!(row["journey_id"].as_str(), Some(journey_id));
        assert!(!row["capability_id"].as_str().unwrap_or("").is_empty());
        assert!(!row["backend_owner"].as_str().unwrap_or("").is_empty());
        // No invented freeze digests: L1 evidence stays empty until real capture
        let l1 = row["evidence_paths"]["L1"].as_str().unwrap_or("");
        assert!(
            l1.is_empty(),
            "{journey_id} must not invent L1 freeze paths, got {l1}"
        );
        assert!(row["expected_semantic_cell_artifact"]
            .as_str()
            .unwrap_or("")
            .is_empty());
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
    let diverged_row = first_row_with_status_mut(&mut manifest, "pass");
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
    let diverged_row = first_row_with_status_mut(&mut manifest, "pass");
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
    let diverged_row = first_row_with_status_mut(&mut manifest, "pass");
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
    let rows = manifest["rows"].as_array_mut().unwrap_or_abort();
    let pass_row = rows
        .iter_mut()
        .find(|row| row["status"].as_str() == Some("pass"))
        .unwrap_or_abort();
    pass_row["owners"]["render_test"] = json!("pending");

    // act / assert
    assert_control(validate_manifest(&manifest), "pending-owner");
}

#[test]
fn validator_rejects_diverged_with_empty_divergence_id() {
    // arrange — no checked-in row is diverged (Wave 4.7), so synthesize one
    let mut manifest = checked_in_manifest();
    let diverged_row = first_row_with_status_mut(&mut manifest, "pass");
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
    let diverged_row = first_row_with_status_mut(&mut manifest, "pass");
    diverged_row["status"] = json!("diverged");
    diverged_row["deliberate_divergence_id"] = json!("DIV-AA-PALETTE");
    diverged_row["owners"]["render_test"] = json!("pending");

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "pending-owner");
}

#[test]
fn validator_rejects_pass_claim_with_missing_applicable_layer() {
    // arrange
    // SHELL-STREAM is incomplete with an empty L2 owner-evidence layer.
    let mut manifest = checked_in_manifest();
    row_mut(&mut manifest, "SHELL-STREAM")["status"] = json!("pass");

    // act
    let result = validate_manifest(&manifest);

    // assert
    assert_control(result, "missing-evidence-layer");
}

#[test]
fn validator_rejects_pass_claim_with_empty_artifact_declaration() {
    // arrange
    let mut manifest = checked_in_manifest();
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
    let diverged_row = first_row_with_status_mut(&mut manifest, "pass");
    diverged_row["status"] = json!("diverged");
    diverged_row["deliberate_divergence_id"] = json!("DIV-AA-PALETTE");
    manifest["identity_policy"]["approved_divergence_notes"]["DIV-AA-PALETTE"] =
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
    let pass_row = row_mut(&mut manifest, "P0-START-01").clone();
    // No row in the checked-in manifest is diverged anymore (OVL-PALETTE's
    // DIV-AA-PALETTE approval was invalidated in Wave 4.7), so synthesize the
    // diverged case from an approval still recorded in the identity policy.
    let diverged_row = {
        let mut row = row_mut(&mut manifest, "OVL-PALETTE").clone();
        row["deliberate_divergence_id"] = json!("DIV-AA-PALETTE");
        row
    };
    let gap_row = {
        let mut row = pass_row.clone();
        row["evidence_paths"]["L4"] = json!("");
        row
    };
    let blocked_row = {
        let mut row = diverged_row.clone();
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
