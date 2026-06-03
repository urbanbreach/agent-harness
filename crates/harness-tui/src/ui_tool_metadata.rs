use crate::text::{collapse_inline_whitespace, non_empty_trimmed};

pub(super) fn tool_json_string(
    output_json: Option<&serde_json::Value>,
    keys: &[&str],
) -> Option<String> {
    let object = output_json?.as_object()?;
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .and_then(collapsed_inline_non_empty)
    })
}

pub(super) fn tool_json_string_ref<'a>(
    output_json: Option<&'a serde_json::Value>,
    keys: &[&str],
) -> Option<&'a str> {
    let object = output_json?.as_object()?;
    keys.iter().find_map(|key| {
        object
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .and_then(non_empty_trimmed)
    })
}

fn tool_json_nested_string_ref<'a>(
    output_json: Option<&'a serde_json::Value>,
    path: &[&str],
) -> Option<&'a str> {
    let mut current = output_json?;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str().and_then(non_empty_trimmed)
}

pub(super) fn task_tool_child_session_id_from_output(
    output_json: Option<&serde_json::Value>,
) -> Option<&str> {
    tool_json_string_ref(
        output_json,
        &[
            "child_session_id",
            "session_id",
            "task_id",
            "childSessionId",
            "sessionId",
            "taskId",
        ],
    )
    .or_else(|| tool_json_nested_string_ref(output_json, &["child_session", "session_id"]))
    .or_else(|| tool_json_nested_string_ref(output_json, &["childSession", "sessionId"]))
    .or_else(|| tool_json_nested_string_ref(output_json, &["metadata", "sessionId"]))
    .or_else(|| tool_json_nested_string_ref(output_json, &["metadata", "session_id"]))
    .or_else(|| tool_json_nested_string_ref(output_json, &["lineage", "child_session_id"]))
    .or_else(|| tool_json_nested_string_ref(output_json, &["lineage", "session_id"]))
    .or_else(|| tool_json_nested_string_ref(output_json, &["lineage", "sessionId"]))
    .or_else(|| {
        tool_json_nested_string_ref(output_json, &["_harness", "lineage", "child_session_id"])
    })
    .or_else(|| tool_json_nested_string_ref(output_json, &["_harness", "lineage", "session_id"]))
    .or_else(|| tool_json_nested_string_ref(output_json, &["_harness", "lineage", "sessionId"]))
}

pub(super) fn task_tool_child_request_id_from_output(
    output_json: Option<&serde_json::Value>,
) -> Option<&str> {
    tool_json_string_ref(
        output_json,
        &[
            "child_request_id",
            "request_id",
            "childRequestId",
            "requestId",
        ],
    )
    .or_else(|| tool_json_nested_string_ref(output_json, &["child_session", "request_id"]))
    .or_else(|| tool_json_nested_string_ref(output_json, &["childSession", "requestId"]))
    .or_else(|| tool_json_nested_string_ref(output_json, &["metadata", "requestId"]))
    .or_else(|| tool_json_nested_string_ref(output_json, &["metadata", "request_id"]))
    .or_else(|| tool_json_nested_string_ref(output_json, &["lineage", "child_request_id"]))
    .or_else(|| tool_json_nested_string_ref(output_json, &["lineage", "request_id"]))
    .or_else(|| tool_json_nested_string_ref(output_json, &["lineage", "requestId"]))
    .or_else(|| {
        tool_json_nested_string_ref(output_json, &["_harness", "lineage", "child_request_id"])
    })
    .or_else(|| tool_json_nested_string_ref(output_json, &["_harness", "lineage", "request_id"]))
    .or_else(|| tool_json_nested_string_ref(output_json, &["_harness", "lineage", "requestId"]))
}

pub(super) fn tool_json_nested_string(
    output_json: Option<&serde_json::Value>,
    path: &[&str],
) -> Option<String> {
    tool_json_nested_string_ref(output_json, path).and_then(collapsed_inline_non_empty)
}

pub(super) fn tool_summary_string(args_summary: &str, keys: &[&str]) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(args_summary) {
        if let Some(object) = value.as_object() {
            if let Some(parsed) = keys.iter().find_map(|key| {
                object
                    .get(*key)
                    .and_then(serde_json::Value::as_str)
                    .map(collapse_inline_whitespace)
                    .filter(|value| !value.is_empty())
            }) {
                return Some(parsed);
            }
        }
    }

    tool_summary_string_fragment(args_summary, keys)
}

fn tool_summary_string_fragment(args_summary: &str, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| tool_summary_fragment_for_key(args_summary, key))
}

fn tool_summary_fragment_for_key(args_summary: &str, key: &str) -> Option<String> {
    let marker = format!("\"{key}\"");
    let marker_index = args_summary.find(&marker)?;
    let mut rest = args_summary[marker_index + marker.len()..]
        .chars()
        .peekable();
    while rest.peek().is_some_and(|ch| ch.is_whitespace()) {
        rest.next();
    }
    if rest.next()? != ':' {
        return None;
    }
    while rest.peek().is_some_and(|ch| ch.is_whitespace()) {
        rest.next();
    }
    if rest.next()? != '"' {
        return None;
    }

    let mut value = String::new();
    let mut escaped = false;
    for ch in rest {
        if escaped {
            match ch {
                'n' | 'r' | 't' => value.push(' '),
                '"' | '\\' | '/' => value.push(ch),
                _ => value.push(ch),
            }
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => break,
            '…' => break,
            _ => value.push(ch),
        }
    }

    Some(collapse_inline_whitespace(&value)).filter(|value| !value.is_empty())
}

pub(super) fn tool_summary_number(args_summary: &str, keys: &[&str]) -> Option<u64> {
    let value = serde_json::from_str::<serde_json::Value>(args_summary).ok()?;
    let object = value.as_object()?;
    keys.iter()
        .find_map(|key| object.get(*key))
        .and_then(serde_json::Value::as_u64)
}

fn collapsed_inline_non_empty(text: &str) -> Option<String> {
    let collapsed = collapse_inline_whitespace(text);
    (!collapsed.is_empty()).then_some(collapsed)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        task_tool_child_request_id_from_output, task_tool_child_session_id_from_output,
        tool_json_nested_string, tool_json_string, tool_json_string_ref, tool_summary_number,
        tool_summary_string,
    };

    #[test]
    fn tool_json_string_prefers_first_non_empty_string_and_collapses_whitespace() {
        let value = json!({"primary": "  alpha\nbeta  ", "fallback": "ignored"});

        assert_eq!(
            tool_json_string(Some(&value), &["primary", "fallback"]),
            Some("alpha beta".to_string())
        );
    }

    #[test]
    fn tool_json_string_ref_preserves_inner_whitespace_but_trims_edges() {
        let value = json!({"request_id": "  req  42  "});

        assert_eq!(
            tool_json_string_ref(Some(&value), &["request_id"]),
            Some("req  42")
        );
    }

    #[test]
    fn task_child_ids_accept_top_level_and_nested_compat_shapes() {
        let top_level = json!({"child_request_id": " req-1 ", "sessionId": " ses-1 "});
        let nested = json!({
            "_harness": {
                "lineage": {
                    "child_request_id": "req-2",
                    "child_session_id": "ses-2"
                }
            }
        });

        assert_eq!(
            task_tool_child_request_id_from_output(Some(&top_level)),
            Some("req-1")
        );
        assert_eq!(
            task_tool_child_session_id_from_output(Some(&top_level)),
            Some("ses-1")
        );
        assert_eq!(
            task_tool_child_request_id_from_output(Some(&nested)),
            Some("req-2")
        );
        assert_eq!(
            task_tool_child_session_id_from_output(Some(&nested)),
            Some("ses-2")
        );
    }

    #[test]
    fn tool_json_nested_string_collapses_nested_string_values() {
        let value = json!({"error": {"message": " failed\n hard "}});

        assert_eq!(
            tool_json_nested_string(Some(&value), &["error", "message"]),
            Some("failed hard".to_string())
        );
    }

    #[test]
    fn tool_summary_string_reads_valid_json_and_truncated_string_fragments() {
        assert_eq!(
            tool_summary_string(r#"{"query":"  find\nneedle  "}"#, &["query"]),
            Some("find needle".to_string())
        );
        assert_eq!(
            tool_summary_string(r#"{"description":"first\nsecond…"#, &["description"]),
            Some("first second".to_string())
        );
    }

    #[test]
    fn tool_summary_number_reads_unsigned_json_numbers() {
        assert_eq!(
            tool_summary_number(r#"{"offset":2,"limit":5}"#, &["start", "limit"]),
            Some(5)
        );
    }
}
