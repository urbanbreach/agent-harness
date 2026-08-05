use serde_json::{Map, Value};

use super::super::TaskGateError;

pub(super) fn is_digest(value: &str) -> bool {
    (value.len() == 40 || value.len() == 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn value_object<'a>(
    value: &'a Value,
    name: &str,
) -> Result<&'a Map<String, Value>, TaskGateError> {
    value
        .get(name)
        .and_then(Value::as_object)
        .ok_or_else(|| TaskGateError::Invalid(format!("{name} is not an object")))
}

pub(crate) fn value_string<'a>(value: &'a Value, name: &str) -> Result<&'a str, TaskGateError> {
    value
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| TaskGateError::Invalid(format!("{name} is not a string")))
}
