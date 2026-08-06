use std::path::Path;

use serde_json::{Map, Value};

use super::super::storage;
use super::super::TaskGateError;

pub(crate) struct PlanTransition<'a> {
    pub(crate) task: &'a str,
    pub(crate) receipt: &'a Path,
    pub(crate) receipt_sha256: &'a str,
    pub(crate) raw_before: &'a str,
    pub(crate) raw_after: &'a str,
    pub(crate) contract: &'a str,
    pub(crate) prior_receipt_sha256: Option<&'a str>,
}

pub(crate) fn record_plan_transition(
    path: &Path,
    transition: &PlanTransition<'_>,
) -> Result<(), TaskGateError> {
    let mut boulder = storage::read_json(path)?;
    super::validate_active(&boulder)?;
    let active_work = super::value_string(&boulder, "active_work_id")?.to_owned();
    let object = boulder
        .as_object_mut()
        .ok_or_else(|| TaskGateError::Invalid("boulder is not an object".to_owned()))?;
    apply_plan_transition(object, transition)?;
    let works = object
        .get_mut("works")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| TaskGateError::Invalid("works is not an object".to_owned()))?;
    let work = works
        .get_mut(&active_work)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| TaskGateError::Invalid("active work is not an object".to_owned()))?;
    apply_plan_transition(work, transition)?;
    storage::atomic_write_json(path, &boulder)
}

pub(crate) fn prior_receipt_sha256(boulder: &Value) -> Result<Option<String>, TaskGateError> {
    let Some(receipts) = boulder.get("task_completion_receipts") else {
        return Ok(None);
    };
    let receipts = receipts.as_object().ok_or_else(|| {
        TaskGateError::Invalid("task_completion_receipts is not an object".to_owned())
    })?;
    if receipts.is_empty() {
        return Ok(None);
    }
    let bytes =
        serde_json::to_vec(receipts).map_err(|error| TaskGateError::Invalid(error.to_string()))?;
    storage::digest(&bytes).map(Some)
}

fn apply_plan_transition(
    object: &mut Map<String, Value>,
    transition: &PlanTransition<'_>,
) -> Result<(), TaskGateError> {
    object.insert(
        "pending_plan_sha256".to_owned(),
        Value::String(transition.raw_after.to_owned()),
    );
    object.insert(
        "plan_contract_sha256".to_owned(),
        Value::String(transition.contract.to_owned()),
    );
    object.insert(
        "raw_plan_hash_chain_head".to_owned(),
        Value::String(transition.raw_after.to_owned()),
    );
    let receipts = object
        .entry("task_completion_receipts")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| {
            TaskGateError::Invalid("task_completion_receipts is not an object".to_owned())
        })?;
    receipts.insert(
        transition.task.to_owned(),
        serde_json::json!({
            "path": transition.receipt,
            "completion_receipt_sha256": transition.receipt_sha256,
            "plan_sha256": transition.raw_after,
            "raw_plan_sha256_before": transition.raw_before,
            "raw_plan_sha256_after": transition.raw_after,
            "plan_contract_sha256": transition.contract,
            "prior_receipt_sha256": transition.prior_receipt_sha256,
            "checkbox_only": true,
        }),
    );
    Ok(())
}
