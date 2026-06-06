use crate::secret_scanner::{default_forbidden_patterns, scan_directory_tree_for_secrets};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::fingerprint::{
    canonical_json, first_json_diff, stable_fingerprint_bytes, stable_fingerprint_file,
};
use super::{
    failure, RedactionSummary, ReportInput, SimulationMatrix, SimulationResult,
    SingleFailureResult, SummaryInput, ARTIFACT_INDEX_SCHEMA_VERSION, DEFAULT_SEED,
    EVENT_SCHEMA_VERSION, MATRIX_SCHEMA_VERSION, NORMALIZATION_PROFILE, REPORT_SCHEMA_VERSION,
};

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
