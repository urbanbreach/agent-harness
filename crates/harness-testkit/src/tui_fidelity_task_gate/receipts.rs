mod values;

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::boulder;
use super::plan;
use super::storage;
use super::{TaskGateError, TaskGateInput, ADMISSION_SCHEMA, GATE_SCHEMA};
pub(super) use values::value_string;
use values::{is_digest, value_object};

const WATCHDOG_SCHEMA: &str = "harness.tui-fidelity.watchdog.v1";
const MAX_DEADLINE_MILLIS: u128 = 120_000;

pub(super) fn validate_input(input: &TaskGateInput) -> Result<(), TaskGateError> {
    if input.task.is_empty() || !input.task.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(TaskGateError::Invalid("task label is invalid".to_owned()));
    }
    if !is_digest(&input.candidate_sha256) {
        return Err(TaskGateError::Invalid(
            "candidate digest must be a SHA-1 or SHA-256 hex digest".to_owned(),
        ));
    }
    if input.revocations.len() != 2 {
        return Err(TaskGateError::Invalid(
            "both completion revocation records are required".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn validate_admission(
    value: &Value,
    input: &TaskGateInput,
) -> Result<(), TaskGateError> {
    if value_string(value, "schema_version")? != ADMISSION_SCHEMA
        || value_string(value, "status")? != "admitted"
        || value_string(value, "task")? != input.task
        || value_string(value, "candidate_sha256")? != input.candidate_sha256
        || value.get("worker_spawn_allowed") != Some(&Value::Bool(false))
        || value.get("task_sessions_empty") != Some(&Value::Bool(true))
    {
        return Err(TaskGateError::Invalid(
            "task admission is missing a matching root-owned binding".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn validate_watchdog(value: &Value) -> Result<(), TaskGateError> {
    if value_string(value, "schema_version")? != WATCHDOG_SCHEMA
        || value.get("status").and_then(Value::as_str) != Some("passed")
        || value.get("deadline_seconds").and_then(Value::as_u64) != Some(120)
        || value.get("timed_out") != Some(&Value::Bool(false))
        || value.get("cancelled") != Some(&Value::Bool(false))
        || value.get("surviving_process_group") != Some(&Value::Bool(false))
        || value.get("exit_code").and_then(Value::as_i64) != Some(0)
        || value
            .get("command")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty)
        || value
            .get("duration_millis")
            .and_then(Value::as_u64)
            .is_none_or(|duration| u128::from(duration) > MAX_DEADLINE_MILLIS)
        || value.get("watchdog").and_then(Value::as_str) != Some("scripts/tui-fidelity/watchdog.sh")
    {
        return Err(TaskGateError::Invalid(
            "aggregate is not a passing 120-second watchdog receipt".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn validate_sealed_verification(
    value: &Value,
    input: &TaskGateInput,
) -> Result<(), TaskGateError> {
    if value_string(value, "schema_version")? != "harness.tui-fidelity.verification.v1"
        || value.get("sealed") != Some(&Value::Bool(true))
        || value_string(value, "candidate_sha")? != input.candidate_sha256
    {
        return Err(TaskGateError::Invalid(
            "verification receipt is not sealed for this candidate".to_owned(),
        ));
    }
    let scheduler = value_object(value, "scheduler")?;
    let key_count = value
        .get("key_count")
        .and_then(Value::as_u64)
        .ok_or_else(|| TaskGateError::Invalid("verification key_count is missing".to_owned()))?;
    if key_count == 0 {
        return Err(TaskGateError::Invalid(
            "verification has no required keys".to_owned(),
        ));
    }
    for field_name in ["failed", "cancelled"] {
        if scheduler.get(field_name).and_then(Value::as_u64) != Some(0) {
            return Err(TaskGateError::Invalid(format!(
                "verification scheduler has nonzero {field_name}"
            )));
        }
    }
    if scheduler.get("passed").and_then(Value::as_u64) != Some(key_count)
        || scheduler.get("skipped").and_then(Value::as_u64) != Some(0)
    {
        return Err(TaskGateError::Invalid(
            "verification scheduler does not account for every required key".to_owned(),
        ));
    }
    let evidence = PathBuf::from(value_string(value, "evidence_path")?);
    let evidence = if evidence.is_absolute() {
        evidence
    } else {
        input.evidence_root.join(evidence)
    };
    let seal = storage::read_json(&evidence.join("seal-manifest.json"))?;
    let records = seal
        .get("records")
        .and_then(Value::as_array)
        .ok_or_else(|| TaskGateError::Invalid("sealed evidence has no records".to_owned()))?;
    if records.len() != usize::try_from(key_count).unwrap_or(usize::MAX)
        || records.is_empty()
        || records
            .iter()
            .any(|record| record.get("state").and_then(Value::as_str) != Some("passed"))
    {
        return Err(TaskGateError::Invalid(
            "sealed evidence contains a non-passed key".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn validate_shard(path: &Path, input: &TaskGateInput) -> Result<(), TaskGateError> {
    let value = storage::read_json(path)?;
    if matches!(
        value.get("status").and_then(Value::as_str),
        Some("failed") | Some("timed_out") | Some("cancelled")
    ) {
        return Err(TaskGateError::Invalid(format!(
            "required shard failed: {}",
            path.display()
        )));
    }
    if let Some(candidate) = value.get("candidate_sha256").and_then(Value::as_str) {
        if candidate != input.candidate_sha256 {
            return Err(TaskGateError::Invalid(format!(
                "shard candidate differs: {}",
                path.display()
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_gate_receipt(
    value: &Value,
    input: &TaskGateInput,
) -> Result<(), TaskGateError> {
    if value_string(value, "schema_version")? != GATE_SCHEMA
        || value_string(value, "status")? != "verified"
        || value_string(value, "task")? != input.task
        || value_string(value, "candidate_sha256")? != input.candidate_sha256
    {
        return Err(TaskGateError::Invalid(
            "task verification receipt does not match completion request".to_owned(),
        ));
    }
    for (name, path) in [
        ("admission_receipt", &input.admission_receipt),
        ("aggregate_receipt", &input.aggregate_receipt),
        ("verification_receipt", &input.verification_receipt),
    ] {
        if !path_matches(value, name, path) {
            return Err(TaskGateError::Invalid(format!(
                "task verification receipt has a mismatched {name}"
            )));
        }
    }
    let listed_shards = value
        .get("shards")
        .and_then(Value::as_array)
        .ok_or_else(|| TaskGateError::Invalid("task verification shards are missing".to_owned()))?;
    if listed_shards.len() != input.shards.len()
        || input.shards.iter().enumerate().any(|(index, path)| {
            listed_shards.get(index).and_then(Value::as_str)
                != Some(path.to_string_lossy().as_ref())
        })
    {
        return Err(TaskGateError::Invalid(
            "task verification shard selection differs".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn validate_current_execution(
    input: &TaskGateInput,
    plan_sha256: &str,
    gate: &Value,
) -> Result<(), TaskGateError> {
    let admission = storage::read_json(&input.admission_receipt)?;
    validate_admission(&admission, input)?;
    if value_string(&admission, "plan_sha256")? != plan_sha256 {
        return Err(TaskGateError::Invalid(
            "plan changed after task admission".to_owned(),
        ));
    }
    let plan_text = storage::read_text(&input.plan)?;
    plan::ensure_open_task(&plan_text, &input.task)?;
    let boulder_json = storage::read_json(&input.boulder)?;
    boulder::validate_active(&boulder_json)?;
    boulder::validate_plan_binding(&boulder_json, plan_sha256)?;
    boulder::validate_revocations(input, &boulder_json)?;
    boulder::validate_dependencies(input)?;
    validate_watchdog(&storage::read_json(&input.aggregate_receipt)?)?;
    validate_sealed_verification(&storage::read_json(&input.verification_receipt)?, input)?;
    if input.shards.is_empty() {
        return Err(TaskGateError::Invalid(
            "task verification has no matching required evidence".to_owned(),
        ));
    }
    for shard in &input.shards {
        validate_shard(shard, input)?;
    }
    if value_string(gate, "plan_sha256")? != plan_sha256 {
        return Err(TaskGateError::Invalid(
            "task verification has no matching required evidence".to_owned(),
        ));
    }
    Ok(())
}

fn path_matches(value: &Value, field: &str, path: &Path) -> bool {
    value.get(field).and_then(Value::as_str) == Some(path.to_string_lossy().as_ref())
}
