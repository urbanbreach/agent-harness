use serde_json::{Map, Value};
use std::collections::BTreeSet;
use std::path::Path;

use super::{
    expect_present, expect_str, failure, non_empty_str, read_json_file, read_jsonl_file,
    stable_fingerprint_bytes, stable_fingerprint_file, string_array, value_type, SimulationFailure,
    SimulationMatrix, SimulationResult, ARTIFACT_INDEX_SCHEMA_VERSION, EVENT_SCHEMA_VERSION,
    MATRIX_SCHEMA_VERSION, NORMALIZATION_PROFILE, REPORT_SCHEMA_VERSION,
};

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
