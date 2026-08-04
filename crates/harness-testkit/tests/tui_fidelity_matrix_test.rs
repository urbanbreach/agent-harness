#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast fixture assertions"
)]

use harness_testkit::tui_fidelity_matrix::{read_coverage_documents, validate_coverage_documents};

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
    assert_eq!(report.trial_count, 2_735);
}

fn inventory_json(ids: &[&str]) -> String {
    serde_json::json!({
        "schema_version": "harness.tui-fidelity.requirement-inventory.v1",
        "reviewed_plan_sha256": "a".repeat(64),
        "requirements": ids.iter().map(|id| serde_json::json!({
            "id": id,
            "source_line": 1,
            "title": id,
        })).collect::<Vec<_>>(),
    })
    .to_string()
}

fn manifest_json(rows: &[(&str, &str)]) -> String {
    serde_json::json!({
        "schema_version": "harness.tui-fidelity.coverage-manifest.v1",
        "reviewed_plan_sha256": "a".repeat(64),
        "inventory_sha256": "b".repeat(64),
        "rows": rows.iter().map(|(row_id, requirement_id)| serde_json::json!({
            "row_id": row_id,
            "requirement_id": requirement_id,
            "scenario_id": "synthetic",
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
