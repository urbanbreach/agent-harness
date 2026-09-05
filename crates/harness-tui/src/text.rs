pub(crate) fn has_trimmed_content(value: &str) -> bool {
    non_empty_trimmed(value).is_some()
}

pub(crate) fn non_empty_trimmed(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

pub(crate) fn non_empty_preserved_string(value: &str) -> Option<String> {
    has_trimmed_content(value).then(|| value.to_string())
}

pub(crate) fn replace_control_chars_except_tabs(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() && character != '\t' {
                ' '
            } else {
                character
            }
        })
        .collect()
}

pub(crate) fn collapse_inline_whitespace(text: &str) -> String {
    replace_control_chars_except_tabs(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnsiStripState {
    Text,
    Escape,
    Csi,
    Osc,
    OscEscape,
    StringControl,
    StringControlEscape,
}

pub(crate) fn strip_ansi_escapes(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut state = AnsiStripState::Text;
    for character in text.chars() {
        match state {
            AnsiStripState::Text => {
                if character == '\u{1b}' {
                    state = AnsiStripState::Escape;
                } else {
                    output.push(character);
                }
            }
            AnsiStripState::Escape => match character {
                '[' => state = AnsiStripState::Csi,
                ']' => state = AnsiStripState::Osc,
                'P' | '^' | '_' => state = AnsiStripState::StringControl,
                _ => state = AnsiStripState::Text,
            },
            AnsiStripState::Csi => {
                if ('@'..='~').contains(&character) {
                    state = AnsiStripState::Text;
                }
            }
            AnsiStripState::Osc => match character {
                '\u{7}' => state = AnsiStripState::Text,
                '\u{1b}' => state = AnsiStripState::OscEscape,
                _ => {}
            },
            AnsiStripState::OscEscape => {
                state = if character == '\\' {
                    AnsiStripState::Text
                } else {
                    AnsiStripState::Osc
                };
            }
            AnsiStripState::StringControl => {
                if character == '\u{1b}' {
                    state = AnsiStripState::StringControlEscape;
                }
            }
            AnsiStripState::StringControlEscape => {
                state = if character == '\\' {
                    AnsiStripState::Text
                } else {
                    AnsiStripState::StringControl
                };
            }
        }
    }
    output
}

pub(crate) fn trimmed_json_string_field(
    value: Option<&serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    let object = value?.as_object()?;
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .and_then(non_empty_trimmed)
            .map(str::to_string)
    })
}

pub(crate) fn trimmed_json_nested_string_field(
    value: Option<&serde_json::Value>,
    path: &[&str],
) -> Option<String> {
    let mut current = value?;
    for key in path {
        current = current.get(*key)?;
    }
    current
        .as_str()
        .and_then(non_empty_trimmed)
        .map(str::to_string)
}

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

#[cfg(test)]
mod tests {
    use super::{collapse_inline_whitespace, compact_payload, strip_ansi_escapes};

    #[test]
    fn collapse_inline_whitespace_replaces_control_chars_and_collapses_runs() {
        assert_eq!(
            collapse_inline_whitespace("  alpha\n\u{7} beta\t gamma  "),
            "alpha beta gamma"
        );
    }

    #[test]
    fn strip_ansi_escapes_removes_csi_osc_and_string_controls() {
        assert_eq!(
            strip_ansi_escapes(
                "\u{1b}[31mred\u{1b}[0m\n\u{1b}]0;title\u{7}plain\u{1b}Phidden\u{1b}\\done"
            ),
            "red\nplaindone"
        );
    }

    #[test]
    fn compact_payload_formats_json_objects_and_caps_fields() {
        assert_eq!(
            compact_payload(
                r#"{"alpha":"two words","beta":[1,2],"gamma":{"nested":true}}"#,
                2,
                80,
            )
            .as_deref(),
            Some("alpha=two words, beta=[2 items], …")
        );
    }

    #[test]
    fn compact_payload_collapses_plain_text_and_truncates() {
        assert_eq!(
            compact_payload("  alpha\n beta\t gamma  ", 4, 12).as_deref(),
            Some("alpha beta …")
        );
    }

    #[test]
    fn compact_payload_omits_blank_payloads() {
        assert_eq!(compact_payload(" \n\t ", 4, 12), None);
    }
}
