#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast fixture assertions"
)]

use harness_testkit::tui_fidelity_closure::validate_boulder_for_completion;

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
        "status": "verified",
    })
    .to_string()
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
