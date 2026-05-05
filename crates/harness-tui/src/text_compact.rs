pub(crate) fn compact_payload(
    payload: &str,
    max_fields: usize,
    max_chars: usize,
) -> Option<String> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return None;
    }

    let compact = match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) => compact_json_value(&value, max_fields),
        Err(_) => collapse_whitespace(trimmed),
    };

    Some(truncate_chars(&compact, max_chars))
}

fn compact_json_value(value: &serde_json::Value, max_fields: usize) -> String {
    match value {
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                return "{}".to_string();
            }

            let mut parts = map
                .iter()
                .take(max_fields)
                .map(|(key, value)| format!("{key}={}", compact_json_leaf(value)))
                .collect::<Vec<_>>();
            if map.len() > max_fields {
                parts.push("…".to_string());
            }
            parts.join(", ")
        }
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                return "[]".to_string();
            }

            let mut parts = items
                .iter()
                .take(max_fields)
                .map(compact_json_leaf)
                .collect::<Vec<_>>();
            if items.len() > max_fields {
                parts.push("…".to_string());
            }
            format!("[{}]", parts.join(", "))
        }
        _ => compact_json_leaf(value),
    }
}

fn compact_json_leaf(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => collapse_whitespace(text),
        serde_json::Value::Number(number) => number.to_string(),
        serde_json::Value::Bool(flag) => flag.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Array(items) => format!(
            "[{} item{}]",
            items.len(),
            if items.len() == 1 { "" } else { "s" }
        ),
        serde_json::Value::Object(fields) => format!(
            "{{{} field{}}}",
            fields.len(),
            if fields.len() == 1 { "" } else { "s" }
        ),
    }
}

fn collapse_whitespace(text: &str) -> String {
    let mut parts = text.split_whitespace();
    let Some(first) = parts.next() else {
        return String::new();
    };

    let mut compact = String::from(first);
    for part in parts {
        compact.push(' ');
        compact.push_str(part);
    }
    compact
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let truncated = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    format!("{truncated}…")
}
