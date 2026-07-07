use harness_testkit::simulation::{
    build_report, validate_matrix_value, RedactionSummary, ReportInput, SimulationFailure,
    SimulationMatrix, ARTIFACT_INDEX_SCHEMA_VERSION, EVENT_SCHEMA_VERSION, MATRIX_SCHEMA_VERSION,
    NORMALIZATION_PROFILE, REPORT_SCHEMA_VERSION,
};
use harness_testkit::UnwrapOrAbort;
use serde_json::{json, Value};

pub fn valid_matrix() -> Value {
    json!({
        "schema_version": MATRIX_SCHEMA_VERSION,
        "invariants": [
            {"invariant_id": "INV-001", "description": "event vocabulary", "behavioral": true},
            {"invariant_id": "INV-002", "description": "tool lifecycle", "behavioral": true},
            {"invariant_id": "INV-003", "description": "replay projection", "behavioral": true},
            {"invariant_id": "INV-004", "description": "redaction and stability", "behavioral": true}
        ],
        "scenarios": [{
            "scenario_id": "golden_path",
            "description": "valid matrix fixture",
            "determinism_class": "offline-deterministic",
            "invariant_ids": ["INV-001", "INV-002", "INV-003", "INV-004"],
            "owner_tests_or_lanes": ["scripts/test-lanes.sh simulation"],
            "replay_command": "harness replay --session <simulation-run-dir> --json",
            "expected_artifacts": [
                "simulation-matrix.json",
                "simulation-events.jsonl",
                "simulation-report.json",
                "artifact-index.jsonl",
                "simulation-summary.txt",
                "normalized-summary-baseline.json",
                "normalized-summary-repeat.json",
                "same-seed-comparison.txt"
            ],
            "seed_policy": "fixed deterministic seed 0 recorded in artifacts",
            "artifact_schema_versions": {
                "matrix": MATRIX_SCHEMA_VERSION,
                "event": EVENT_SCHEMA_VERSION,
                "report": REPORT_SCHEMA_VERSION,
                "artifact_index": ARTIFACT_INDEX_SCHEMA_VERSION,
                "normalization": NORMALIZATION_PROFILE
            },
            "negative_controls": ["unknown-invariant-id-fails"],
            "live_policy": "not-applicable",
            "expected_predicates": {}
        }]
    })
}

pub fn matrix() -> SimulationMatrix {
    validate_matrix_value(&valid_matrix(), "valid-matrix").unwrap_or_abort()
}

pub fn valid_event_rows() -> Vec<Value> {
    vec![
        json!({
            "schema_version": EVENT_SCHEMA_VERSION,
            "seq": 1,
            "scenario_id": "golden_path",
            "seed": "0",
            "run_id": "run_1",
            "run_fingerprint": "fingerprint",
            "actor": "coordinator",
            "component": "harness-core",
            "event_kind": "run_started",
            "invariant_ids": ["INV-001"],
            "redaction": {"status": "clean", "redacted_fields": [], "scanner": "harness-testkit-secret-scanner"},
            "replay_command_fingerprint": "replay-fp",
            "payload": {}
        }),
        json!({
            "schema_version": EVENT_SCHEMA_VERSION,
            "seq": 2,
            "scenario_id": "golden_path",
            "seed": "0",
            "run_id": "run_1",
            "run_fingerprint": "fingerprint",
            "actor": "replay",
            "component": "harness",
            "event_kind": "replay_projection_validated",
            "invariant_ids": ["INV-003"],
            "redaction": {"status": "clean", "redacted_fields": [], "scanner": "harness-testkit-secret-scanner"},
            "replay_command_fingerprint": "replay-fp",
            "payload": {}
        }),
    ]
}

pub fn valid_report() -> Value {
    let matrix = matrix();
    let normalized = json!({
        "seed": "0",
        "tool_artifacts": [],
        "tool_lifecycle": {},
        "permission_lifecycle": {},
        "edit_artifact_links": {},
        "replay": {},
    });
    let invariants = vec![json!({
        "invariant_id": "INV-001",
        "status": "pass",
        "failure_signal": "invariant",
        "message": "ok",
        "expected": {},
        "observed": {},
    })];
    let redaction_summary = RedactionSummary {
        scanner: "harness-testkit-secret-scanner".to_owned(),
        scanned_artifact_count: 8,
        clean_artifact_count: 8,
        redacted_field_count: 2,
        rejected_artifact_count: 0,
        secret_finding_count: 0,
        rejected_artifacts: Vec::new(),
    };

    build_report(ReportInput {
        matrix: &matrix,
        normalized: &normalized,
        run_fingerprint: "run-fingerprint",
        invariant_results: &invariants,
        behavior_delta: &[],
        artifact_index: &[],
        redaction_summary: &redaction_summary,
        same_seed_status: "pass",
        raw_evidence_paths: vec!["raw-evidence/baseline/replay.json".to_owned()],
    })
}

#[allow(clippy::panic, reason = "test support code must panic gracefully")]
pub fn assert_control<T: std::fmt::Debug>(
    result: Result<T, Vec<SimulationFailure>>,
    control: &str,
) {
    let Err(failures) = result else {
        panic!("negative control should fail");
    };
    assert!(
        failures.iter().any(|failure| failure.control == control),
        "expected control `{control}` in failures: {failures:#?}"
    );
    let first = failures
        .iter()
        .find(|failure| failure.control == control)
        .unwrap_or_abort();
    assert!(!first.path.is_empty());
    assert!(!first.expected.is_empty());
    assert!(!first.observed.is_empty());
}

#[allow(clippy::panic, reason = "test support code must panic gracefully")]
pub fn assert_delta_status(deltas: &[Value], kind: &str, status: &str) {
    let row = deltas
        .iter()
        .find(|row| row.get("kind").and_then(Value::as_str) == Some(kind))
        .unwrap_or_else(|| panic!("missing delta row `{kind}` in {deltas:#?}"));
    assert_eq!(row.get("status").and_then(Value::as_str), Some(status));
    assert!(
        row.get("details")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty()),
        "changed delta row should include details: {row:#?}"
    );
}
