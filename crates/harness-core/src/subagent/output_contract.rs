//! Caller-provided final-answer contracts, compiled once per native registration.
use serde_json::{Map, Value};
use std::sync::Arc;

const MAX_ERROR_CHARS: usize = 512;

pub(crate) fn compile(schema: &Map<String, Value>) -> Result<Arc<jsonschema::Validator>, String> {
    let schema = Value::Object(schema.clone());
    check_complexity(&schema, &schema, 0, &mut 0)?;
    jsonschema::validator_for(&schema)
        .map(Arc::new)
        .map_err(|error| format!("Invalid output_schema: {error}"))
}

// Count expanded references as well as literal nodes before the validator compiles.
// This also bounds recursive schemas and branching allOf/anyOf expansion.
fn check_complexity(
    root: &Value,
    value: &Value,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), String> {
    *nodes += 1;
    if *nodes > 4096 || depth > 64 {
        return Err(
            "Invalid output_schema: exceeds 4096 expanded nodes or 64 nesting levels".into(),
        );
    }
    match value {
        Value::Object(object) => {
            if object.contains_key("$dynamicRef")
                || object.contains_key("$recursiveRef")
                || (depth > 0 && object.contains_key("$id"))
            {
                return Err("Invalid output_schema: dynamic references and nested resource ids are unsupported".into());
            }
            if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                let pointer = reference
                    .strip_prefix('#')
                    .filter(|pointer| pointer.is_empty() || pointer.starts_with('/'))
                    .ok_or_else(|| {
                        "Invalid output_schema: references must be local JSON pointers".to_owned()
                    })?;
                let target = root.pointer(pointer).ok_or_else(|| {
                    format!("Invalid output_schema: unresolved reference {reference}")
                })?;
                check_complexity(root, target, depth + 1, nodes)?;
            }
            for value in object.values() {
                check_complexity(root, value, depth + 1, nodes)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                check_complexity(root, value, depth + 1, nodes)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn instructions(schema: &Map<String, Value>) -> String {
    format!(
        "Final answer contract: return a single JSON object matching this JSON Schema. Return only the JSON object, without prose or markdown fences.\n{}",
        Value::Object(schema.clone())
    )
}

pub(crate) fn validate(
    validator: Option<&jsonschema::Validator>,
    text: &str,
) -> (Option<Value>, Vec<String>) {
    let Some(validator) = validator else {
        return (None, Vec::new());
    };
    let value = match extract(text) {
        Ok(value) if value.is_object() => value,
        Ok(_) => return (None, vec!["The final answer must be a JSON object.".into()]),
        Err(error) => return (None, vec![error]),
    };
    match validator.validate(&value) {
        Ok(()) => (Some(value), Vec::new()),
        Err(error) => {
            let problem = format!("{}: {error}", error.instance_path())
                .chars()
                .take(MAX_ERROR_CHARS)
                .collect();
            (None, vec![problem])
        }
    }
}

fn extract(text: &str) -> Result<Value, String> {
    let text = text.trim();
    if let Ok(value) = serde_json::from_str(text) {
        return Ok(value);
    }
    if let Some((_, fenced)) = text.split_once("```json")
        && let Some((json, _)) = fenced.split_once("```")
    {
        return serde_json::from_str(json.trim())
            .map_err(|error| format!("Invalid JSON in the json fence: {error}"));
    }
    let start = text
        .find('{')
        .ok_or_else(|| "No JSON object found in the final answer.".to_owned())?;
    let mut depth = 0_usize;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, ch) in text[start..].char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
        } else {
            match ch {
                '"' => quoted = true,
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return serde_json::from_str(&text[start..=start + offset])
                            .map_err(|error| format!("Invalid JSON object: {error}"));
                    }
                }
                _ => {}
            }
        }
    }
    Err("The final answer contains an incomplete JSON object.".into())
}
