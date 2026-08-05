#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast fixture assertions"
)]

use std::fs;

use harness_testkit::tui_fidelity_closure::{
    ClosureVerificationInput, ReviewReceiptInput, complete_boulder_atomically,
    validate_boulder_for_completion, verify_closure,
};
use harness_testkit::tui_fidelity_compare::hash_bytes;

#[test]
fn closure_validation_rejects_direct_completed_state() {
    let boulder = boulder_json("completed", "completed");
    let receipt = valid_receipt();

    let error = validate_boulder_for_completion(&boulder, &receipt)
        .expect_err("direct completed state must not be accepted");

    assert!(error.to_string().contains("active-to-completed transition"));
}

#[test]
fn closure_validation_rejects_works_mirror_divergence() {
    let boulder = boulder_json("active", "paused");
    let receipt = valid_receipt();

    let error = validate_boulder_for_completion(&boulder, &receipt)
        .expect_err("works and mirror divergence must fail closed");

    assert!(error.to_string().contains("works/mirror divergence"));
}

#[test]
fn closure_verify_rejects_mismatched_contract_plan_sha() {
    let mut fixture = closure_fixture();
    let mut contract: serde_json::Value =
        serde_json::from_str(&fixture.contract_json).expect("fixture contract JSON");
    contract["reviewed_plan_sha256"] = serde_json::Value::String("f".repeat(64));
    fixture.contract_json = serde_json::to_string(&contract).expect("fixture contract JSON");

    let error = verify_closure(fixture.input()).expect_err("stale contract must fail closed");

    assert!(
        error
            .to_string()
            .contains("contract plan digest differs from live plan")
    );
}

#[test]
fn closure_verify_rejects_missing_preliminary_reviews() {
    let mut fixture = closure_fixture();
    fixture.review_receipts.clear();

    let error = verify_closure(fixture.input()).expect_err("missing reviews must fail closed");

    assert!(
        error
            .to_string()
            .contains("exactly two preliminary reviews are required")
    );
}

#[test]
fn closure_complete_rejects_replayed_receipt() {
    let directory = tempfile::tempdir().expect("temporary closure directory");
    let boulder_path = directory.path().join("boulder.json");
    let mut boulder: serde_json::Value =
        serde_json::from_str(&boulder_json("active", "active")).expect("fixture Boulder JSON");
    boulder["closure_receipt_sha256"] = serde_json::Value::String("a".repeat(64));
    fs::write(
        &boulder_path,
        serde_json::to_string(&boulder).expect("fixture Boulder JSON"),
    )
    .expect("write Boulder fixture");

    let error = complete_boulder_atomically(&boulder_path, &valid_receipt())
        .expect_err("a consumed receipt must not be replayed");

    assert!(
        error
            .to_string()
            .contains("closure receipt has already been consumed")
    );
}

#[test]
fn closure_complete_rejects_works_mirror_divergence() {
    let directory = tempfile::tempdir().expect("temporary closure directory");
    let boulder_path = directory.path().join("boulder.json");
    fs::write(&boulder_path, boulder_json("active", "paused")).expect("write Boulder fixture");

    let error = complete_boulder_atomically(&boulder_path, &valid_receipt())
        .expect_err("divergent works and mirror must fail closed");

    assert!(error.to_string().contains("works/mirror divergence"));
}

fn valid_receipt() -> String {
    serde_json::json!({
        "schema_version": "harness.tui-fidelity.closure-receipt.v1",
        "recovery_run_id": "run-1",
        "reviewed_plan_sha256": "a".repeat(64),
        "requirement_inventory_sha256": "b".repeat(64),
        "coverage_manifest_sha256": "c".repeat(64),
        "candidate_sha256": "d".repeat(40),
        "evidence_root": "/tmp/evidence",
        "nonce": "e".repeat(64),
        "receipt_path": "/tmp/evidence/closure-receipt.json",
        "receipt_sha256": "f".repeat(64),
        "status": "verified",
    })
    .to_string()
}

struct ClosureFixture {
    contract_json: String,
    plan_bytes: Vec<u8>,
    inventory_json: String,
    manifest_json: String,
    revocation_json: String,
    review_receipts: Vec<ReviewReceiptInput>,
    candidate_sha256: String,
}

impl ClosureFixture {
    fn input(&self) -> ClosureVerificationInput<'_> {
        ClosureVerificationInput {
            contract_json: &self.contract_json,
            plan_bytes: &self.plan_bytes,
            inventory_json: &self.inventory_json,
            manifest_json: &self.manifest_json,
            revocation_json: &self.revocation_json,
            review_receipts: &self.review_receipts,
            candidate_sha256: &self.candidate_sha256,
            evidence_root: "/tmp/closure-test",
        }
    }
}

fn closure_fixture() -> ClosureFixture {
    let plan_bytes = b"- [ ] 1. Requirement\n".to_vec();
    let plan_sha256 = hash_bytes(&plan_bytes).expect("plan hash");
    let inventory_json = serde_json::json!({
        "schema_version": "harness.tui-fidelity.requirement-inventory.v1",
        "reviewed_plan_sha256": plan_sha256,
        "requirements": [{
            "id": "global.design",
            "source_line": 1,
            "title": "Requirement",
            "obligation": {"type": "dual_capture"},
        }],
    })
    .to_string();
    let inventory_sha256 = hash_bytes(inventory_json.as_bytes()).expect("inventory hash");
    let manifest_json = serde_json::json!({
        "schema_version": "harness.tui-fidelity.coverage-manifest.v1",
        "reviewed_plan_sha256": plan_sha256,
        "inventory_sha256": inventory_sha256,
        "rows": [{
            "row_id": "coverage-requirement",
            "requirement_id": "global.design",
            "scenario_id": "synthetic",
            "action_path": "synthetic-action",
            "path_classification": "native_path",
            "viewport": {"cols": 80, "rows": 24},
            "terminal_tier": "truecolor",
            "persona": "keyboard-first",
            "theme_mode": "default",
            "media_mode": "none",
            "failure_path": "none",
            "trials": 5,
        }],
    })
    .to_string();
    let manifest_sha256 = hash_bytes(manifest_json.as_bytes()).expect("manifest hash");
    let contract_json = serde_json::json!({
        "schema_version": "harness.tui-fidelity.closure-contract.v1",
        "reviewed_plan_sha256": plan_sha256,
        "requirement_inventory_sha256": inventory_sha256,
        "coverage_manifest_sha256": manifest_sha256,
        "recovery_run_id": "run-1",
        "preliminary_reviews": [
            {"review_id": "review-a", "receipt_path": "review-a.json"},
            {"review_id": "review-b", "receipt_path": "review-b.json"},
        ],
    })
    .to_string();
    let review_receipts = [("review-a", "session-a"), ("review-b", "session-b")]
        .into_iter()
        .map(|(review_id, reviewer_session)| ReviewReceiptInput {
            review_id: review_id.to_owned(),
            json: serde_json::json!({
                "verdict": "APPROVE",
                "plan_sha256": plan_sha256,
                "inventory_sha256": inventory_sha256,
                "reviewer_session": reviewer_session,
            })
            .to_string(),
        })
        .collect();

    ClosureFixture {
        contract_json,
        plan_bytes,
        inventory_json,
        manifest_json,
        revocation_json: serde_json::json!({
            "status": "REJECTED",
            "reviewed_plan_sha256": plan_sha256,
        })
        .to_string(),
        review_receipts,
        candidate_sha256: "d".repeat(40),
    }
}

fn boulder_json(works_status: &str, mirror_status: &str) -> String {
    let work = serde_json::json!({
        "work_id": "work-1",
        "active_plan": ".omo/plans/plan.md",
        "plan_name": "plan",
        "status": works_status,
        "session_ids": ["opencode:session"],
    });
    serde_json::json!({
        "schema_version": 2,
        "active_work_id": "work-1",
        "works": {"work-1": work},
        "active_plan": ".omo/plans/plan.md",
        "plan_name": "plan",
        "status": mirror_status,
        "session_ids": ["opencode:session"],
    })
    .to_string()
}
