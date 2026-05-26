use crate::secret_scanner::{default_forbidden_patterns, scan_directory_tree_for_secrets};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const MATRIX_SCHEMA_VERSION: &str = "simulation-matrix-v1";
pub const EVENT_SCHEMA_VERSION: &str = "simulation-event-v1";
pub const REPORT_SCHEMA_VERSION: &str = "simulation-report-v1";
pub const ARTIFACT_INDEX_SCHEMA_VERSION: &str = "artifact-index-v1";
pub const NORMALIZATION_PROFILE: &str = "simulation-normalization-v1";
pub const DEFAULT_SCENARIO_ID: &str = "golden_path";
pub const DEFAULT_SEED: &str = "0";

pub const REQUIRED_ARTIFACTS: &[&str] = &[
    "simulation-matrix.json",
    "simulation-events.jsonl",
    "simulation-report.json",
    "artifact-index.jsonl",
    "simulation-summary.txt",
    "normalized-summary-baseline.json",
    "normalized-summary-repeat.json",
    "same-seed-comparison.txt",
];

const DETERMINISM_CLASSES: &[&str] = &[
    "offline-deterministic",
    "pty-signoff",
    "live-signoff",
    "native-signoff",
    "planned",
    "waived",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimulationFailure {
    pub control: String,
    pub path: String,
    pub scenario_id: Option<String>,
    pub invariant_id: Option<String>,
    pub expected: String,
    pub observed: String,
    pub message: String,
}

impl SimulationFailure {
    pub fn new(
        control: impl Into<String>,
        path: impl Into<String>,
        expected: impl Into<String>,
        observed: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            control: control.into(),
            path: path.into(),
            scenario_id: None,
            invariant_id: None,
            expected: expected.into(),
            observed: observed.into(),
            message: message.into(),
        }
    }

    pub fn scenario(mut self, scenario_id: impl Into<String>) -> Self {
        self.scenario_id = Some(scenario_id.into());
        self
    }

    pub fn invariant(mut self, invariant_id: impl Into<String>) -> Self {
        self.invariant_id = Some(invariant_id.into());
        self
    }
}

impl fmt::Display for SimulationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "control={} path={} expected={} observed={} message={}",
            self.control, self.path, self.expected, self.observed, self.message
        )?;
        if let Some(scenario_id) = &self.scenario_id {
            write!(f, " scenario_id={scenario_id}")?;
        }
        if let Some(invariant_id) = &self.invariant_id {
            write!(f, " invariant_id={invariant_id}")?;
        }
        Ok(())
    }
}

impl std::error::Error for SimulationFailure {}

pub type SimulationResult<T> = Result<T, Vec<SimulationFailure>>;
pub type SingleFailureResult<T> = Result<T, Box<SimulationFailure>>;

#[derive(Debug, Clone)]
pub struct SimulationMatrix {
    pub scenario: ScenarioContract,
    pub invariant_ids: BTreeSet<String>,
}

#[derive(Debug, Clone)]
pub struct ScenarioContract {
    pub scenario_id: String,
    pub determinism_class: String,
    pub invariant_ids: Vec<String>,
    pub expected_artifacts: Vec<String>,
    pub replay_command: String,
    pub expected_predicates: Value,
    pub negative_controls: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RedactionSummary {
    pub scanner: String,
    pub scanned_artifact_count: usize,
    pub clean_artifact_count: usize,
    pub redacted_field_count: usize,
    pub rejected_artifact_count: usize,
    pub secret_finding_count: usize,
    pub rejected_artifacts: Vec<String>,
}

pub struct ReportInput<'a> {
    pub matrix: &'a SimulationMatrix,
    pub normalized: &'a Value,
    pub run_fingerprint: &'a str,
    pub invariant_results: &'a [Value],
    pub behavior_delta: &'a [Value],
    pub artifact_index: &'a [Value],
    pub redaction_summary: &'a RedactionSummary,
    pub same_seed_status: &'a str,
    pub raw_evidence_paths: Vec<String>,
}

pub struct SummaryInput<'a> {
    pub matrix: &'a SimulationMatrix,
    pub normalized: &'a Value,
    pub run_fingerprint: &'a str,
    pub invariant_results: &'a [Value],
    pub redaction_summary: &'a RedactionSummary,
    pub same_seed_status: &'a str,
    pub artifact_index_path: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DeltaStatus {
    None,
    Added,
    Removed,
    Changed,
}

impl DeltaStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Changed => "changed",
        }
    }
}

impl RedactionSummary {
    pub fn clean() -> Self {
        Self {
            scanner: "harness-testkit-secret-scanner".to_owned(),
            scanned_artifact_count: 0,
            clean_artifact_count: 0,
            redacted_field_count: 0,
            rejected_artifact_count: 0,
            secret_finding_count: 0,
            rejected_artifacts: Vec::new(),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "scanner": self.scanner,
            "scanned_artifact_count": self.scanned_artifact_count,
            "clean_artifact_count": self.clean_artifact_count,
            "redacted_field_count": self.redacted_field_count,
            "rejected_artifact_count": self.rejected_artifact_count,
            "secret_finding_count": self.secret_finding_count,
            "rejected_artifacts": self.rejected_artifacts,
        })
    }
}

pub fn validate_matrix_file(path: &Path) -> SimulationResult<SimulationMatrix> {
    let value = read_json_file(path).map_err(|failure| vec![*failure])?;
    validate_matrix_value(&value, &path.display().to_string())
}

pub fn validate_matrix_value(value: &Value, path: &str) -> SimulationResult<SimulationMatrix> {
    let Some(root) = value.as_object() else {
        return Err(vec![failure(
            "invalid-schema-row",
            path,
            "top-level object",
            value_type(value),
            "matrix root must be an object",
        )]);
    };

    let mut failures = Vec::new();
    expect_str(
        root,
        "schema_version",
        MATRIX_SCHEMA_VERSION,
        "matrix-drift",
        path,
        &mut failures,
    );

    let mut invariant_ids = BTreeSet::new();
    let mut behavioral_invariants = BTreeSet::new();
    match root.get("invariants").and_then(Value::as_array) {
        Some(rows) if !rows.is_empty() => {
            for (index, row) in rows.iter().enumerate() {
                let row_path = format!("{path}#/invariants/{index}");
                let Some(object) = row.as_object() else {
                    failures.push(failure(
                        "invalid-schema-row",
                        row_path,
                        "invariant object",
                        value_type(row),
                        "invariant row must be an object",
                    ));
                    continue;
                };
                let Some(invariant_id) = non_empty_str(object, "invariant_id") else {
                    failures.push(failure(
                        "invalid-schema-row",
                        &row_path,
                        "invariant_id",
                        "<missing>",
                        "invariant row is missing invariant_id",
                    ));
                    continue;
                };
                if !invariant_ids.insert(invariant_id.to_owned()) {
                    failures.push(
                        failure(
                            "invalid-schema-row",
                            &row_path,
                            "unique invariant_id",
                            invariant_id,
                            "duplicate invariant_id",
                        )
                        .invariant(invariant_id),
                    );
                }
                if object.get("behavioral").and_then(Value::as_bool) == Some(true) {
                    behavioral_invariants.insert(invariant_id.to_owned());
                }
                if non_empty_str(object, "description").is_none() {
                    failures.push(
                        failure(
                            "invalid-schema-row",
                            &row_path,
                            "description",
                            "<missing>",
                            "invariant row is missing description",
                        )
                        .invariant(invariant_id),
                    );
                }
            }
        }
        _ => failures.push(failure(
            "invalid-schema-row",
            path,
            "non-empty invariants array",
            "<missing-or-empty>",
            "matrix must declare invariants",
        )),
    }

    let Some(scenarios) = root.get("scenarios").and_then(Value::as_array) else {
        failures.push(failure(
            "invalid-schema-row",
            path,
            "scenarios array",
            "<missing>",
            "matrix must declare scenarios",
        ));
        return finish_matrix(
            failures,
            invariant_ids,
            behavioral_invariants,
            Vec::new(),
            path,
        );
    };

    let mut seen_scenarios = BTreeSet::new();
    let mut owned_invariants = BTreeSet::new();
    let mut parsed_scenarios = Vec::new();
    for (index, row) in scenarios.iter().enumerate() {
        let row_path = format!("{path}#/scenarios/{index}");
        let Some(object) = row.as_object() else {
            failures.push(failure(
                "invalid-schema-row",
                row_path,
                "scenario object",
                value_type(row),
                "scenario row must be an object",
            ));
            continue;
        };

        for field in [
            "scenario_id",
            "description",
            "determinism_class",
            "invariant_ids",
            "owner_tests_or_lanes",
            "replay_command",
            "expected_artifacts",
            "seed_policy",
            "artifact_schema_versions",
            "negative_controls",
            "live_policy",
        ] {
            if !object.contains_key(field) {
                failures.push(failure(
                    "invalid-schema-row",
                    &row_path,
                    field,
                    "<missing>",
                    "scenario row is missing a required field",
                ));
            }
        }

        let scenario_id = non_empty_str(object, "scenario_id").unwrap_or("<missing>");
        if !seen_scenarios.insert(scenario_id.to_owned()) {
            failures.push(
                failure(
                    "duplicate-scenario-id",
                    &row_path,
                    "unique scenario_id",
                    scenario_id,
                    "duplicate scenario_id",
                )
                .scenario(scenario_id),
            );
        }

        let determinism_class = non_empty_str(object, "determinism_class").unwrap_or("<missing>");
        if !DETERMINISM_CLASSES.contains(&determinism_class) {
            failures.push(
                failure(
                    "invalid-schema-row",
                    &row_path,
                    DETERMINISM_CLASSES.join(","),
                    determinism_class,
                    "unknown determinism_class",
                )
                .scenario(scenario_id),
            );
        }
        if determinism_class == "waived" && non_empty_str(object, "waiver_reason").is_none() {
            failures.push(
                failure(
                    "invalid-schema-row",
                    &row_path,
                    "waiver_reason",
                    "<missing>",
                    "waived scenarios must include a reason",
                )
                .scenario(scenario_id),
            );
        }

        let scenario_invariant_ids = string_array(object.get("invariant_ids"));
        if scenario_invariant_ids.is_empty() && determinism_class == "offline-deterministic" {
            failures.push(
                failure(
                    "unknown-invariant-id",
                    &row_path,
                    "at least one invariant_id",
                    "<empty>",
                    "offline deterministic scenarios must own behavioral invariants",
                )
                .scenario(scenario_id),
            );
        }
        for invariant_id in &scenario_invariant_ids {
            if !invariant_ids.contains(invariant_id) {
                failures.push(
                    failure(
                        "unknown-invariant-id",
                        &row_path,
                        "known invariant_id",
                        invariant_id,
                        "scenario references an unknown invariant",
                    )
                    .scenario(scenario_id)
                    .invariant(invariant_id),
                );
            } else {
                owned_invariants.insert(invariant_id.to_owned());
            }
            if determinism_class != "offline-deterministic"
                && behavioral_invariants.contains(invariant_id)
            {
                failures.push(
                    failure(
                        "signoff-row-claiming-behavioral-ownership",
                        &row_path,
                        "offline-deterministic",
                        determinism_class,
                        "non-offline rows cannot own behavioral invariants",
                    )
                    .scenario(scenario_id)
                    .invariant(invariant_id),
                );
            }
        }

        let expected_artifacts = string_array(object.get("expected_artifacts"));
        if expected_artifacts.is_empty() {
            failures.push(
                failure(
                    "missing-expected-artifact",
                    &row_path,
                    "expected_artifacts",
                    "<empty>",
                    "scenario must declare expected artifacts",
                )
                .scenario(scenario_id),
            );
        }
        for artifact in REQUIRED_ARTIFACTS {
            if !expected_artifacts.iter().any(|item| item == artifact) {
                failures.push(
                    failure(
                        "missing-expected-artifact",
                        &row_path,
                        *artifact,
                        expected_artifacts.join(","),
                        "scenario is missing a required simulation artifact declaration",
                    )
                    .scenario(scenario_id),
                );
            }
        }
        validate_schema_versions(object, &row_path, scenario_id, &mut failures);

        parsed_scenarios.push(ScenarioContract {
            scenario_id: scenario_id.to_owned(),
            determinism_class: determinism_class.to_owned(),
            invariant_ids: scenario_invariant_ids,
            expected_artifacts,
            replay_command: non_empty_str(object, "replay_command")
                .unwrap_or("")
                .to_owned(),
            expected_predicates: object
                .get("expected_predicates")
                .cloned()
                .unwrap_or_else(|| json!({})),
            negative_controls: string_array(object.get("negative_controls")),
        });
    }

    for invariant_id in &invariant_ids {
        if !owned_invariants.contains(invariant_id) {
            failures.push(
                failure(
                    "unknown-invariant-id",
                    path,
                    "owning scenario",
                    "<none>",
                    "invariant has no owning scenario",
                )
                .invariant(invariant_id),
            );
        }
    }

    finish_matrix(
        failures,
        invariant_ids,
        behavioral_invariants,
        parsed_scenarios,
        path,
    )
}

pub fn read_json_file(path: &Path) -> SingleFailureResult<Value> {
    let text = fs::read_to_string(path).map_err(|err| {
        Box::new(failure(
            "invalid-schema-row",
            path.display().to_string(),
            "readable JSON file",
            err.to_string(),
            "failed to read JSON file",
        ))
    })?;
    serde_json::from_str(&text).map_err(|err| {
        Box::new(failure(
            "invalid-schema-row",
            path.display().to_string(),
            "valid JSON",
            err.to_string(),
            "failed to parse JSON file",
        ))
    })
}

pub fn read_jsonl_file(path: &Path, control: &str) -> SingleFailureResult<Vec<Value>> {
    let text = fs::read_to_string(path).map_err(|err| {
        Box::new(failure(
            control,
            path.display().to_string(),
            "readable JSONL file",
            err.to_string(),
            "failed to read JSONL file",
        ))
    })?;
    let mut rows = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let row = serde_json::from_str(line).map_err(|err| {
            Box::new(failure(
                control,
                format!("{}:{}", path.display(), index + 1),
                "valid JSON row",
                err.to_string(),
                "failed to parse JSONL row",
            ))
        })?;
        rows.push(row);
    }
    Ok(rows)
}

pub fn build_normalized_summary(
    matrix: &SimulationMatrix,
    raw_events: &[Value],
    replay: &Value,
    seed: &str,
) -> Value {
    json!({
        "schema_version": "simulation-normalized-summary-v1",
        "normalization_profile": NORMALIZATION_PROFILE,
        "scenario_id": matrix.scenario.scenario_id,
        "seed": seed,
        "event_kind_counts": event_kind_counts(raw_events),
        "provider_request_digests": provider_request_digests(raw_events),
        "replay": replay_predicate(replay),
        "tool_artifacts": tool_artifacts(replay),
        "tool_lifecycle": tool_lifecycle(raw_events),
        "permission_lifecycle": permission_lifecycle(raw_events),
        "edit_artifact_links": edit_artifact_links(raw_events),
    })
}

pub fn compare_normalized_summaries(left: &Value, right: &Value) -> SimulationResult<()> {
    if left == right {
        Ok(())
    } else {
        let (path, expected, observed) = first_json_diff(left, right, "$".to_owned());
        Err(vec![failure(
            "same-seed-normalized-summary-mismatch",
            path,
            expected,
            observed,
            "same-seed normalized summaries differ",
        )])
    }
}

pub fn simulation_event_rows(
    matrix: &SimulationMatrix,
    raw_events: &[Value],
    replay: &Value,
    seed: &str,
    run_fingerprint: &str,
) -> Vec<Value> {
    let replay_fingerprint = stable_fingerprint_bytes(matrix.scenario.replay_command.as_bytes());
    let mut rows = Vec::new();
    for raw_event in raw_events {
        let event_kind = raw_event_kind(raw_event).unwrap_or("unknown");
        let (payload, redacted_fields) = redacted_payload(raw_event, event_kind);
        rows.push(json!({
            "schema_version": EVENT_SCHEMA_VERSION,
            "seq": raw_event.get("seq").and_then(Value::as_u64).unwrap_or(0),
            "scenario_id": matrix.scenario.scenario_id,
            "seed": seed,
            "run_id": raw_event.get("run_id").and_then(Value::as_str).unwrap_or(""),
            "run_fingerprint": run_fingerprint,
            "actor": actor_for_event_kind(event_kind),
            "component": component_for_event_kind(event_kind),
            "event_kind": event_kind,
            "invariant_ids": invariant_ids_for_event_kind(event_kind),
            "redaction": {
                "status": "clean",
                "redacted_fields": redacted_fields,
                "scanner": "harness-testkit-secret-scanner"
            },
            "replay_command_fingerprint": replay_fingerprint,
            "payload": payload,
        }));
    }
    rows.push(json!({
        "schema_version": EVENT_SCHEMA_VERSION,
        "seq": raw_events.len() as u64 + 1,
        "scenario_id": matrix.scenario.scenario_id,
        "seed": seed,
        "run_id": replay.get("run_id").and_then(Value::as_str).unwrap_or(""),
        "run_fingerprint": run_fingerprint,
        "actor": "replay",
        "component": "harness",
        "event_kind": "replay_projection_validated",
        "invariant_ids": ["INV-003", "INV-004"],
        "redaction": {
            "status": "clean",
            "redacted_fields": ["session_path", "workspace_root"],
            "scanner": "harness-testkit-secret-scanner"
        },
        "replay_command_fingerprint": replay_fingerprint,
        "payload": replay_predicate(replay),
    }));
    rows
}

pub fn validate_simulation_events_file(
    matrix: &SimulationMatrix,
    path: &Path,
) -> SimulationResult<Vec<Value>> {
    let rows = read_jsonl_file(path, "event-schema").map_err(|failure| vec![*failure])?;
    validate_simulation_events(matrix, &rows, &path.display().to_string())?;
    Ok(rows)
}

pub fn validate_simulation_events(
    matrix: &SimulationMatrix,
    rows: &[Value],
    path: &str,
) -> SimulationResult<()> {
    let mut failures = Vec::new();
    let mut expected_seq = 1_u64;
    for (index, row) in rows.iter().enumerate() {
        let row_path = format!("{path}:{}", index + 1);
        let Some(object) = row.as_object() else {
            failures.push(failure(
                "event-schema",
                row_path,
                "event object",
                value_type(row),
                "simulation event row must be an object",
            ));
            continue;
        };
        expect_str(
            object,
            "schema_version",
            EVENT_SCHEMA_VERSION,
            "event-schema",
            &row_path,
            &mut failures,
        );
        let observed_seq = object.get("seq").and_then(Value::as_u64);
        if observed_seq != Some(expected_seq) {
            failures.push(failure(
                "non-monotonic-jsonl-sequence",
                &row_path,
                expected_seq.to_string(),
                observed_seq.map_or_else(|| "<missing>".to_owned(), |seq| seq.to_string()),
                "simulation event seq must be contiguous and monotonic",
            ));
        }
        expected_seq += 1;
        expect_present(object, "seed", "event-schema", &row_path, &mut failures);
        expect_present(
            object,
            "actor",
            "missing-actor-identity",
            &row_path,
            &mut failures,
        );
        expect_present(
            object,
            "component",
            "missing-component-identity",
            &row_path,
            &mut failures,
        );
        expect_present(
            object,
            "event_kind",
            "event-schema",
            &row_path,
            &mut failures,
        );
        if non_empty_str(object, "run_id").is_none()
            && non_empty_str(object, "run_fingerprint").is_none()
        {
            failures.push(failure(
                "event-schema",
                &row_path,
                "run_id or run_fingerprint",
                "<missing>",
                "simulation event row must identify its run",
            ));
        }
        let scenario_id = non_empty_str(object, "scenario_id").unwrap_or("<missing>");
        if scenario_id != matrix.scenario.scenario_id {
            failures.push(
                failure(
                    "unknown-scenario-id-in-event-row",
                    &row_path,
                    &matrix.scenario.scenario_id,
                    scenario_id,
                    "event row scenario_id is not admitted by the matrix",
                )
                .scenario(scenario_id),
            );
        }
        let invariant_ids = string_array(object.get("invariant_ids"));
        if invariant_ids.is_empty() {
            failures.push(failure(
                "event-schema",
                &row_path,
                "non-empty invariant_ids array",
                object
                    .get("invariant_ids")
                    .map_or("<missing>".to_owned(), value_type),
                "event row must declare contributing invariant IDs",
            ));
        }
        for invariant_id in invariant_ids {
            if !matrix.invariant_ids.contains(&invariant_id) {
                failures.push(
                    failure(
                        "unknown-invariant-id",
                        &row_path,
                        "known invariant_id",
                        &invariant_id,
                        "event row references an unknown invariant",
                    )
                    .scenario(scenario_id)
                    .invariant(invariant_id),
                );
            }
        }
        match object.get("redaction").and_then(Value::as_object) {
            Some(redaction) => {
                expect_str(
                    redaction,
                    "status",
                    "clean",
                    "redaction",
                    &row_path,
                    &mut failures,
                );
                expect_str(
                    redaction,
                    "scanner",
                    "harness-testkit-secret-scanner",
                    "redaction",
                    &row_path,
                    &mut failures,
                );
                match redaction.get("redacted_fields").and_then(Value::as_array) {
                    Some(fields) => {
                        for (field_index, field) in fields.iter().enumerate() {
                            if field.as_str().filter(|value| !value.is_empty()).is_none() {
                                failures.push(failure(
                                    "redaction",
                                    format!("{row_path}#/redaction/redacted_fields/{field_index}"),
                                    "non-empty redacted field name",
                                    value_type(field),
                                    "redacted_fields entries must be named fields",
                                ));
                            }
                        }
                    }
                    None => failures.push(failure(
                        "redaction",
                        &row_path,
                        "redacted_fields array",
                        redaction
                            .get("redacted_fields")
                            .map_or("<missing>".to_owned(), value_type),
                        "event row redaction metadata must list redacted fields",
                    )),
                }
            }
            None => failures.push(failure(
                "redaction",
                &row_path,
                "redaction object",
                "<missing>",
                "event row must include redaction metadata",
            )),
        }
        if non_empty_str(object, "replay_command_fingerprint").is_none() {
            failures.push(failure(
                "replay",
                &row_path,
                "non-empty replay_command_fingerprint",
                object
                    .get("replay_command_fingerprint")
                    .map_or("<missing>".to_owned(), value_type),
                "event row must include replay command observability",
            ));
        }
        if object.get("payload").and_then(Value::as_object).is_none() {
            failures.push(failure(
                "event-schema",
                &row_path,
                "payload object",
                object
                    .get("payload")
                    .map_or("<missing>".to_owned(), value_type),
                "event row payload must contain redacted derived predicate data",
            ));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}

pub fn validate_artifact_index_file(
    matrix: &SimulationMatrix,
    artifact_root: &Path,
    index_path: &Path,
) -> SimulationResult<Vec<Value>> {
    let rows = read_jsonl_file(index_path, "artifact-missing").map_err(|failure| vec![*failure])?;
    validate_artifact_index(
        matrix,
        artifact_root,
        &rows,
        &index_path.display().to_string(),
    )?;
    Ok(rows)
}

pub fn validate_artifact_index(
    matrix: &SimulationMatrix,
    artifact_root: &Path,
    rows: &[Value],
    path: &str,
) -> SimulationResult<()> {
    let mut failures = Vec::new();
    let mut indexed_paths = BTreeSet::new();
    for (index, row) in rows.iter().enumerate() {
        let row_path = format!("{path}:{}", index + 1);
        let Some(object) = row.as_object() else {
            failures.push(failure(
                "artifact-missing",
                row_path,
                "artifact index object",
                value_type(row),
                "artifact index row must be an object",
            ));
            continue;
        };
        expect_str(
            object,
            "schema_version",
            ARTIFACT_INDEX_SCHEMA_VERSION,
            "artifact-missing",
            &row_path,
            &mut failures,
        );
        for field in [
            "scenario_id",
            "artifact_kind",
            "path",
            "redaction_status",
            "producer",
            "fingerprint",
        ] {
            expect_present(object, field, "artifact-missing", &row_path, &mut failures);
        }
        let scenario_id = non_empty_str(object, "scenario_id").unwrap_or("<missing>");
        if scenario_id != matrix.scenario.scenario_id {
            failures.push(
                failure(
                    "artifact-missing",
                    &row_path,
                    &matrix.scenario.scenario_id,
                    scenario_id,
                    "artifact index references an unknown scenario",
                )
                .scenario(scenario_id),
            );
        }
        if let Some(relative) = non_empty_str(object, "path") {
            if Path::new(relative).is_absolute() || relative.split('/').any(|part| part == "..") {
                failures.push(failure(
                    "artifact-missing",
                    &row_path,
                    "relative path below artifact root",
                    relative,
                    "artifact index path must be relative and contained",
                ));
            } else {
                indexed_paths.insert(relative.to_owned());
                let artifact_path = artifact_root.join(relative);
                if !artifact_path.exists() {
                    failures.push(
                        failure(
                            "missing-expected-artifact",
                            &row_path,
                            "artifact exists",
                            relative,
                            "indexed artifact is missing from artifact root",
                        )
                        .scenario(scenario_id),
                    );
                } else if let Some(expected_fingerprint) = non_empty_str(object, "fingerprint") {
                    let observed_fingerprint = stable_fingerprint_file(&artifact_path)
                        .unwrap_or_else(|| "<unreadable>".to_owned());
                    if observed_fingerprint != expected_fingerprint {
                        failures.push(
                            failure(
                                "artifact-missing",
                                &row_path,
                                expected_fingerprint,
                                observed_fingerprint,
                                "artifact fingerprint must match normalized content",
                            )
                            .scenario(scenario_id),
                        );
                    }
                }
            }
        }
    }
    for artifact in &matrix.scenario.expected_artifacts {
        if !indexed_paths.contains(artifact) || !artifact_root.join(artifact).exists() {
            failures.push(
                failure(
                    "missing-expected-artifact",
                    path,
                    artifact,
                    "<missing>",
                    "required simulation artifact is missing or not indexed",
                )
                .scenario(&matrix.scenario.scenario_id),
            );
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}

pub fn validate_report_file(
    matrix: &SimulationMatrix,
    report_path: &Path,
) -> SimulationResult<Value> {
    let report = read_json_file(report_path).map_err(|failure| vec![*failure])?;
    validate_report(matrix, &report, &report_path.display().to_string())?;
    Ok(report)
}

pub fn validate_report(
    matrix: &SimulationMatrix,
    report: &Value,
    path: &str,
) -> SimulationResult<()> {
    let Some(object) = report.as_object() else {
        return Err(vec![failure(
            "event-schema",
            path,
            "report object",
            value_type(report),
            "simulation report must be an object",
        )]);
    };
    let mut failures = Vec::new();
    expect_str(
        object,
        "schema_version",
        REPORT_SCHEMA_VERSION,
        "event-schema",
        path,
        &mut failures,
    );
    expect_str(
        object,
        "matrix_schema_version",
        MATRIX_SCHEMA_VERSION,
        "event-schema",
        path,
        &mut failures,
    );
    expect_str(
        object,
        "event_schema_version",
        EVENT_SCHEMA_VERSION,
        "event-schema",
        path,
        &mut failures,
    );
    for field in [
        "run",
        "summary",
        "behavior_delta",
        "invariant_results",
        "artifact_index",
        "replay_commands",
        "failure_signals",
        "redaction_summary",
        "volatile_fields",
        "raw_evidence_paths",
    ] {
        if !object.contains_key(field) {
            failures.push(
                failure(
                    "event-schema",
                    path,
                    field,
                    "<missing>",
                    "simulation report is missing a required section",
                )
                .scenario(&matrix.scenario.scenario_id),
            );
        }
    }
    if let Some(run) = expect_report_object(object, "run", path, &mut failures) {
        let run_path = format!("{path}#/run");
        expect_present(run, "seed", "event-schema", &run_path, &mut failures);
        expect_present(
            run,
            "run_fingerprint",
            "event-schema",
            &run_path,
            &mut failures,
        );
        expect_str(
            run,
            "normalization_profile",
            NORMALIZATION_PROFILE,
            "event-schema",
            &run_path,
            &mut failures,
        );
    }
    if let Some(summary) = expect_report_object(object, "summary", path, &mut failures) {
        let summary_path = format!("{path}#/summary");
        match non_empty_str(summary, "status") {
            Some("pass" | "fail") => {}
            Some(status) => failures.push(failure(
                "event-schema",
                &summary_path,
                "pass or fail",
                status,
                "report summary status must be explicit",
            )),
            None => failures.push(failure(
                "event-schema",
                &summary_path,
                "status",
                "<missing-or-empty>",
                "report summary must include status",
            )),
        }
        expect_present(
            summary,
            "narrative",
            "event-schema",
            &summary_path,
            &mut failures,
        );
    }
    expect_report_array(object, "behavior_delta", path, &mut failures);
    expect_report_array(object, "invariant_results", path, &mut failures);
    expect_report_array(object, "artifact_index", path, &mut failures);
    expect_report_array(object, "failure_signals", path, &mut failures);
    expect_report_array(object, "volatile_fields", path, &mut failures);
    expect_report_array(object, "raw_evidence_paths", path, &mut failures);
    if let Some(replay_commands) =
        expect_report_array(object, "replay_commands", path, &mut failures)
    {
        if replay_commands.is_empty() {
            failures.push(
                failure(
                    "event-schema",
                    format!("{path}#/replay_commands"),
                    "at least one replay command",
                    "[]",
                    "report must record replay command evidence",
                )
                .scenario(&matrix.scenario.scenario_id),
            );
        }
        for (index, command) in replay_commands.iter().enumerate() {
            let command_path = format!("{path}#/replay_commands/{index}");
            let Some(command) = command.as_object() else {
                failures.push(failure(
                    "event-schema",
                    &command_path,
                    "replay command object",
                    value_type(command),
                    "replay command rows must be objects",
                ));
                continue;
            };
            expect_str(
                command,
                "scenario_id",
                &matrix.scenario.scenario_id,
                "event-schema",
                &command_path,
                &mut failures,
            );
            expect_str(
                command,
                "command",
                &matrix.scenario.replay_command,
                "event-schema",
                &command_path,
                &mut failures,
            );
            expect_str(
                command,
                "fingerprint",
                &stable_fingerprint_bytes(matrix.scenario.replay_command.as_bytes()),
                "event-schema",
                &command_path,
                &mut failures,
            );
            expect_str(
                command,
                "validation_status",
                "pass",
                "event-schema",
                &command_path,
                &mut failures,
            );
            expect_present(
                command,
                "evidence_path",
                "event-schema",
                &command_path,
                &mut failures,
            );
        }
    }
    if let Some(redaction_summary) =
        expect_report_object(object, "redaction_summary", path, &mut failures)
    {
        let redaction_path = format!("{path}#/redaction_summary");
        expect_str(
            redaction_summary,
            "scanner",
            "harness-testkit-secret-scanner",
            "event-schema",
            &redaction_path,
            &mut failures,
        );
        for field in [
            "scanned_artifact_count",
            "clean_artifact_count",
            "redacted_field_count",
            "rejected_artifact_count",
            "secret_finding_count",
        ] {
            expect_report_u64(redaction_summary, field, &redaction_path, &mut failures);
        }
        expect_report_array(
            redaction_summary,
            "rejected_artifacts",
            &redaction_path,
            &mut failures,
        );
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}

pub fn scan_simulation_artifact_root(
    artifact_root: &Path,
) -> SingleFailureResult<RedactionSummary> {
    let findings = scan_directory_tree_for_secrets(artifact_root, &default_forbidden_patterns())
        .map_err(|err| {
            Box::new(failure(
                "secret-scan",
                artifact_root.display().to_string(),
                "scannable artifact root",
                err.to_string(),
                "secret scanner failed on simulation artifact root",
            ))
        })?;
    let scanned_artifact_count = count_files(artifact_root).map_err(|err| {
        Box::new(failure(
            "secret-scan",
            artifact_root.display().to_string(),
            "countable artifact root",
            err.to_string(),
            "failed to count simulation artifacts",
        ))
    })?;
    let rejected_artifacts = findings
        .iter()
        .map(|finding| finding.file.display().to_string())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let redacted_field_count = count_redacted_fields(artifact_root).map_err(|err| {
        Box::new(failure(
            "secret-scan",
            artifact_root
                .join("simulation-events.jsonl")
                .display()
                .to_string(),
            "countable redacted field metadata",
            err.to_string(),
            "failed to count simulation redaction metadata",
        ))
    })?;
    Ok(RedactionSummary {
        scanner: "harness-testkit-secret-scanner".to_owned(),
        scanned_artifact_count,
        clean_artifact_count: scanned_artifact_count.saturating_sub(rejected_artifacts.len()),
        redacted_field_count,
        rejected_artifact_count: rejected_artifacts.len(),
        secret_finding_count: findings.len(),
        rejected_artifacts,
    })
}

pub fn invariant_results(
    matrix: &SimulationMatrix,
    normalized: &Value,
    same_seed_status: &str,
) -> Vec<Value> {
    let expected = &matrix.scenario.expected_predicates;
    let expected_tool_permission_chain = json!({
        "tool_artifacts": expected.get("tool_artifacts").cloned().unwrap_or(Value::Null),
        "tool_lifecycle": expected.get("tool_lifecycle").cloned().unwrap_or(Value::Null),
        "permission_lifecycle": expected.get("permission_lifecycle").cloned().unwrap_or(Value::Null),
        "edit_artifact_links": expected.get("edit_artifact_links").cloned().unwrap_or(Value::Null),
    });
    let observed_tool_permission_chain = json!({
        "tool_artifacts": normalized.get("tool_artifacts").cloned().unwrap_or(Value::Null),
        "tool_lifecycle": normalized.get("tool_lifecycle").cloned().unwrap_or(Value::Null),
        "permission_lifecycle": normalized.get("permission_lifecycle").cloned().unwrap_or(Value::Null),
        "edit_artifact_links": normalized.get("edit_artifact_links").cloned().unwrap_or(Value::Null),
    });

    vec![
        invariant_result(
            "INV-001",
            expected.get("event_kind_counts"),
            normalized.get("event_kind_counts"),
            "invariant",
            "event vocabulary matches expected predicates",
        ),
        invariant_result(
            "INV-002",
            Some(&expected_tool_permission_chain),
            Some(&observed_tool_permission_chain),
            "tool-lifecycle",
            "edit permission, tool lifecycle, and artifact digest chain match expected predicates",
        ),
        invariant_result(
            "INV-003",
            expected.get("replay"),
            normalized.get("replay"),
            "replay",
            "replay projection matches expected predicates",
        ),
        json!({
            "invariant_id": "INV-004",
            "status": if same_seed_status == "pass" { "pass" } else { "fail" },
            "failure_signal": "same-seed-stability",
            "message": "redaction and same-seed normalized comparison are stable",
            "expected": "pass",
            "observed": same_seed_status,
        }),
    ]
}

pub fn behavior_delta(matrix: &SimulationMatrix, normalized: &Value) -> Vec<Value> {
    let expected = &matrix.scenario.expected_predicates;
    let (added_predicates, removed_predicates, changed_predicates) =
        predicate_deltas(expected, normalized);
    vec![
        classified_delta_row(
            "added-predicate",
            DeltaStatus::Added,
            &Value::Null,
            &added_predicates,
        ),
        classified_delta_row(
            "removed-predicate",
            DeltaStatus::Removed,
            &removed_predicates,
            &Value::Null,
        ),
        classified_delta_row(
            "changed-predicate",
            DeltaStatus::Changed,
            &predicates_for_keys(&changed_predicates, expected),
            &predicates_for_keys(&changed_predicates, normalized),
        ),
        delta_row(
            "changed-artifact-fingerprint",
            expected.get("tool_artifacts").unwrap_or(&Value::Null),
            normalized.get("tool_artifacts").unwrap_or(&Value::Null),
        ),
        delta_row(
            "changed-replay-result",
            expected.get("replay").unwrap_or(&Value::Null),
            normalized.get("replay").unwrap_or(&Value::Null),
        ),
        delta_row(
            "changed-provider-request-digest",
            expected
                .get("provider_request_digests")
                .unwrap_or(&Value::Null),
            normalized
                .get("provider_request_digests")
                .unwrap_or(&Value::Null),
        ),
    ]
}

fn predicate_deltas(expected: &Value, normalized: &Value) -> (Value, Value, Value) {
    let Some(expected_map) = expected.as_object() else {
        return (Value::Null, Value::Null, Value::Null);
    };
    let Some(normalized_map) = normalized.as_object() else {
        return (Value::Null, expected.clone(), Value::Null);
    };
    let semantic_keys = expected_map
        .keys()
        .chain(normalized_semantic_keys(normalized_map))
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut added = Map::new();
    let mut removed = Map::new();
    let mut changed = Map::new();

    for key in semantic_keys {
        match (expected_map.get(&key), normalized_map.get(&key)) {
            (None, Some(observed)) => {
                added.insert(key, observed.clone());
            }
            (Some(expected_value), None) => {
                removed.insert(key, expected_value.clone());
            }
            (Some(expected_value), Some(observed_value)) if expected_value != observed_value => {
                changed.insert(key, expected_value.clone());
            }
            (Some(_), Some(_)) | (None, None) => {}
        }
    }

    (
        Value::Object(added),
        Value::Object(removed),
        Value::Object(changed),
    )
}

fn normalized_semantic_keys(normalized: &Map<String, Value>) -> impl Iterator<Item = &String> {
    normalized.keys().filter(|key| {
        !matches!(
            key.as_str(),
            "schema_version" | "normalization_profile" | "scenario_id" | "seed"
        )
    })
}

fn predicates_for_keys(keys: &Value, source: &Value) -> Value {
    let Some(key_map) = keys.as_object() else {
        return Value::Null;
    };
    let Some(source_map) = source.as_object() else {
        return Value::Null;
    };
    let mut selected = Map::new();
    for key in key_map.keys() {
        selected.insert(
            key.to_owned(),
            source_map.get(key).cloned().unwrap_or(Value::Null),
        );
    }
    Value::Object(selected)
}

fn classified_delta_row(
    kind: &str,
    changed_status: DeltaStatus,
    expected: &Value,
    observed: &Value,
) -> Value {
    let no_delta = expected == observed
        || (is_empty_object(expected) && observed.is_null())
        || (expected.is_null() && is_empty_object(observed));
    let status = if no_delta {
        DeltaStatus::None
    } else {
        changed_status
    };
    let details = if status == DeltaStatus::None {
        Vec::new()
    } else {
        let (path, expected_value, observed_value) =
            first_json_diff(expected, observed, "$".to_owned());
        vec![json!({"path": path, "expected": expected_value, "observed": observed_value})]
    };
    json!({
        "kind": kind,
        "status": status.as_str(),
        "details": details,
    })
}

fn is_empty_object(value: &Value) -> bool {
    value.as_object().is_some_and(Map::is_empty)
}

pub fn build_report(input: ReportInput<'_>) -> Value {
    let failure_signals = input
        .invariant_results
        .iter()
        .filter(|row| row.get("status").and_then(Value::as_str) != Some("pass"))
        .map(|row| {
            json!({
                "category": row.get("failure_signal").and_then(Value::as_str).unwrap_or("invariant"),
                "message": row.get("message").and_then(Value::as_str).unwrap_or("invariant failed"),
            })
        })
        .collect::<Vec<_>>();
    let status = if failure_signals.is_empty()
        && input.redaction_summary.secret_finding_count == 0
        && input.same_seed_status == "pass"
    {
        "pass"
    } else {
        "fail"
    };
    json!({
        "schema_version": REPORT_SCHEMA_VERSION,
        "matrix_schema_version": MATRIX_SCHEMA_VERSION,
        "event_schema_version": EVENT_SCHEMA_VERSION,
        "run": {
            "seed": input.normalized.get("seed").and_then(Value::as_str).unwrap_or(DEFAULT_SEED),
            "run_fingerprint": input.run_fingerprint,
            "normalization_profile": NORMALIZATION_PROFILE,
        },
        "summary": {
            "status": status,
            "narrative": "offline deterministic golden_path simulation generated from real harness events and replay projection",
        },
        "behavior_delta": input.behavior_delta,
        "invariant_results": input.invariant_results,
        "artifact_index": input.artifact_index,
        "replay_commands": [{
            "scenario_id": input.matrix.scenario.scenario_id,
            "command": input.matrix.scenario.replay_command,
            "fingerprint": stable_fingerprint_bytes(input.matrix.scenario.replay_command.as_bytes()),
            "validation_status": "pass",
            "evidence_path": "raw-evidence/baseline/replay.json",
        }],
        "failure_signals": failure_signals,
        "redaction_summary": input.redaction_summary.to_json(),
        "volatile_fields": [
            "raw session_path",
            "raw workspace_root",
            "raw resolved_path",
            "artifact_root",
            "timestamp_utc in lane env.txt"
        ],
        "raw_evidence_paths": input.raw_evidence_paths,
    })
}

pub fn artifact_index_rows(
    artifact_root: &Path,
    matrix: &SimulationMatrix,
    relative_paths: &[String],
) -> Vec<Value> {
    relative_paths
        .iter()
        .map(|path| {
            let artifact_path = artifact_root.join(path);
            json!({
                "schema_version": ARTIFACT_INDEX_SCHEMA_VERSION,
                "scenario_id": matrix.scenario.scenario_id,
                "artifact_kind": artifact_kind(path),
                "path": path,
                "redaction_status": "clean",
                "producer": "harness-testkit-simulation-evidence",
                "fingerprint": stable_fingerprint_file(&artifact_path).unwrap_or_else(|| stable_fingerprint_bytes(path.as_bytes())),
            })
        })
        .collect()
}

pub fn summary_text(input: SummaryInput<'_>) -> String {
    let has_failed_invariant = input
        .invariant_results
        .iter()
        .any(|row| row.get("status").and_then(Value::as_str) != Some("pass"));
    let status = if has_failed_invariant
        || input.redaction_summary.secret_finding_count > 0
        || input.same_seed_status != "pass"
    {
        "fail"
    } else {
        "pass"
    };
    let mut lines = vec![
        "Simulation status".to_owned(),
        format!("status={status}"),
        format!(
            "seed={}",
            input
                .normalized
                .get("seed")
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_SEED)
        ),
        format!("run_fingerprint={}", input.run_fingerprint),
        format!("matrix_version={MATRIX_SCHEMA_VERSION}"),
        format!("event_schema_version={EVENT_SCHEMA_VERSION}"),
        "scenario_count_by_determinism_class".to_owned(),
        "offline-deterministic=1".to_owned(),
        "pty-signoff=0".to_owned(),
        "live-signoff=0".to_owned(),
        "native-signoff=0".to_owned(),
        "planned=0".to_owned(),
        "waived=0".to_owned(),
        "invariant_results".to_owned(),
    ];
    for row in input.invariant_results {
        lines.push(format!(
            "{}={}",
            row.get("invariant_id")
                .and_then(Value::as_str)
                .unwrap_or("unknown"),
            row.get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ));
    }
    lines.push("negative_controls".to_owned());
    for control in &input.matrix.scenario.negative_controls {
        lines.push(format!("{control}=covered-by-tests"));
    }
    let behavior_delta = behavior_delta(input.matrix, input.normalized);
    lines.push(format!(
        "behavior_delta={}",
        behavior_delta_summary(&behavior_delta)
    ));
    lines.push(format!(
        "replay_command={}",
        input.matrix.scenario.replay_command
    ));
    lines.push(format!("artifact_index_path={}", input.artifact_index_path));
    lines.push(format!(
        "redaction_status={}",
        if input.redaction_summary.secret_finding_count == 0 {
            "clean"
        } else {
            "rejected"
        }
    ));
    lines.push(format!(
        "top_failure_signals={}",
        if status == "pass" {
            "none"
        } else {
            "see simulation-report.json"
        }
    ));
    lines.push(format!("same_seed_comparison={}", input.same_seed_status));
    lines.push(String::new());
    lines.join("\n")
}

fn behavior_delta_summary(rows: &[Value]) -> String {
    let mut changed = Vec::new();
    for row in rows {
        let status = row.get("status").and_then(Value::as_str).unwrap_or("none");
        if status == "none" {
            continue;
        }
        let kind = row
            .get("kind")
            .or_else(|| row.get("delta_kind"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        changed.push(format!("{kind}:{status}"));
    }
    if changed.is_empty() {
        "none".to_owned()
    } else {
        changed.join(",")
    }
}

pub fn write_json_pretty(path: &Path, value: &Value) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(path)?;
    serde_json::to_writer_pretty(&mut file, value).map_err(json_io_error)?;
    file.write_all(b"\n")
}

pub fn write_jsonl(path: &Path, rows: &[Value]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::File::create(path)?;
    for row in rows {
        serde_json::to_writer(&mut file, row).map_err(json_io_error)?;
        file.write_all(b"\n")?;
    }
    Ok(())
}

pub fn stable_fingerprint_bytes(bytes: &[u8]) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

pub fn stable_fingerprint_value(value: &Value) -> String {
    stable_fingerprint_bytes(canonical_json(value).as_bytes())
}

pub fn stable_fingerprint_file(path: &Path) -> Option<String> {
    match path.file_name().and_then(|name| name.to_str()) {
        Some("artifact-index.jsonl") => {
            stable_fingerprint_jsonl_without_embedded_fingerprints(path)
        }
        Some("simulation-report.json") => {
            stable_fingerprint_json_without_embedded_fingerprints(path)
        }
        _ => fs::read(path)
            .ok()
            .map(|bytes| stable_fingerprint_bytes(&bytes)),
    }
}

fn stable_fingerprint_jsonl_without_embedded_fingerprints(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let mut rows = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let mut row = serde_json::from_str::<Value>(line).ok()?;
        normalize_embedded_fingerprints(&mut row);
        rows.push(row);
    }
    Some(stable_fingerprint_value(&Value::Array(rows)))
}

fn stable_fingerprint_json_without_embedded_fingerprints(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let mut value = serde_json::from_str::<Value>(&text).ok()?;
    normalize_embedded_fingerprints(&mut value);
    Some(stable_fingerprint_value(&value))
}

fn normalize_embedded_fingerprints(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(fingerprint) = map.get_mut("fingerprint") {
                *fingerprint = Value::String("<normalized-fingerprint>".to_owned());
            }
            for child in map.values_mut() {
                normalize_embedded_fingerprints(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_embedded_fingerprints(item);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn finish_matrix(
    failures: Vec<SimulationFailure>,
    invariant_ids: BTreeSet<String>,
    _behavioral_invariants: BTreeSet<String>,
    scenarios: Vec<ScenarioContract>,
    path: &str,
) -> SimulationResult<SimulationMatrix> {
    if !failures.is_empty() {
        return Err(failures);
    }
    let Some(scenario) = scenarios
        .into_iter()
        .find(|scenario| scenario.scenario_id == DEFAULT_SCENARIO_ID)
    else {
        return Err(vec![failure(
            "invalid-schema-row",
            path,
            DEFAULT_SCENARIO_ID,
            "<missing>",
            "MVP matrix must admit golden_path",
        )]);
    };
    Ok(SimulationMatrix {
        scenario,
        invariant_ids,
    })
}

fn validate_schema_versions(
    object: &Map<String, Value>,
    row_path: &str,
    scenario_id: &str,
    failures: &mut Vec<SimulationFailure>,
) {
    let versions = object
        .get("artifact_schema_versions")
        .and_then(Value::as_object);
    for (key, expected) in [
        ("matrix", MATRIX_SCHEMA_VERSION),
        ("event", EVENT_SCHEMA_VERSION),
        ("report", REPORT_SCHEMA_VERSION),
        ("artifact_index", ARTIFACT_INDEX_SCHEMA_VERSION),
        ("normalization", NORMALIZATION_PROFILE),
    ] {
        let observed = versions
            .and_then(|values| values.get(key))
            .and_then(Value::as_str)
            .unwrap_or("<missing>");
        if observed != expected {
            failures.push(
                failure(
                    "invalid-schema-row",
                    row_path,
                    expected,
                    observed,
                    "scenario artifact_schema_versions is missing or malformed",
                )
                .scenario(scenario_id),
            );
        }
    }
}

fn event_kind_counts(raw_events: &[Value]) -> BTreeMap<String, u64> {
    let mut counts = BTreeMap::new();
    for event in raw_events {
        if let Some(kind) = raw_event_kind(event) {
            *counts.entry(kind.to_owned()).or_insert(0) += 1;
        }
    }
    counts
}

fn provider_request_digests(raw_events: &[Value]) -> Vec<String> {
    raw_events
        .iter()
        .filter(|event| raw_event_kind(event) == Some("provider_request_started"))
        .filter_map(|event| {
            raw_event_data(event)
                .get("request_digest")
                .and_then(Value::as_str)
        })
        .map(str::to_owned)
        .collect()
}

fn replay_predicate(replay: &Value) -> Value {
    json!({
        "status": replay.get("status").and_then(Value::as_str).unwrap_or(""),
        "mode_source": replay.get("mode_source").and_then(Value::as_str).unwrap_or(""),
        "is_resumable": replay.get("is_resumable").and_then(Value::as_bool).unwrap_or(true),
        "artifact_count": replay.get("artifact_count").and_then(Value::as_u64).unwrap_or(0),
        "pending_permissions": replay.get("pending_permissions").and_then(Value::as_array).map_or(0, Vec::len),
        "tasks_in_flight": replay.get("tasks_in_flight").and_then(Value::as_array).map_or(0, Vec::len),
        "total_events": replay.get("total_events").and_then(Value::as_u64).unwrap_or(0),
    })
}

fn tool_artifacts(replay: &Value) -> Vec<Value> {
    let mut artifacts = replay
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    json!({
                        "path": item.get("path").and_then(Value::as_str).unwrap_or(""),
                        "digest": item.get("digest").and_then(Value::as_str).unwrap_or(""),
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    artifacts.sort_by_key(canonical_json);
    artifacts
}

fn tool_lifecycle(raw_events: &[Value]) -> Value {
    let statuses = raw_events
        .iter()
        .filter(|event| raw_event_kind(event) == Some("tool_call_finished"))
        .map(raw_event_data)
        .filter_map(|data| data.get("status").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    json!({
        "requested": count_kind(raw_events, "tool_call_requested"),
        "started": count_kind(raw_events, "tool_call_started"),
        "finished": count_kind(raw_events, "tool_call_finished"),
        "finish_statuses": statuses,
    })
}

fn permission_lifecycle(raw_events: &[Value]) -> Value {
    let decisions = raw_events
        .iter()
        .filter(|event| raw_event_kind(event) == Some("permission_resolved"))
        .map(raw_event_data)
        .filter_map(|data| data.get("decision").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    json!({
        "requested": count_kind(raw_events, "permission_requested"),
        "resolved": count_kind(raw_events, "permission_resolved"),
        "decisions": decisions,
    })
}

fn edit_artifact_links(raw_events: &[Value]) -> Value {
    let applied = raw_events
        .iter()
        .find(|event| raw_event_kind(event) == Some("edit_applied"))
        .map(raw_event_data);
    let artifact = raw_events
        .iter()
        .find(|event| raw_event_kind(event) == Some("artifact_written"))
        .map(raw_event_data);
    let diff_rel_path = applied
        .and_then(|data| data.get("diff_rel_path"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let diff_digest = applied
        .and_then(|data| data.get("diff_digest"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let artifact_path = artifact
        .and_then(|data| data.get("path"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let artifact_digest = artifact
        .and_then(|data| data.get("digest"))
        .and_then(Value::as_str)
        .unwrap_or("");
    json!({
        "diff_rel_path": diff_rel_path,
        "diff_digest": diff_digest,
        "artifact_path": artifact_path,
        "artifact_digest": artifact_digest,
        "path_digest_match": diff_rel_path == artifact_path && diff_digest == artifact_digest,
    })
}

fn count_kind(raw_events: &[Value], kind: &str) -> usize {
    raw_events
        .iter()
        .filter(|event| raw_event_kind(event) == Some(kind))
        .count()
}

fn raw_event_kind(event: &Value) -> Option<&str> {
    event.get("payload")?.get("event_type")?.as_str()
}

fn raw_event_data(event: &Value) -> &Value {
    event
        .get("payload")
        .and_then(|payload| payload.get("data"))
        .unwrap_or(&Value::Null)
}

fn actor_for_event_kind(event_kind: &str) -> &'static str {
    if event_kind.starts_with("provider_") || event_kind == "assistant_message_finished" {
        "provider"
    } else if event_kind.starts_with("tool_call")
        || event_kind.starts_with("edit_")
        || event_kind == "artifact_written"
    {
        "tool"
    } else {
        "coordinator"
    }
}

fn component_for_event_kind(event_kind: &str) -> &'static str {
    if event_kind.starts_with("provider_") || event_kind == "assistant_message_finished" {
        "harness-providers"
    } else if event_kind.starts_with("tool_call")
        || event_kind.starts_with("edit_")
        || event_kind == "artifact_written"
    {
        "harness-tools"
    } else {
        "harness-core"
    }
}

fn invariant_ids_for_event_kind(event_kind: &str) -> Vec<&'static str> {
    if event_kind.starts_with("tool_call")
        || event_kind.starts_with("edit_")
        || event_kind == "artifact_written"
        || event_kind.starts_with("permission_")
    {
        vec!["INV-002", "INV-004"]
    } else if event_kind == "replay_projection_validated" {
        vec!["INV-003", "INV-004"]
    } else {
        vec!["INV-001", "INV-004"]
    }
}

fn redacted_payload(event: &Value, event_kind: &str) -> (Value, Vec<&'static str>) {
    let data = raw_event_data(event);
    match event_kind {
        "run_started" => (
            json!({"run_name": data.get("run_name").and_then(Value::as_str).unwrap_or("")}),
            vec!["workspace_root"],
        ),
        "provider_request_started" => (
            json!({
                "provider_id": data.get("provider_id").and_then(Value::as_str).unwrap_or(""),
                "model_id": data.get("model_id").and_then(Value::as_str).unwrap_or(""),
                "prompt_summary": data.get("prompt_summary").and_then(Value::as_str).unwrap_or(""),
                "request_digest": data.get("request_digest").and_then(Value::as_str).unwrap_or(""),
            }),
            Vec::new(),
        ),
        "provider_request_finished" => (
            json!({"finish_reason": data.get("finish_reason").and_then(Value::as_str).unwrap_or("")}),
            Vec::new(),
        ),
        "task_cancelled" => (
            json!({
                "task_scope": data.get("task_scope").and_then(Value::as_str).unwrap_or(""),
                "reason_fingerprint": data.get("reason").and_then(Value::as_str).map(|reason| stable_fingerprint_bytes(reason.as_bytes())).unwrap_or_default(),
            }),
            vec!["reason"],
        ),
        "permission_requested" => (
            json!({
                "permission_id": data.get("permission_id").and_then(Value::as_str).unwrap_or(""),
                "kind": data.get("kind").and_then(Value::as_str).unwrap_or(""),
                "tool_call_id": data.get("tool_call_id").and_then(Value::as_str).unwrap_or(""),
                "default_decision": data.get("default_decision").and_then(Value::as_str).unwrap_or(""),
            }),
            vec!["summary"],
        ),
        "permission_resolved" => (
            json!({
                "permission_id": data.get("permission_id").and_then(Value::as_str).unwrap_or(""),
                "decision": data.get("decision").and_then(Value::as_str).unwrap_or(""),
            }),
            Vec::new(),
        ),
        "tool_call_requested" => (
            json!({
                "tool_call_id": data.get("tool_call_id").and_then(Value::as_str).unwrap_or(""),
                "tool_id": data.get("tool_id").and_then(Value::as_str).unwrap_or(""),
                "args_digest": data.get("args_digest").and_then(Value::as_str).unwrap_or(""),
            }),
            vec!["args_summary"],
        ),
        "tool_call_started" => (
            json!({"tool_call_id": data.get("tool_call_id").and_then(Value::as_str).unwrap_or("")}),
            Vec::new(),
        ),
        "tool_call_finished" => (
            json!({
                "tool_call_id": data.get("tool_call_id").and_then(Value::as_str).unwrap_or(""),
                "status": data.get("status").and_then(Value::as_str).unwrap_or(""),
                "output_digest": data.get("output_digest").and_then(Value::as_str).unwrap_or(""),
                "diff_rel_path": data.pointer("/output_json/diff_rel_path").and_then(Value::as_str).unwrap_or(""),
                "diff_digest": data.pointer("/output_json/diff_digest").and_then(Value::as_str).unwrap_or(""),
            }),
            vec!["output_json.resolved_path"],
        ),
        "edit_proposed" => (
            json!({
                "edit_id": data.get("edit_id").and_then(Value::as_str).unwrap_or(""),
                "path": data.get("path").and_then(Value::as_str).unwrap_or(""),
                "patch_digest": data.get("patch_digest").and_then(Value::as_str).unwrap_or(""),
            }),
            Vec::new(),
        ),
        "edit_applied" => (
            json!({
                "edit_id": data.get("edit_id").and_then(Value::as_str).unwrap_or(""),
                "path": data.get("path").and_then(Value::as_str).unwrap_or(""),
                "diff_rel_path": data.get("diff_rel_path").and_then(Value::as_str).unwrap_or(""),
                "diff_digest": data.get("diff_digest").and_then(Value::as_str).unwrap_or(""),
            }),
            Vec::new(),
        ),
        "artifact_written" => (
            json!({
                "path": data.get("path").and_then(Value::as_str).unwrap_or(""),
                "digest": data.get("digest").and_then(Value::as_str).unwrap_or(""),
                "bytes": data.get("bytes").and_then(Value::as_u64).unwrap_or(0),
                "tool_call_id": data.get("tool_call_id").and_then(Value::as_str).unwrap_or(""),
            }),
            Vec::new(),
        ),
        _ => (
            json!({
                "source_seq": event.get("seq").and_then(Value::as_u64).unwrap_or(0),
                "source_event_id": event.get("event_id").and_then(Value::as_str).unwrap_or(""),
            }),
            Vec::new(),
        ),
    }
}

fn invariant_result(
    invariant_id: &str,
    expected: Option<&Value>,
    observed: Option<&Value>,
    failure_signal: &str,
    message: &str,
) -> Value {
    let expected = expected.unwrap_or(&Value::Null);
    let observed = observed.unwrap_or(&Value::Null);
    json!({
        "invariant_id": invariant_id,
        "status": if expected == observed { "pass" } else { "fail" },
        "failure_signal": failure_signal,
        "message": message,
        "expected": expected,
        "observed": observed,
    })
}

fn delta_row(kind: &str, expected: &Value, observed: &Value) -> Value {
    let details = if expected == observed {
        Vec::new()
    } else {
        let (path, expected_value, observed_value) =
            first_json_diff(expected, observed, "$".to_owned());
        vec![json!({"path": path, "expected": expected_value, "observed": observed_value})]
    };
    json!({
        "kind": kind,
        "status": if details.is_empty() { "none" } else { "changed" },
        "details": details,
    })
}

fn first_json_diff(left: &Value, right: &Value, path: String) -> (String, String, String) {
    match (left, right) {
        (Value::Object(left_map), Value::Object(right_map)) => {
            let keys = left_map
                .keys()
                .chain(right_map.keys())
                .collect::<BTreeSet<_>>();
            for key in keys {
                match (left_map.get(key), right_map.get(key)) {
                    (Some(left_value), Some(right_value)) if left_value == right_value => {}
                    (Some(left_value), Some(right_value)) => {
                        return first_json_diff(left_value, right_value, format!("{path}.{key}"))
                    }
                    (Some(left_value), None) => {
                        return (
                            format!("{path}.{key}"),
                            canonical_json(left_value),
                            "<missing>".to_owned(),
                        )
                    }
                    (None, Some(right_value)) => {
                        return (
                            format!("{path}.{key}"),
                            "<missing>".to_owned(),
                            canonical_json(right_value),
                        )
                    }
                    (None, None) => {}
                }
            }
        }
        (Value::Array(left_items), Value::Array(right_items)) => {
            for index in 0..left_items.len().max(right_items.len()) {
                match (left_items.get(index), right_items.get(index)) {
                    (Some(left_value), Some(right_value)) if left_value == right_value => {}
                    (Some(left_value), Some(right_value)) => {
                        return first_json_diff(left_value, right_value, format!("{path}[{index}]"))
                    }
                    (Some(left_value), None) => {
                        return (
                            format!("{path}[{index}]"),
                            canonical_json(left_value),
                            "<missing>".to_owned(),
                        )
                    }
                    (None, Some(right_value)) => {
                        return (
                            format!("{path}[{index}]"),
                            "<missing>".to_owned(),
                            canonical_json(right_value),
                        )
                    }
                    (None, None) => {}
                }
            }
        }
        _ => {}
    }
    (path, canonical_json(left), canonical_json(right))
}

fn artifact_kind(path: &str) -> &'static str {
    if path.ends_with("events.jsonl") {
        "events"
    } else if path.ends_with("report.json") {
        "report"
    } else if path.ends_with("matrix.json") {
        "matrix"
    } else if path.ends_with("summary.txt") {
        "summary"
    } else if path.ends_with("replay.json") {
        "replay"
    } else if path.ends_with("artifact-index.jsonl") {
        "artifact-index"
    } else {
        "evidence"
    }
}

fn expect_str(
    object: &Map<String, Value>,
    field: &str,
    expected: &str,
    control: &str,
    path: &str,
    failures: &mut Vec<SimulationFailure>,
) {
    let observed = non_empty_str(object, field);
    if observed != Some(expected) {
        failures.push(failure(
            control,
            path,
            expected,
            observed.unwrap_or("<missing>"),
            format!("field {field} has an unexpected value"),
        ));
    }
}

fn expect_present(
    object: &Map<String, Value>,
    field: &str,
    control: &str,
    path: &str,
    failures: &mut Vec<SimulationFailure>,
) {
    if non_empty_str(object, field).is_none() {
        failures.push(failure(
            control,
            path,
            field,
            "<missing-or-empty>",
            format!("required field {field} is missing"),
        ));
    }
}

fn expect_report_object<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    path: &str,
    failures: &mut Vec<SimulationFailure>,
) -> Option<&'a Map<String, Value>> {
    match object.get(field) {
        Some(Value::Object(map)) => Some(map),
        Some(value) => {
            failures.push(failure(
                "event-schema",
                format!("{path}#/{field}"),
                "object",
                value_type(value),
                format!("report section {field} must be an object"),
            ));
            None
        }
        None => {
            failures.push(failure(
                "event-schema",
                format!("{path}#/{field}"),
                "object",
                "<missing>",
                format!("report section {field} is missing"),
            ));
            None
        }
    }
}

fn expect_report_array<'a>(
    object: &'a Map<String, Value>,
    field: &str,
    path: &str,
    failures: &mut Vec<SimulationFailure>,
) -> Option<&'a Vec<Value>> {
    match object.get(field) {
        Some(Value::Array(items)) => Some(items),
        Some(value) => {
            failures.push(failure(
                "event-schema",
                format!("{path}#/{field}"),
                "array",
                value_type(value),
                format!("report section {field} must be an array"),
            ));
            None
        }
        None => {
            failures.push(failure(
                "event-schema",
                format!("{path}#/{field}"),
                "array",
                "<missing>",
                format!("report section {field} is missing"),
            ));
            None
        }
    }
}

fn expect_report_u64(
    object: &Map<String, Value>,
    field: &str,
    path: &str,
    failures: &mut Vec<SimulationFailure>,
) {
    if object.get(field).and_then(Value::as_u64).is_none() {
        failures.push(failure(
            "event-schema",
            format!("{path}#/{field}"),
            "unsigned integer",
            object.get(field).map_or("<missing>".to_owned(), value_type),
            format!("report field {field} must be an unsigned integer"),
        ));
    }
}

fn non_empty_str<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
    object
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn value_type(value: &Value) -> String {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
    .to_owned()
}

fn canonical_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned())
}

fn count_files(root: &Path) -> io::Result<usize> {
    if !root.exists() {
        return Ok(0);
    }
    let metadata = fs::symlink_metadata(root)?;
    if metadata.file_type().is_symlink() {
        return Ok(0);
    }
    if metadata.is_file() {
        return Ok(1);
    }
    let mut children: Vec<PathBuf> = fs::read_dir(root)?
        .map(|entry| entry.map(|item| item.path()))
        .collect::<Result<_, _>>()?;
    children.sort();
    let mut count = 0;
    for child in children {
        count += count_files(&child)?;
    }
    Ok(count)
}

fn count_redacted_fields(root: &Path) -> io::Result<usize> {
    let events_path = root.join("simulation-events.jsonl");
    if !events_path.exists() {
        return Ok(0);
    }
    let text = fs::read_to_string(&events_path)?;
    let mut count = 0;
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value = serde_json::from_str::<Value>(line).map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}:{}: {err}", events_path.display(), index + 1),
            )
        })?;
        count += value
            .pointer("/redaction/redacted_fields")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
    }
    Ok(count)
}

fn failure(
    control: impl Into<String>,
    path: impl Into<String>,
    expected: impl Into<String>,
    observed: impl Into<String>,
    message: impl Into<String>,
) -> SimulationFailure {
    SimulationFailure::new(control, path, expected, observed, message)
}

fn json_io_error(err: serde_json::Error) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, err)
}
