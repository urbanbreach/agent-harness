use std::fmt;

use serde_json::{Map, Value};

#[derive(Debug)]
pub enum ClosureError {
    Json(String),
    Invalid(String),
}

impl fmt::Display for ClosureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(detail) => write!(formatter, "closure JSON: {detail}"),
            Self::Invalid(detail) => write!(formatter, "closure state: {detail}"),
        }
    }
}

impl std::error::Error for ClosureError {}

pub fn validate_boulder_for_completion(
    boulder_json: &str,
    receipt_json: &str,
) -> Result<Value, ClosureError> {
    let boulder: Value = serde_json::from_str(boulder_json)
        .map_err(|error| ClosureError::Json(format!("boulder: {error}")))?;
    let receipt: Value = serde_json::from_str(receipt_json)
        .map_err(|error| ClosureError::Json(format!("receipt: {error}")))?;
    validate_receipt_shape(&receipt)?;
    let boulder_object = object(&boulder, "boulder")?;
    let works = object(field(boulder_object, "works")?, "works")?;
    let active_work_id = string(field(boulder_object, "active_work_id")?, "active_work_id")?;
    let work = object(
        works
            .get(active_work_id)
            .ok_or_else(|| ClosureError::Invalid("active work is missing from works".to_owned()))?,
        "active work",
    )?;
    let mirror = boulder_object;
    validate_mirror_identity(work, mirror)?;
    if string(field(work, "status")?, "work status")? != "active"
        || string(field(mirror, "status")?, "mirror status")? != "active"
    {
        return Err(ClosureError::Invalid(
            "only an active-to-completed transition is permitted".to_owned(),
        ));
    }
    Ok(boulder)
}

pub fn complete_boulder_atomically(
    boulder_path: &std::path::Path,
    receipt_json: &str,
) -> Result<(), ClosureError> {
    let boulder_json = std::fs::read_to_string(boulder_path)
        .map_err(|error| ClosureError::Invalid(format!("read boulder: {error}")))?;
    let mut boulder = validate_boulder_for_completion(&boulder_json, receipt_json)?;
    let receipt: Value = serde_json::from_str(receipt_json)
        .map_err(|error| ClosureError::Json(format!("receipt: {error}")))?;
    let receipt_object = object(&receipt, "receipt")?;
    let receipt_path = string_field(receipt_object, "receipt_path")?;
    let receipt_sha256 = string_field(receipt_object, "receipt_sha256")?;
    let candidate_sha256 = string_field(receipt_object, "candidate_sha256")?;
    if receipt_path.is_empty() || receipt_sha256.len() != 64 || candidate_sha256.len() < 40 {
        return Err(ClosureError::Invalid(
            "closure receipt path or digest is invalid".to_owned(),
        ));
    }
    let object = boulder
        .as_object_mut()
        .ok_or_else(|| ClosureError::Invalid("boulder is not an object".to_owned()))?;
    reject_replayed_receipt(object)?;
    set_status_and_receipt(object, &receipt_path, &receipt_sha256, &candidate_sha256);
    let work_id = string_field(object, "active_work_id")?.to_owned();
    let works = object
        .get_mut("works")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| ClosureError::Invalid("works is not an object".to_owned()))?;
    let work = works
        .get_mut(&work_id)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| ClosureError::Invalid("active work is missing".to_owned()))?;
    set_work_receipt(work, &receipt_path, &receipt_sha256, &candidate_sha256);
    let temporary = boulder_path.with_extension("json.closure-tmp");
    let bytes = serde_json::to_vec_pretty(&boulder)
        .map_err(|error| ClosureError::Json(error.to_string()))?;
    std::fs::write(&temporary, bytes)
        .map_err(|error| ClosureError::Invalid(format!("write temporary Boulder: {error}")))?;
    std::fs::rename(&temporary, boulder_path)
        .map_err(|error| ClosureError::Invalid(format!("atomic Boulder rename: {error}")))
}

fn validate_receipt_shape(receipt: &Value) -> Result<(), ClosureError> {
    let object = object(receipt, "receipt")?;
    if string_field(object, "schema_version")? != "harness.tui-fidelity.closure-receipt.v1"
        || string_field(object, "status")? != "verified"
    {
        return Err(ClosureError::Invalid(
            "closure receipt is not verified".to_owned(),
        ));
    }
    for field_name in [
        "recovery_run_id",
        "reviewed_plan_sha256",
        "requirement_inventory_sha256",
        "coverage_manifest_sha256",
        "candidate_sha256",
        "evidence_root",
        "nonce",
    ] {
        if string_field(object, field_name)?.is_empty() {
            return Err(ClosureError::Invalid(format!(
                "receipt field {field_name} is empty"
            )));
        }
    }
    Ok(())
}

fn validate_mirror_identity(
    work: &Map<String, Value>,
    mirror: &Map<String, Value>,
) -> Result<(), ClosureError> {
    let work_status = string_field(work, "status")?;
    let mirror_status = string_field(mirror, "status")?;
    if work_status != mirror_status {
        return Err(ClosureError::Invalid("works/mirror divergence".to_owned()));
    }
    for field_name in ["active_plan", "plan_name", "session_ids"] {
        if field(work, field_name)? != field(mirror, field_name)? {
            return Err(ClosureError::Invalid("works/mirror divergence".to_owned()));
        }
    }
    Ok(())
}

fn set_status_and_receipt(
    object: &mut Map<String, Value>,
    receipt_path: &str,
    receipt_sha256: &str,
    candidate_sha256: &str,
) {
    object.insert("status".to_owned(), Value::String("completed".to_owned()));
    object.insert(
        "closure_receipt_path".to_owned(),
        Value::String(receipt_path.to_owned()),
    );
    object.insert(
        "closure_receipt_sha256".to_owned(),
        Value::String(receipt_sha256.to_owned()),
    );
    object.insert(
        "frozen_candidate_sha".to_owned(),
        Value::String(candidate_sha256.to_owned()),
    );
}

fn reject_replayed_receipt(object: &Map<String, Value>) -> Result<(), ClosureError> {
    for field_name in [
        "closure_receipt_path",
        "closure_receipt_sha256",
        "frozen_candidate_sha",
    ] {
        if object
            .get(field_name)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.is_empty())
        {
            return Err(ClosureError::Invalid(
                "closure receipt has already been consumed".to_owned(),
            ));
        }
    }
    let works = object
        .get("works")
        .and_then(Value::as_object)
        .ok_or_else(|| ClosureError::Invalid("works is not an object".to_owned()))?;
    if works.values().any(|value| {
        value.as_object().is_some_and(|work| {
            work.get("closure_receipt_sha256")
                .and_then(Value::as_str)
                .is_some_and(|digest| !digest.is_empty())
        })
    }) {
        return Err(ClosureError::Invalid(
            "a work already has a closure receipt".to_owned(),
        ));
    }
    Ok(())
}

fn set_work_receipt(
    work: &mut Map<String, Value>,
    receipt_path: &str,
    receipt_sha256: &str,
    candidate_sha256: &str,
) {
    work.insert("status".to_owned(), Value::String("completed".to_owned()));
    work.insert(
        "closure_receipt_path".to_owned(),
        Value::String(receipt_path.to_owned()),
    );
    work.insert(
        "closure_receipt_sha256".to_owned(),
        Value::String(receipt_sha256.to_owned()),
    );
    work.insert(
        "frozen_candidate_sha".to_owned(),
        Value::String(candidate_sha256.to_owned()),
    );
}

fn object<'a>(value: &'a Value, name: &str) -> Result<&'a Map<String, Value>, ClosureError> {
    value
        .as_object()
        .ok_or_else(|| ClosureError::Invalid(format!("{name} is not an object")))
}

fn field<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a Value, ClosureError> {
    object
        .get(name)
        .ok_or_else(|| ClosureError::Invalid(format!("missing field {name}")))
}

fn string<'a>(value: &'a Value, name: &str) -> Result<&'a str, ClosureError> {
    value
        .as_str()
        .ok_or_else(|| ClosureError::Invalid(format!("{name} is not a string")))
}

fn string_field<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a str, ClosureError> {
    string(field(object, name)?, name)
}
