#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast fixture assertions"
)]

use harness_testkit::tui_fidelity_matrix::{
    execute_matrix, execute_matrix_bounded, read_coverage_documents, validate_coverage_documents,
    CoverageManifest, RequirementInventory,
};

#[test]
fn coverage_validation_rejects_duplicate_rows_and_requirements() {
    let inventory = inventory_json(&["req-a", "req-b"]);
    let manifest = manifest_json(&[("row-a", "req-a"), ("row-a", "req-b"), ("row-b", "req-b")]);

    let error = validate_coverage_documents(&inventory, &manifest)
        .expect_err("duplicate rows and requirements must fail");

    let message = error.to_string();
    assert!(message.contains("duplicate row_id"));
    assert!(message.contains("duplicate requirement_id"));
}

#[test]
fn coverage_validation_rejects_omitted_and_unmapped_requirements() {
    let inventory = inventory_json(&["req-a", "req-b"]);
    let manifest = manifest_json(&[("row-a", "req-a"), ("row-c", "req-c")]);

    let error = validate_coverage_documents(&inventory, &manifest)
        .expect_err("omitted and unmapped requirements must fail");

    let message = error.to_string();
    assert!(message.contains("missing requirement_id req-b"));
    assert!(message.contains("unmapped requirement_id req-c"));
}

#[test]
fn checked_in_coverage_manifest_has_one_row_per_requirement() {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let inventory = repo_root.join("configs/tui-fidelity-requirement-inventory.json");
    let manifest = repo_root.join("configs/tui-fidelity-coverage-manifest.json");

    let (_, _, report) = read_coverage_documents(&inventory, &manifest)
        .expect("checked-in inventory and manifest must validate");

    assert_eq!(report.requirement_count, 547);
    assert_eq!(report.row_count, 547);
    assert_eq!(report.capture_key_count, 452);
}

#[test]
fn matrix_executes_shared_capture_key_once_without_counterfeit_repetitions() {
    // Given: two requirements with the same execution-effective capture inputs.
    let inventory_json = inventory_json(&["req-a", "req-b"]);
    let manifest_json = manifest_json(&[("row-a", "req-a"), ("row-b", "req-b")]);
    let inventory: RequirementInventory =
        serde_json::from_str(&inventory_json).expect("inventory fixture");
    let manifest: CoverageManifest =
        serde_json::from_str(&manifest_json).expect("manifest fixture");
    let report =
        validate_coverage_documents(&inventory_json, &manifest_json).expect("coverage fixture");
    let evidence = tempfile::tempdir().expect("matrix evidence");
    let executions = std::cell::Cell::new(0_usize);

    // When: the matrix executor runs each derived capture key once.
    let receipt = execute_matrix(manifest, report, "synthetic", evidence.path(), |_| {
        executions.set(executions.get() + 1);
        Ok((true, true, "passed".to_owned()))
    })
    .expect("deduplicated matrix");

    // Then: runtime work and evidence accounting both occur once per row.
    assert_eq!(inventory.requirements.len(), 2);
    assert_eq!(executions.get(), 1);
    assert_eq!(receipt.report.capture_key_count, 1);
    assert_eq!(receipt.rows.len(), 2);
    assert!(receipt
        .rows
        .iter()
        .all(|row| row.execution.capture_succeeded && row.execution.comparison_passed));
}

#[test]
fn bounded_matrix_runs_distinct_capture_keys_concurrently() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    let inventory_json = inventory_json(&["req-a", "req-b"]);
    let manifest_json = manifest_json_with_scenarios(&[
        ("row-a", "req-a", "synthetic-a"),
        ("row-b", "req-b", "synthetic-b"),
    ]);
    let _inventory: RequirementInventory =
        serde_json::from_str(&inventory_json).expect("inventory fixture");
    let manifest: CoverageManifest =
        serde_json::from_str(&manifest_json).expect("manifest fixture");
    let report =
        validate_coverage_documents(&inventory_json, &manifest_json).expect("coverage fixture");
    let evidence = tempfile::tempdir().expect("matrix evidence");
    let active = Arc::new(AtomicUsize::new(0));
    let maximum = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(2));

    let receipt = execute_matrix_bounded(manifest, report, "synthetic", evidence.path(), 2, {
        let active = Arc::clone(&active);
        let maximum = Arc::clone(&maximum);
        let barrier = Arc::clone(&barrier);
        move |_| {
            let current = active.fetch_add(1, Ordering::SeqCst) + 1;
            maximum.fetch_max(current, Ordering::SeqCst);
            barrier.wait();
            active.fetch_sub(1, Ordering::SeqCst);
            Ok((true, true, "passed".to_owned()))
        }
    })
    .expect("bounded matrix");

    assert_eq!(maximum.load(Ordering::SeqCst), 2);
    assert_eq!(receipt.report.capture_key_count, 2);
    assert_eq!(receipt.rows.len(), 2);
    assert!(receipt
        .rows
        .iter()
        .all(|row| row.execution.capture_succeeded && row.execution.comparison_passed));
}

fn inventory_json(ids: &[&str]) -> String {
    serde_json::json!({
        "schema_version": "harness.tui-fidelity.requirement-inventory.v1",
        "reviewed_plan_sha256": "a".repeat(64),
        "requirements": ids.iter().map(|id| serde_json::json!({
            "id": id,
            "source_line": 1,
            "title": id,
            "obligation": {"type": "dual_capture"},
        })).collect::<Vec<_>>(),
    })
    .to_string()
}

fn manifest_json(rows: &[(&str, &str)]) -> String {
    let rows = rows
        .iter()
        .map(|(row_id, requirement_id)| (*row_id, *requirement_id, "synthetic"));
    manifest_json_with_scenarios(&rows.collect::<Vec<_>>())
}

fn manifest_json_with_scenarios(rows: &[(&str, &str, &str)]) -> String {
    serde_json::json!({
        "schema_version": "harness.tui-fidelity.coverage-manifest.v1",
        "reviewed_plan_sha256": "a".repeat(64),
        "inventory_sha256": "b".repeat(64),
        "rows": rows.iter().map(|(row_id, requirement_id, scenario_id)| serde_json::json!({
            "row_id": row_id,
            "requirement_id": requirement_id,
            "scenario_id": scenario_id,
            "action_path": "synthetic-action",
            "path_classification": "native_path",
            "viewport": {"cols": 80, "rows": 24},
            "terminal_tier": "truecolor",
            "persona": "keyboard",
            "theme_mode": "default",
            "media_mode": "none",
            "failure_path": "none",
            "trials": 5,
        })).collect::<Vec<_>>(),
    })
    .to_string()
}
