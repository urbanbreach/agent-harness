use std::collections::BTreeSet;
use std::fs;

use serde_json::{Map, Value};

use super::storage;
use super::{TaskGateError, TaskGateInput};

mod transition;

pub(super) use transition::{prior_receipt_sha256, record_plan_transition, PlanTransition};

pub(super) fn validate_active(value: &Value) -> Result<(), TaskGateError> {
    let object = value
        .as_object()
        .ok_or_else(|| TaskGateError::Invalid("boulder is not an object".to_owned()))?;
    if map_string(object, "status")? != "active" {
        return Err(TaskGateError::Invalid("Boulder is not active".to_owned()));
    }
    let active_work = map_string(object, "active_work_id")?;
    let works = map_object(object, "works")?;
    let work = map_object(works, active_work)?;
    if map_string(work, "status")? != "active" {
        return Err(TaskGateError::Invalid(
            "active work is not active".to_owned(),
        ));
    }
    validate_mirror_state(object, work)?;
    match object.get("task_sessions") {
        Some(Value::Object(sessions)) if sessions.is_empty() => {}
        Some(Value::Object(_)) => {
            return Err(TaskGateError::Invalid(
                "worker task sessions are present".to_owned(),
            ));
        }
        Some(_) => {
            return Err(TaskGateError::Invalid(
                "task_sessions is not an object".to_owned(),
            ));
        }
        None => {
            return Err(TaskGateError::Invalid(
                "task_sessions is missing".to_owned(),
            ));
        }
    }
    if object
        .get("bootstrap_admission_58")
        .and_then(Value::as_object)
        .is_some_and(|admission| admission.get("worker_spawn_allowed") == Some(&Value::Bool(true)))
    {
        return Err(TaskGateError::Invalid(
            "bootstrap admission permits worker spawning".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn validate_plan_binding(
    boulder: &Value,
    plan_sha256: &str,
) -> Result<(), TaskGateError> {
    let reviewed = value_string(boulder, "reviewed_plan_sha256")?;
    let pending = boulder.get("pending_plan_sha256").and_then(Value::as_str);
    let chain_head = value_string(boulder, "raw_plan_hash_chain_head")?;
    if reviewed != plan_sha256 && pending != Some(plan_sha256) && chain_head != plan_sha256 {
        return Err(TaskGateError::Invalid(
            "Boulder reviewed plan digest differs from plan".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn validate_plan_contract(
    boulder: &Value,
    contract_sha256: &str,
) -> Result<(), TaskGateError> {
    if value_string(boulder, "plan_contract_sha256")? != contract_sha256 {
        return Err(TaskGateError::Invalid(
            "Boulder plan contract digest differs from plan".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn validate_bootstrap(boulder: &Value, task: &str) -> Result<(), TaskGateError> {
    if task != "58" {
        return Ok(());
    }
    let admission = value_object(boulder, "bootstrap_admission_58")?;
    if map_string(admission, "status")? != "admitted"
        || admission.get("task").and_then(Value::as_u64) != Some(58)
        || admission.get("execution_owner").and_then(Value::as_str) != Some("root_orchestrator")
        || admission.get("worker_spawn_allowed") != Some(&Value::Bool(false))
        || admission.get("task_sessions_empty_at_promotion") != Some(&Value::Bool(true))
    {
        return Err(TaskGateError::Invalid(
            "Todo 58 bootstrap admission is not root-owned and worker-free".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn validate_revocations(
    input: &TaskGateInput,
    boulder: &Value,
) -> Result<Vec<Value>, TaskGateError> {
    let bindings = boulder
        .get("completion_revocation_bindings")
        .and_then(Value::as_array)
        .ok_or_else(|| TaskGateError::Invalid("Boulder has no revocation bindings".to_owned()))?;
    let mut seen = BTreeSet::new();
    input
        .revocations
        .iter()
        .map(|path| {
            let digest = storage::digest(&fs::read(path).map_err(|error| TaskGateError::Io {
                path: path.clone(),
                detail: error.to_string(),
            })?)?;
            let display = path.to_string_lossy();
            if !seen.insert(display.to_string()) {
                return Err(TaskGateError::Invalid(
                    "completion revocation paths must be distinct".to_owned(),
                ));
            }
            let binding = bindings.iter().find(|binding| {
                binding
                    .get("path")
                    .and_then(Value::as_str)
                    .is_some_and(|expected| display.ends_with(expected))
            });
            let Some(binding) = binding else {
                return Err(TaskGateError::Invalid(format!(
                    "revocation {} is not Boulder-bound",
                    path.display()
                )));
            };
            if binding.get("sha256").and_then(Value::as_str) != Some(digest.as_str()) {
                return Err(TaskGateError::Invalid(format!(
                    "revocation digest differs for {}",
                    path.display()
                )));
            }
            Ok(serde_json::json!({"path": path, "sha256": digest}))
        })
        .collect()
}

pub(super) fn validate_dependencies(input: &TaskGateInput) -> Result<(), TaskGateError> {
    for path in &input.dependencies {
        let value = storage::read_json(path)?;
        if value_string(&value, "status")? != "completed"
            || value_string(&value, "candidate_sha256")? != input.candidate_sha256
        {
            return Err(TaskGateError::Invalid(format!(
                "dependency receipt is not completed for {}",
                path.display()
            )));
        }
    }
    Ok(())
}

fn map_object<'a>(
    object: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a Map<String, Value>, TaskGateError> {
    object
        .get(name)
        .and_then(Value::as_object)
        .ok_or_else(|| TaskGateError::Invalid(format!("{name} is not an object")))
}

fn value_object<'a>(value: &'a Value, name: &str) -> Result<&'a Map<String, Value>, TaskGateError> {
    value
        .get(name)
        .and_then(Value::as_object)
        .ok_or_else(|| TaskGateError::Invalid(format!("{name} is not an object")))
}

fn map_string<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a str, TaskGateError> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| TaskGateError::Invalid(format!("{name} is not a string")))
}

fn value_string<'a>(value: &'a Value, name: &str) -> Result<&'a str, TaskGateError> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| TaskGateError::Invalid(format!("{name} is not a string")))
}

fn validate_mirror_state(
    root: &Map<String, Value>,
    work: &Map<String, Value>,
) -> Result<(), TaskGateError> {
    for field in [
        "pending_plan_sha256",
        "plan_contract_sha256",
        "raw_plan_hash_chain_head",
        "task_completion_receipts",
    ] {
        if mirror_value(root, field) != mirror_value(work, field) {
            return Err(TaskGateError::Invalid(format!(
                "Boulder mirror divergence for {field}"
            )));
        }
    }
    Ok(())
}

fn mirror_value<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a Value> {
    object.get(field).filter(|value| !value.is_null())
}
