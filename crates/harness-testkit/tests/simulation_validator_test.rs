use harness_testkit::simulation::{
    behavior_delta, compare_normalized_summaries, scan_simulation_artifact_root, summary_text,
    validate_artifact_index, validate_matrix_value, validate_report, validate_simulation_events,
    RedactionSummary, SummaryInput, ARTIFACT_INDEX_SCHEMA_VERSION,
};
use serde_json::json;
use std::fs;

mod support;

use support::simulation_validator::{
    assert_control, assert_delta_status, matrix, valid_event_rows, valid_matrix, valid_report,
};

#[test]
fn matrix_validator_accepts_checked_in_shape() {
    // arrange
    let value = valid_matrix();

    // act
    let matrix = validate_matrix_value(&value, "valid-matrix").expect("valid matrix");

    // assert
    assert_eq!(matrix.scenario.scenario_id, "golden_path");
}

#[test]
fn matrix_validator_rejects_schema_version_drift() {
    // arrange
    let mut value = valid_matrix();
    value["schema_version"] = json!("simulation-matrix-v0");

    // act
    let result = validate_matrix_value(&value, "matrix.json");

    // assert
    assert_control(result, "matrix-drift");
}

#[test]
fn unknown_invariant_id_fails() {
    // arrange
    let mut value = valid_matrix();
    value["scenarios"][0]["invariant_ids"] = json!(["INV-999"]);

    // act
    let result = validate_matrix_value(&value, "matrix.json");

    // assert
    assert_control(result, "unknown-invariant-id");
}

#[test]
fn invalid_schema_row_fails() {
    // arrange
    let mut value = valid_matrix();
    value["scenarios"][0]
        .as_object_mut()
        .expect("object")
        .remove("replay_command");

    // act
    let result = validate_matrix_value(&value, "matrix.json");

    // assert
    assert_control(result, "invalid-schema-row");
}

#[test]
fn duplicate_scenario_id_fails() {
    // arrange
    let mut value = valid_matrix();
    let duplicate = value["scenarios"][0].clone();
    value["scenarios"]
        .as_array_mut()
        .expect("array")
        .push(duplicate);

    // act
    let result = validate_matrix_value(&value, "matrix.json");

    // assert
    assert_control(result, "duplicate-scenario-id");
}

#[test]
fn signoff_row_claiming_behavioral_ownership_fails() {
    // arrange
    let mut value = valid_matrix();
    value["scenarios"][0]["determinism_class"] = json!("pty-signoff");

    // act
    let result = validate_matrix_value(&value, "matrix.json");

    // assert
    assert_control(result, "signoff-row-claiming-behavioral-ownership");
}

#[test]
fn missing_expected_artifact_fails() {
    // arrange
    let mut value = valid_matrix();
    value["scenarios"][0]["expected_artifacts"] = json!(["simulation-events.jsonl"]);

    // act
    let result = validate_matrix_value(&value, "matrix.json");

    // assert
    assert_control(result, "missing-expected-artifact");
}

#[test]
fn simulation_events_accept_valid_rows() {
    // arrange
    let matrix = matrix();
    let rows = valid_event_rows();

    // act
    let result = validate_simulation_events(&matrix, &rows, "simulation-events.jsonl");

    // assert
    result.expect("valid event rows");
}

#[test]
fn non_monotonic_jsonl_sequence_fails() {
    // arrange
    let mut rows = valid_event_rows();
    rows[1]["seq"] = json!(3);
    let matrix = matrix();

    // act
    let result = validate_simulation_events(&matrix, &rows, "simulation-events.jsonl");

    // assert
    assert_control(result, "non-monotonic-jsonl-sequence");
}

#[test]
fn missing_actor_identity_fails() {
    // arrange
    let mut rows = valid_event_rows();
    rows[0].as_object_mut().expect("object").remove("actor");
    let matrix = matrix();

    // act
    let result = validate_simulation_events(&matrix, &rows, "simulation-events.jsonl");

    // assert
    assert_control(result, "missing-actor-identity");
}

#[test]
fn missing_component_identity_fails() {
    // arrange
    let mut rows = valid_event_rows();
    rows[0].as_object_mut().expect("object").remove("component");
    let matrix = matrix();

    // act
    let result = validate_simulation_events(&matrix, &rows, "simulation-events.jsonl");

    // assert
    assert_control(result, "missing-component-identity");
}

#[test]
fn unknown_scenario_id_in_event_row_fails() {
    // arrange
    let mut rows = valid_event_rows();
    rows[0]["scenario_id"] = json!("unknown");
    let matrix = matrix();

    // act
    let result = validate_simulation_events(&matrix, &rows, "simulation-events.jsonl");

    // assert
    assert_control(result, "unknown-scenario-id-in-event-row");
}

#[test]
fn unknown_invariant_id_in_event_row_fails() {
    // arrange
    let mut rows = valid_event_rows();
    rows[0]["invariant_ids"] = json!(["INV-999"]);
    let matrix = matrix();

    // act
    let result = validate_simulation_events(&matrix, &rows, "simulation-events.jsonl");

    // assert
    assert_control(result, "unknown-invariant-id");
}

#[test]
fn missing_invariant_ids_in_event_row_fails() {
    // arrange
    let mut rows = valid_event_rows();
    rows[0]
        .as_object_mut()
        .expect("event object")
        .remove("invariant_ids");
    let matrix = matrix();

    // act
    let result = validate_simulation_events(&matrix, &rows, "simulation-events.jsonl");

    // assert
    assert_control(result, "event-schema");
}

#[test]
fn missing_replay_command_fingerprint_fails() {
    // arrange
    let mut rows = valid_event_rows();
    rows[0]["replay_command_fingerprint"] = json!("");
    let matrix = matrix();

    // act
    let result = validate_simulation_events(&matrix, &rows, "simulation-events.jsonl");

    // assert
    assert_control(result, "replay");
}

#[test]
fn malformed_event_redaction_metadata_fails() {
    // arrange
    let mut rows = valid_event_rows();
    rows[0]["redaction"] = json!({"status": "clean"});
    let matrix = matrix();

    // act
    let result = validate_simulation_events(&matrix, &rows, "simulation-events.jsonl");

    // assert
    assert_control(result, "redaction");
}

#[test]
fn malformed_report_run_shape_fails() {
    // arrange
    let mut report = valid_report();
    report["run"] = json!({"seed": "0", "run_fingerprint": "run-fingerprint"});
    let matrix = matrix();

    // act
    let result = validate_report(&matrix, &report, "simulation-report.json");

    // assert
    assert_control(result, "event-schema");
}

#[test]
fn malformed_report_replay_command_shape_fails() {
    // arrange
    let mut report = valid_report();
    report["replay_commands"][0]
        .as_object_mut()
        .expect("replay command object")
        .remove("fingerprint");
    let matrix = matrix();

    // act
    let result = validate_report(&matrix, &report, "simulation-report.json");

    // assert
    assert_control(result, "event-schema");
}

#[test]
fn malformed_report_redaction_summary_shape_fails() {
    // arrange
    let mut report = valid_report();
    report["redaction_summary"] = json!({"scanner": "harness-testkit-secret-scanner"});
    let matrix = matrix();

    // act
    let result = validate_report(&matrix, &report, "simulation-report.json");

    // assert
    assert_control(result, "event-schema");
}

#[test]
fn behavior_delta_classifies_predicate_changes() {
    // arrange
    let mut matrix = matrix();
    matrix.scenario.expected_predicates = json!({
        "same": 1,
        "removed": true,
        "changed": "old",
    });
    let normalized = json!({
        "same": 1,
        "changed": "new",
        "added": ["predicate"],
    });

    // act
    let deltas = behavior_delta(&matrix, &normalized);

    // assert
    assert_delta_status(&deltas, "added-predicate", "added");
    assert_delta_status(&deltas, "removed-predicate", "removed");
    assert_delta_status(&deltas, "changed-predicate", "changed");
}

#[test]
fn summary_text_reports_non_empty_behavior_delta() {
    // arrange
    let mut matrix = matrix();
    matrix.scenario.expected_predicates = json!({"expected": true});
    let normalized = json!({"seed": "0", "expected": false});
    let invariants = vec![json!({"invariant_id": "INV-001", "status": "pass"})];
    let redaction_summary = RedactionSummary::clean();

    // act
    let summary = summary_text(SummaryInput {
        matrix: &matrix,
        normalized: &normalized,
        run_fingerprint: "run-fingerprint",
        invariant_results: &invariants,
        redaction_summary: &redaction_summary,
        same_seed_status: "pass",
        artifact_index_path: "artifact-index.jsonl",
    });

    // assert
    assert!(
        summary.contains("behavior_delta=changed-predicate:changed"),
        "summary should report behavior delta: {summary}"
    );
}

#[test]
fn same_seed_normalized_summary_mismatch_fails_with_path() {
    // arrange
    let left = json!({"scenario_id": "golden_path", "replay": {"status": "finished"}});
    let right = json!({"scenario_id": "golden_path", "replay": {"status": "failed"}});

    // act
    let failures = compare_normalized_summaries(&left, &right).expect_err("mismatch fails");

    // assert
    assert_eq!(failures[0].control, "same-seed-normalized-summary-mismatch");
    assert_eq!(failures[0].path, "$.replay.status");
    assert_eq!(failures[0].expected, "\"finished\"");
    assert_eq!(failures[0].observed, "\"failed\"");
}

#[test]
fn artifact_index_rejects_fingerprint_mismatch() {
    // arrange
    let temp = tempfile::tempdir().expect("tempdir");
    for artifact in [
        "simulation-matrix.json",
        "simulation-events.jsonl",
        "simulation-report.json",
        "artifact-index.jsonl",
        "simulation-summary.txt",
        "normalized-summary-baseline.json",
        "normalized-summary-repeat.json",
        "same-seed-comparison.txt",
    ] {
        fs::write(temp.path().join(artifact), artifact).expect("write artifact");
    }
    let rows = [
        "simulation-matrix.json",
        "simulation-events.jsonl",
        "simulation-report.json",
        "artifact-index.jsonl",
        "simulation-summary.txt",
        "normalized-summary-baseline.json",
        "normalized-summary-repeat.json",
        "same-seed-comparison.txt",
    ]
    .into_iter()
    .map(|path| {
        json!({
            "schema_version": ARTIFACT_INDEX_SCHEMA_VERSION,
            "scenario_id": "golden_path",
            "artifact_kind": "evidence",
            "path": path,
            "redaction_status": "clean",
            "producer": "test",
            "fingerprint": "wrong-fingerprint"
        })
    })
    .collect::<Vec<_>>();
    let matrix = matrix();

    // act
    let result = validate_artifact_index(&matrix, temp.path(), &rows, "artifact-index.jsonl");

    // assert
    assert_control(result, "artifact-missing");
}

#[test]
fn artifact_index_requires_relative_existing_paths() {
    // arrange
    let temp = tempfile::tempdir().expect("tempdir");
    fs::write(temp.path().join("simulation-events.jsonl"), "{}").expect("write artifact");
    let rows = vec![json!({
        "schema_version": ARTIFACT_INDEX_SCHEMA_VERSION,
        "scenario_id": "golden_path",
        "artifact_kind": "events",
        "path": "simulation-events.jsonl",
        "redaction_status": "clean",
        "producer": "test",
        "fingerprint": "fp"
    })];
    let matrix = matrix();

    // act
    let result = validate_artifact_index(&matrix, temp.path(), &rows, "artifact-index.jsonl");

    // assert
    assert_control(result, "missing-expected-artifact");
}

#[test]
fn secret_bearing_artifact_is_rejected_by_scanner() {
    // arrange
    let temp = tempfile::tempdir().expect("tempdir");
    fs::write(
        temp.path().join("simulation-report.json"),
        r#"{"token":"sk-negativecontrol12345"}"#,
    )
    .expect("write secret artifact");

    // act
    let summary = scan_simulation_artifact_root(temp.path()).expect("scan root");

    // assert
    assert_eq!(summary.secret_finding_count, 1);
    assert_eq!(summary.rejected_artifact_count, 1);
}

#[test]
fn scanner_counts_generated_redacted_fields() {
    // arrange
    let temp = tempfile::tempdir().expect("tempdir");
    fs::write(
        temp.path().join("simulation-events.jsonl"),
        r#"{"redaction":{"redacted_fields":["workspace_root","summary"]}}
{"redaction":{"redacted_fields":[]}}
{"redaction":{"redacted_fields":["session_path"]}}
"#,
    )
    .expect("write events");

    // act
    let summary = scan_simulation_artifact_root(temp.path()).expect("scan root");

    // assert
    assert_eq!(summary.redacted_field_count, 3);
}
