use serde_json::{Map, Value};

pub(super) fn sanitize(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(sanitize).collect()),
        Value::Object(map) => sanitize_object(map),
        other => other,
    }
}

fn sanitize_object(map: Map<String, Value>) -> Value {
    let mut result = map
        .into_iter()
        .map(|(key, value)| {
            let sanitized = if value.is_object() || value.is_array() {
                sanitize(value)
            } else {
                value
            };
            (key, sanitized)
        })
        .collect::<Map<_, _>>();

    stringify_enum(&mut result);
    split_type_array(&mut result);
    filter_required(&mut result);
    default_array_items(&mut result);
    remove_non_object_fields(&mut result);
    Value::Object(result)
}

fn stringify_enum(schema: &mut Map<String, Value>) {
    if let Some(Value::Array(values)) = schema.get_mut("enum") {
        for value in values {
            if !value.is_string() {
                *value = Value::String(value_to_string(value));
            }
        }
        if matches!(
            schema.get("type").and_then(Value::as_str),
            Some("integer" | "number")
        ) {
            schema.insert("type".to_string(), Value::String("string".to_string()));
        }
    }
}

fn split_type_array(schema: &mut Map<String, Value>) {
    let Some(Value::Array(types)) = schema.get("type") else {
        return;
    };
    let types = types.clone();
    schema.remove("type");
    let has_null = types.iter().any(|value| value.as_str() == Some("null"));
    let non_null = types
        .into_iter()
        .filter(|value| value.as_str() != Some("null"))
        .collect::<Vec<_>>();
    if non_null.is_empty() {
        schema.insert("type".to_string(), Value::String("null".to_string()));
        return;
    }
    schema.insert(
        "anyOf".to_string(),
        Value::Array(
            non_null
                .into_iter()
                .map(|value| {
                    let mut item = Map::new();
                    item.insert("type".to_string(), value);
                    Value::Object(item)
                })
                .collect(),
        ),
    );
    if has_null {
        schema.insert("nullable".to_string(), Value::Bool(true));
    }
}

fn filter_required(schema: &mut Map<String, Value>) {
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return;
    }
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return;
    };
    let property_names = properties
        .keys()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(Value::Array(required)) = schema.get_mut("required") {
        required.retain(|field| {
            field
                .as_str()
                .is_some_and(|name| property_names.contains(name))
        });
    }
}

fn default_array_items(schema: &mut Map<String, Value>) {
    if schema.get("type").and_then(Value::as_str) != Some("array") || has_combiner(schema) {
        return;
    }
    if !schema.contains_key("items") {
        schema.insert("items".to_string(), Value::Object(Map::new()));
    }
    if let Some(Value::Object(items)) = schema.get_mut("items") {
        if !has_schema_intent(items) {
            items.insert("type".to_string(), Value::String("string".to_string()));
        }
    }
}

fn remove_non_object_fields(schema: &mut Map<String, Value>) {
    if schema.get("type").and_then(Value::as_str) == Some("object")
        || !schema.contains_key("type")
        || has_combiner(schema)
    {
        return;
    }
    schema.remove("properties");
    schema.remove("required");
}

fn has_combiner(schema: &Map<String, Value>) -> bool {
    ["anyOf", "oneOf", "allOf"]
        .iter()
        .any(|key| schema.get(*key).is_some_and(Value::is_array))
}

fn has_schema_intent(schema: &Map<String, Value>) -> bool {
    has_combiner(schema)
        || [
            "type",
            "properties",
            "items",
            "prefixItems",
            "enum",
            "const",
            "$ref",
            "additionalProperties",
            "patternProperties",
            "required",
            "not",
            "if",
            "then",
            "else",
        ]
        .iter()
        .any(|key| schema.contains_key(*key))
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}
