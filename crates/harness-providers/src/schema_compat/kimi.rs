use serde_json::{Map, Value};

pub(super) fn sanitize(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(sanitize).collect()),
        Value::Object(map) => sanitize_object(map),
        other => other,
    }
}

fn sanitize_object(map: Map<String, Value>) -> Value {
    if let Some(Value::String(reference)) = map.get("$ref") {
        let mut result = Map::new();
        result.insert("$ref".to_string(), Value::String(reference.clone()));
        return Value::Object(result);
    }

    let mut result = map
        .into_iter()
        .map(|(key, value)| (key, sanitize(value)))
        .collect::<Map<_, _>>();
    if let Some(Value::Array(items)) = result.remove("items") {
        result.insert(
            "items".to_string(),
            items
                .into_iter()
                .next()
                .unwrap_or_else(|| Value::Object(Map::new())),
        );
    }
    Value::Object(result)
}
