#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast fixture assertions"
)]

use harness_testkit::tui_fidelity_matrix::{
    execute_matrix, execute_matrix_bounded, read_coverage_documents, validate_coverage_documents,
    validate_scenario_registry, CoverageManifest, RequirementInventory,
};

#[test]
fn coverage_validation_rejects_duplicate_rows_and_requirements() {
    // arrange
    let inventory = inventory_json(&["req-a", "req-b"]);
    let manifest = manifest_json(&[("row-a", "req-a"), ("row-a", "req-b"), ("row-b", "req-b")]);

    // act
    let error = validate_coverage_documents(&inventory, &manifest)
        .expect_err("duplicate rows and requirements must fail");

    // assert
    let message = error.to_string();
    assert!(message.contains("duplicate row_id"));
    assert!(message.contains("duplicate requirement_id"));
}

#[test]
fn coverage_validation_rejects_omitted_and_unmapped_requirements() {
    // arrange
    let inventory = inventory_json(&["req-a", "req-b"]);
    let manifest = manifest_json(&[("row-a", "req-a"), ("row-c", "req-c")]);

    // act
    let error = validate_coverage_documents(&inventory, &manifest)
        .expect_err("omitted and unmapped requirements must fail");

    // assert
    let message = error.to_string();
    assert!(message.contains("missing requirement_id req-b"));
    assert!(message.contains("unmapped requirement_id req-c"));
}

#[test]
fn checked_in_coverage_manifest_has_one_row_per_requirement() {
    // arrange
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let inventory = repo_root.join("configs/tui-fidelity-requirement-inventory.json");
    let manifest = repo_root.join("configs/tui-fidelity-coverage-manifest.json");

    // act
    let (_, _, report) = read_coverage_documents(&inventory, &manifest)
        .expect("checked-in inventory and manifest must validate");

    // assert
    assert_eq!(report.requirement_count, 547);
    assert_eq!(report.row_count, 547);
    assert_eq!(report.capture_key_count, 452);
}

#[test]
fn matrix_executes_every_row_and_five_trials_when_capture_keys_are_shared() {
    // arrange: two row obligations with the same execution-effective capture inputs.
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

    // act: the matrix executor expands the row/trial bijection.
    let receipt = execute_matrix(manifest, report, "synthetic", evidence.path(), |_| {
        executions.set(executions.get() + 1);
        Ok((true, true, "passed".to_owned()))
    })
    .expect("complete matrix");

    // assert: neither the shared capture key nor the trial loop loses a row obligation.
    assert_eq!(inventory.requirements.len(), 2);
    assert_eq!(executions.get(), 10);
    assert_eq!(receipt.report.capture_key_count, 1);
    assert_eq!(receipt.report.execution_count, 10);
    assert_eq!(receipt.rows.len(), 2);
    assert!(receipt.rows.iter().all(|row| row.executions.len() == 5
        && row
            .executions
            .iter()
            .all(|execution| execution.capture_succeeded && execution.comparison_passed)));
}

#[test]
fn bounded_matrix_runs_distinct_capture_keys_concurrently() {
    // arrange
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

    // act
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

    // assert
    assert_eq!(maximum.load(Ordering::SeqCst), 2);
    assert_eq!(receipt.report.capture_key_count, 2);
    assert_eq!(receipt.report.execution_count, 10);
    assert_eq!(receipt.rows.len(), 2);
    assert!(receipt.rows.iter().all(|row| row.executions.len() == 5));
}

#[test]
fn checked_in_matrix_expands_547_rows_to_exactly_2735_bijective_executions() {
    use std::collections::BTreeSet;
    use std::sync::Mutex;

    // arrange: the checked-in 547-row active coverage contract.
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let inventory = repo_root.join("configs/tui-fidelity-requirement-inventory.json");
    let manifest = repo_root.join("configs/tui-fidelity-coverage-manifest.json");
    let (_, manifest, report) =
        read_coverage_documents(&inventory, &manifest).expect("checked-in coverage");
    let evidence = tempfile::tempdir().expect("matrix evidence");
    let observed = Mutex::new(Vec::new());

    // act: the bounded synthetic executor expands the matrix without launching a capture.
    let receipt =
        execute_matrix_bounded(manifest, report, "synthetic", evidence.path(), 4, |run| {
            observed
                .lock()
                .expect("observation lock")
                .push((run.row.row_id, run.trial));
            Ok((true, true, "passed".to_owned()))
        })
        .expect("synthetic matrix");

    // assert: every row has trials 1 through 5 exactly once.
    let observed = observed.into_inner().expect("observations");
    let unique = observed.iter().cloned().collect::<BTreeSet<_>>();
    assert_eq!(observed.len(), 547 * 5);
    assert_eq!(unique.len(), 547 * 5);
    assert_eq!(receipt.report.execution_count, 547 * 5);
    assert!(receipt.rows.iter().all(|row| {
        row.executions
            .iter()
            .map(|execution| execution.trial)
            .eq(1..=5)
    }));
}

#[test]
fn matrix_failure_records_failed_trial_without_writing_completion() {
    // arrange: one five-trial row whose third capture fails.
    let inventory_json = inventory_json(&["req-a"]);
    let manifest_json = manifest_json(&[("row-a", "req-a")]);
    let manifest: CoverageManifest = serde_json::from_str(&manifest_json).expect("manifest");
    let report =
        validate_coverage_documents(&inventory_json, &manifest_json).expect("coverage fixture");
    let evidence = tempfile::tempdir().expect("matrix evidence");

    // act: execution reaches the failing trial.
    let error = execute_matrix(manifest, report, "synthetic", evidence.path(), |run| {
        Ok((
            run.trial != 3,
            run.trial != 3,
            format!("trial {}", run.trial),
        ))
    })
    .expect_err("failed matrix cannot return success");

    // assert: a failed receipt exists, but no completion receipt is emitted.
    assert!(error.to_string().contains("must both pass"));
    let receipt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(evidence.path().join("matrix-receipt.json")).expect("failed receipt"),
    )
    .expect("failed receipt JSON");
    assert_eq!(receipt["status"], "failed");
    assert_eq!(
        receipt["rows"][0]["executions"][2]["capture_succeeded"],
        false
    );
    assert!(!evidence.path().join("matrix-completion.json").exists());
}

#[test]
fn coverage_validation_rejects_non_five_trial_rows_and_empty_dimensions() {
    // arrange: a row that cannot satisfy the complete matrix contract.
    let inventory = inventory_json(&["req-a"]);
    let mut manifest: serde_json::Value =
        serde_json::from_str(&manifest_json(&[("row-a", "req-a")])).expect("manifest fixture");
    manifest["rows"][0]["trials"] = 4.into();
    manifest["rows"][0]["persona"] = "".into();

    // act: coverage is validated.
    let error = validate_coverage_documents(&inventory, &manifest.to_string())
        .expect_err("incomplete row must fail");

    // assert: both the trial and dimension defects are named.
    assert!(error.to_string().contains("exactly 5 trials"));
    assert!(error.to_string().contains("empty coverage dimension"));
}

#[test]
fn active_coverage_resolves_registered_scenarios_without_promoting_inactive_families() {
    // arrange: the active coverage manifest and the complete baseline registry.
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest: CoverageManifest = serde_json::from_slice(
        &std::fs::read(repo_root.join("configs/tui-fidelity-coverage-manifest.json"))
            .expect("coverage manifest"),
    )
    .expect("coverage manifest JSON");
    let registry = std::fs::read_to_string(
        repo_root.join("crates/harness-testkit/src/tui_fidelity_scenarios/baseline/registry.json"),
    )
    .expect("scenario registry");

    // act: scenario and viewport reachability is validated.
    let report = validate_scenario_registry(&registry, &manifest).expect("registered coverage");

    // assert: canary, packet, cancel, and fail families stay explicitly non-acceptance.
    assert_eq!(report.active_families.len(), 20);
    assert_eq!(
        report.registered_non_acceptance_families,
        [
            "baseline-cancel",
            "baseline-fail",
            "canary-resize-wait",
            "canary-terminal-query",
            "canary-terminal-tier",
            "packet6-composer",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    );
}

#[test]
fn active_coverage_rejects_missing_scenario_family() {
    // arrange: one active row that names no registered scenario family.
    let manifest_json =
        manifest_json_with_scenarios(&[("row-a", "req-a", "missing--default-80x24")]);
    let manifest: CoverageManifest = serde_json::from_str(&manifest_json).expect("manifest");
    let registry = serde_json::json!({
        "schema_version": "harness.tui-fidelity.baseline-registry.v1",
        "viewports": [{"id": "default-80x24", "cols": 80, "rows": 24}],
        "scenarios": [
            {"id": "baseline-cancel", "path": "cancel", "state": "cancel", "owner_source_paths": ["owner"]},
            {"id": "baseline-fail", "path": "fail", "state": "fail", "owner_source_paths": ["owner"]},
            {"id": "canary-resize-wait", "path": "resize", "state": "resize", "owner_source_paths": ["owner"]},
            {"id": "canary-terminal-query", "path": "query", "state": "query", "owner_source_paths": ["owner"]},
            {"id": "canary-terminal-tier", "path": "tier", "state": "tier", "owner_source_paths": ["owner"]},
            {"id": "packet6-composer", "path": "packet", "state": "packet", "owner_source_paths": ["owner"]}
        ]
    });

    // act: registry reachability is checked.
    let error = validate_scenario_registry(&registry.to_string(), &manifest)
        .expect_err("missing scenario must fail");

    // assert: the missing family is diagnostic.
    assert!(error
        .to_string()
        .contains("missing scenario family missing"));
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
