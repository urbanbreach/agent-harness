use reqwest::StatusCode;
use serde_json::{json, Value};

use crate::text::{has_trimmed_content, trimmed_non_empty};

pub(crate) fn render_list_output<F>(title: &str, items: &[Value], render: F) -> String
where
    F: Fn(&Value) -> String,
{
    if items.is_empty() {
        format!("{title}: none")
    } else {
        let lines = items.iter().map(render).collect::<Vec<_>>().join("\n");
        format!("{title}\n{lines}")
    }
}

pub(crate) fn render_content_entries(content: Option<&Vec<Value>>) -> Vec<String> {
    content
        .into_iter()
        .flat_map(|entries| entries.iter())
        .filter_map(render_content_entry)
        .collect()
}

fn render_content_entry(entry: &Value) -> Option<String> {
    match entry.get("type").and_then(Value::as_str) {
        Some("text") => entry
            .get("text")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        Some("image") => Some(format!(
            "[image {}]",
            entry
                .get("mimeType")
                .and_then(Value::as_str)
                .unwrap_or("unknown mime")
        )),
        Some("resource") => entry
            .get("resource")
            .and_then(render_resource_entry)
            .or_else(|| Some(compact_json(entry))),
        _ => Some(compact_json(entry)),
    }
}

pub(crate) fn render_resource_contents(contents: &[Value]) -> String {
    if contents.is_empty() {
        return "MCP resource returned no contents".to_string();
    }

    contents
        .iter()
        .map(|entry| render_resource_entry(entry).unwrap_or_else(|| compact_json(entry)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_resource_entry(entry: &Value) -> Option<String> {
    if let Some(text) = entry.get("text").and_then(Value::as_str) {
        return Some(text.to_string());
    }
    if let Some(blob) = entry.get("blob").and_then(Value::as_str) {
        let uri = entry
            .get("uri")
            .and_then(Value::as_str)
            .unwrap_or("resource");
        let mime = entry
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream");
        return Some(format!(
            "[binary resource {uri} ({mime}, {} base64 chars)]",
            blob.len()
        ));
    }
    entry
        .get("uri")
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

pub(crate) fn render_prompt_messages(messages: &[Value]) -> String {
    if messages.is_empty() {
        return "MCP prompt returned no messages".to_string();
    }

    messages
        .iter()
        .map(|message| {
            let role = message
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("message");
            let content = message
                .get("content")
                .and_then(Value::as_array)
                .map(|entries| render_content_entries(Some(entries)).join("\n"))
                .filter(|text| has_trimmed_content(text))
                .unwrap_or_else(|| compact_json(message));
            format!("{role}: {content}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn normalize_object_value(value: Value) -> Value {
    match value {
        Value::Null => Value::Object(serde_json::Map::new()),
        Value::Object(_) => value,
        other => json!({ "value": other }),
    }
}

fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn jsonrpc_error_message(error: &Value) -> String {
    let code = error
        .get("code")
        .and_then(Value::as_i64)
        .map(|value| value.to_string());
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .map(normalize_mcp_error_message)
        .unwrap_or_else(|| compact_json(error));
    match code {
        Some(code) => format!("{message} (code {code})"),
        None => message,
    }
}

pub(crate) fn render_mcp_http_parse_error(body: &str, err: &serde_json::Error) -> String {
    match describe_upstream_non_json_response(body) {
        Some(message) => format!("failed to parse MCP HTTP response: {message}"),
        None => format!("failed to parse MCP HTTP response: {err}"),
    }
}

pub(crate) fn render_mcp_http_status_error(
    status: StatusCode,
    headers: &reqwest::header::HeaderMap,
    body: &str,
) -> String {
    let status_prefix = format!("MCP HTTP request failed with status {status}");

    if status == StatusCode::TOO_MANY_REQUESTS {
        let retry_after = retry_after_hint(headers)
            .map(|value| format!("; retry-after {value}"))
            .unwrap_or_default();
        let detail = extract_upstream_error_detail(body)
            .or_else(|| describe_upstream_non_json_response(body))
            .unwrap_or_else(|| "upstream service rate limited the request".to_string());
        return format!("{status_prefix}: {detail}{retry_after}");
    }

    if let Some(detail) =
        extract_upstream_error_detail(body).or_else(|| describe_upstream_non_json_response(body))
    {
        return format!("{status_prefix}: {detail}");
    }

    status_prefix
}

pub(crate) fn normalize_mcp_error_message(message: &str) -> String {
    describe_upstream_non_json_response(message).unwrap_or_else(|| collapse_whitespace(message))
}

fn extract_upstream_error_detail(body: &str) -> Option<String> {
    let trimmed = trimmed_non_empty(body)?;

    let value: Value = serde_json::from_str(trimmed).ok()?;
    if let Some(error) = value.get("error") {
        return Some(jsonrpc_error_message(error));
    }
    for field in ["message", "detail", "error_description", "error"] {
        if let Some(message) = value.get(field).and_then(Value::as_str) {
            let collapsed = collapse_whitespace(message);
            if !collapsed.is_empty() {
                return Some(collapsed);
            }
        }
    }
    None
}

pub(crate) fn describe_upstream_non_json_response(body: &str) -> Option<String> {
    let collapsed = collapse_whitespace(body);
    if collapsed.is_empty() {
        return None;
    }

    let snippet = truncated_snippet(&collapsed, 160);
    let lower = collapsed.to_ascii_lowercase();
    let looks_like_non_json = lower.contains("unexpected token")
        || lower.contains("not valid json")
        || lower.contains("failed to parse");
    let looks_like_too_many_requests = lower.contains("too many requests")
        || lower.contains("too many request")
        || lower.contains("too many r");
    let looks_like_rate_limit = looks_like_too_many_requests || lower.contains("rate limit");
    let looks_like_html =
        lower.contains("<html") || lower.contains("<!doctype html") || lower.contains("<body");

    if looks_like_too_many_requests {
        return Some(
            "upstream service returned a non-JSON rate-limit response (Too Many Requests)"
                .to_string(),
        );
    }
    if looks_like_rate_limit {
        return Some(format!(
            "upstream service rate limited the request: {snippet}"
        ));
    }
    if looks_like_html {
        return Some(format!(
            "upstream service returned HTML instead of JSON: {snippet}"
        ));
    }
    if looks_like_non_json {
        return Some(format!(
            "upstream service returned non-JSON content: {snippet}"
        ));
    }
    None
}

fn retry_after_hint(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(trimmed_non_empty)
        .map(ToString::to_string)
}

fn truncated_snippet(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::{
        describe_upstream_non_json_response, normalize_mcp_error_message,
        render_mcp_http_parse_error, render_mcp_http_status_error,
    };
    use reqwest::{header::HeaderMap, StatusCode};
    use serde_json::Value;

    #[test]
    fn mcp_error_normalization_marks_rate_limited_non_json_errors() {
        let message = normalize_mcp_error_message(
            "Unexpected token 'T', \"Too Many R\"... is not valid JSON",
        );
        assert_eq!(
            message,
            "upstream service returned a non-JSON rate-limit response (Too Many Requests)"
        );
    }

    #[test]
    fn mcp_http_parse_error_uses_body_context_for_non_json_responses() {
        let err = serde_json::from_str::<Value>("Too Many Requests")
            .expect_err("plain text should not parse as json");
        let message = render_mcp_http_parse_error("Too Many Requests", &err);
        assert!(message.contains("non-JSON"));
        assert!(message.contains("Too Many Requests"));
    }

    #[test]
    fn mcp_non_json_description_ignores_normal_text() {
        assert!(describe_upstream_non_json_response("transient upstream issue").is_none());
    }

    #[test]
    fn mcp_http_status_error_extracts_jsonrpc_body_message() {
        let message = render_mcp_http_status_error(
            StatusCode::BAD_GATEWAY,
            &HeaderMap::new(),
            r#"{"error":{"code":-32000,"message":"backend unavailable"}}"#,
        );
        assert!(message.contains("502 Bad Gateway"));
        assert!(message.contains("backend unavailable"));
        assert!(message.contains("code -32000"));
    }

    #[test]
    fn mcp_http_status_error_marks_rate_limits_and_retry_after() {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::RETRY_AFTER,
            "12".parse().expect("retry-after"),
        );

        let message = render_mcp_http_status_error(
            StatusCode::TOO_MANY_REQUESTS,
            &headers,
            "<html><body>Too Many Requests</body></html>",
        );
        assert!(message.contains("429 Too Many Requests"));
        assert!(message.contains("rate-limit response"));
        assert!(message.contains("retry-after 12"));
        assert!(!message.contains("<html>"));
    }
}
