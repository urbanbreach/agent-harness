// allow: SIZE_OK — simulation matrix schema + evidence collection (matrix version + invariant definitions + artifact provenance)
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;

mod evidence;
mod fingerprint;
mod validation;

pub use evidence::{
    artifact_index_rows, behavior_delta, build_normalized_summary, build_report,
    compare_normalized_summaries, invariant_results, scan_simulation_artifact_root,
    simulation_event_rows, summary_text,
};
pub use fingerprint::{
    stable_fingerprint_bytes, stable_fingerprint_file, stable_fingerprint_value, write_json_pretty,
    write_jsonl,
};
pub use validation::{
    validate_artifact_index, validate_artifact_index_file, validate_report, validate_report_file,
    validate_simulation_events, validate_simulation_events_file,
};

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

fn failure(
    control: impl Into<String>,
    path: impl Into<String>,
    expected: impl Into<String>,
    observed: impl Into<String>,
    message: impl Into<String>,
) -> SimulationFailure {
    SimulationFailure::new(control, path, expected, observed, message)
}
