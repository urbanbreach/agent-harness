use std::collections::BTreeSet;

use serde_json::{Map, Value};

const MAX_SCHEMA_DEPTH: usize = 32;

pub(super) fn sanitize(value: Value) -> Result<Value, &'static str> {
    let root = value.clone();
    sanitize_value(value, &root, 0, &mut Vec::new())
}

fn sanitize_value(
    value: Value,
    root: &Value,
    depth: usize,
    ref_stack: &mut Vec<String>,
) -> Result<Value, &'static str> {
    if depth > MAX_SCHEMA_DEPTH {
        return Err("schema nesting exceeds the supported depth");
    }
    match value {
        Value::Array(items) => items
            .into_iter()
            .map(|item| sanitize_value(item, root, depth + 1, ref_stack))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(map) => sanitize_object(map, root, depth, ref_stack),
        scalar => Ok(scalar),
    }
}

fn sanitize_object(
    mut schema: Map<String, Value>,
    root: &Value,
    depth: usize,
    ref_stack: &mut Vec<String>,
) -> Result<Value, &'static str> {
    if schema.contains_key("prefixItems") {
        return Err("`prefixItems` is not supported by the OpenAI tool schema subset");
    }

    if let Some(reference) = schema.remove("$ref") {
        let reference = reference
            .as_str()
            .ok_or("`$ref` must be a local string reference")?;
        let pointer = reference
            .strip_prefix('#')
            .filter(|pointer| pointer.starts_with('/'))
            .ok_or("only local JSON Pointer `$ref` values are supported")?;
        if ref_stack.iter().any(|active| active == reference) {
            return Err("cyclic `$ref` is not supported by the OpenAI tool schema subset");
        }
        let resolved = root
            .pointer(pointer)
            .cloned()
            .ok_or("`$ref` target does not exist")?;
        if schema.keys().any(|key| !is_annotation(key)) {
            return Err("structural siblings next to `$ref` are not supported");
        }
        ref_stack.push(reference.to_string());
        let mut resolved = sanitize_value(resolved, root, depth + 1, ref_stack)?;
        ref_stack.pop();
        let resolved_object = resolved
            .as_object_mut()
            .ok_or("`$ref` target must be an object schema")?;
        resolved_object.extend(schema);
        return Ok(resolved);
    }

    let alternatives = take_alternatives(&mut schema)?;
    let all_of = schema.remove("allOf");
    if alternatives.is_some() && all_of.is_some() {
        return Err("mixed schema combinators are not supported");
    }

    for key in ["$defs", "$schema", "definitions", "not"] {
        schema.remove(key);
    }
    schema = schema
        .into_iter()
        .map(|(key, value)| {
            sanitize_value(value, root, depth + 1, ref_stack).map(|value| (key, value))
        })
        .collect::<Result<_, _>>()?;

    normalize_type(&mut schema)?;
    normalize_items(&mut schema)?;

    if let Some(alternatives) = alternatives {
        let selected = select_narrow_alternative(alternatives, root, depth, ref_stack)?;
        merge_schema(&mut schema, selected)?;
    }
    if let Some(all_of) = all_of {
        let fragments = all_of
            .as_array()
            .ok_or("`allOf` must be an array")?
            .iter()
            .cloned()
            .map(|fragment| sanitize_value(fragment, root, depth + 1, ref_stack))
            .collect::<Result<Vec<_>, _>>()?;
        for fragment in fragments {
            merge_schema(&mut schema, fragment)?;
        }
    }

    filter_required(&mut schema);
    Ok(Value::Object(schema))
}

fn is_annotation(key: &str) -> bool {
    matches!(
        key,
        "default" | "deprecated" | "description" | "examples" | "title"
    )
}

fn take_alternatives(schema: &mut Map<String, Value>) -> Result<Option<Vec<Value>>, &'static str> {
    let one_of = schema.remove("oneOf");
    let any_of = schema.remove("anyOf");
    if one_of.is_some() && any_of.is_some() {
        return Err("mixed `oneOf` and `anyOf` are not supported");
    }
    one_of
        .or(any_of)
        .map(|value| {
            value
                .as_array()
                .cloned()
                .filter(|items| !items.is_empty())
                .ok_or("schema alternatives must be a non-empty array")
        })
        .transpose()
}

fn select_narrow_alternative(
    alternatives: Vec<Value>,
    root: &Value,
    depth: usize,
    ref_stack: &mut Vec<String>,
) -> Result<Value, &'static str> {
    let sanitized = alternatives
        .into_iter()
        .map(|value| sanitize_value(value, root, depth + 1, ref_stack))
        .collect::<Result<Vec<_>, _>>()?;
    sanitized
        .iter()
        .find(|value| value.get("type").and_then(Value::as_str) == Some("object"))
        .or_else(|| {
            sanitized
                .iter()
                .find(|value| value.get("type").and_then(Value::as_str) != Some("null"))
        })
        .cloned()
        .ok_or("schema alternatives contain no representable branch")
}

fn normalize_type(schema: &mut Map<String, Value>) -> Result<(), &'static str> {
    let Some(Value::Array(types)) = schema.get("type").cloned() else {
        return Ok(());
    };
    let non_null = types
        .into_iter()
        .filter_map(|value| value.as_str().map(str::to_string))
        .filter(|value| value != "null")
        .collect::<BTreeSet<_>>();
    let selected = non_null
        .into_iter()
        .next()
        .ok_or("a type union containing only `null` is not supported")?;
    schema.insert("type".to_string(), Value::String(selected));
    Ok(())
}

fn normalize_items(schema: &mut Map<String, Value>) -> Result<(), &'static str> {
    let Some(Value::Array(items)) = schema.get("items").cloned() else {
        return Ok(());
    };
    if items.len() != 1 {
        return Err("heterogeneous tuple `items` are not supported");
    }
    schema.insert(
        "items".to_string(),
        items.into_iter().next().unwrap_or(Value::Null),
    );
    Ok(())
}

fn merge_schema(target: &mut Map<String, Value>, source: Value) -> Result<(), &'static str> {
    let source = source
        .as_object()
        .ok_or("schema combinator branch must be an object schema")?;
    for (key, value) in source {
        match key.as_str() {
            "properties" => merge_properties(target, value)?,
            "required" => merge_required(target, value)?,
            _ if is_annotation(key) => {
                target.entry(key.clone()).or_insert_with(|| value.clone());
            }
            _ => match target.get(key) {
                None => {
                    target.insert(key.clone(), value.clone());
                }
                Some(existing) if existing == value => {}
                Some(_) => return Err("schema branches contain conflicting constraints"),
            },
        }
    }
    Ok(())
}

fn merge_properties(target: &mut Map<String, Value>, value: &Value) -> Result<(), &'static str> {
    let incoming = value.as_object().ok_or("`properties` must be an object")?;
    let properties = target
        .entry("properties".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("`properties` must be an object")?;
    for (name, schema) in incoming {
        match properties.get(name) {
            None => {
                properties.insert(name.clone(), schema.clone());
            }
            Some(existing) if existing == schema => {}
            Some(_) => return Err("schema branches define a property incompatibly"),
        }
    }
    Ok(())
}

fn merge_required(target: &mut Map<String, Value>, value: &Value) -> Result<(), &'static str> {
    let incoming = value.as_array().ok_or("`required` must be an array")?;
    let required = target
        .entry("required".to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or("`required` must be an array")?;
    for field in incoming {
        if !required.contains(field) {
            required.push(field.clone());
        }
    }
    Ok(())
}

fn filter_required(schema: &mut Map<String, Value>) {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return;
    };
    let property_names = properties.keys().cloned().collect::<BTreeSet<_>>();
    if let Some(Value::Array(required)) = schema.get_mut("required") {
        required.retain(|field| {
            field
                .as_str()
                .is_some_and(|name| property_names.contains(name))
        });
    }
}
