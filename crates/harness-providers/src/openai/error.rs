use crate::ProviderErrorCategory;

use super::non_empty_string;

pub(super) fn format_transport_error(err: reqwest::Error) -> String {
    let is_timeout = err.is_timeout();
    let is_connect = err.is_connect();
    let status = err.status();
    let sanitized = err.without_url();
    let mut details = Vec::new();
    if is_timeout {
        details.push("timeout");
    }
    if is_connect {
        details.push("connection");
    }
    if status.is_some() {
        details.push("status");
    }
    let category = if details.is_empty() {
        "transport".to_string()
    } else {
        details.join("/")
    };
    format!(
        "openai_compatible request failed before receiving response ({category} error): {sanitized}"
    )
}

pub(super) fn format_non_success_status_message(
    status: u16,
    body: Option<&str>,
    api_key: &str,
) -> String {
    let detail = body
        .and_then(extract_provider_error_detail)
        .or_else(|| body.and_then(non_empty_string).map(str::to_string))
        .map(|body| sanitize_provider_error_detail(&body, api_key))
        .filter(|body| !body.is_empty());

    match detail {
        Some(detail) => format!("openai_compatible request failed with status {status}: {detail}"),
        None => format!("openai_compatible request failed with status {status}"),
    }
}

pub(super) fn categorize_non_success_status(
    status: u16,
    body: Option<&str>,
    api_key: &str,
) -> ProviderErrorCategory {
    if api_key.trim().is_empty() {
        return ProviderErrorCategory::MissingCredentials;
    }

    if status == 429 {
        return ProviderErrorCategory::RateLimited;
    }

    let detail = body
        .and_then(extract_provider_error_detail)
        .or_else(|| body.and_then(non_empty_string).map(str::to_string))
        .unwrap_or_default()
        .to_ascii_lowercase();

    if matches!(status, 401 | 403) {
        if detail.contains("missing")
            && (detail.contains("api key")
                || detail.contains("apikey")
                || detail.contains("credential")
                || detail.contains("authorization"))
        {
            ProviderErrorCategory::MissingCredentials
        } else {
            ProviderErrorCategory::InvalidCredentials
        }
    } else if detail.contains("context_length_exceeded")
        || detail.contains("context length")
        || detail.contains("context window")
        || detail.contains("maximum context")
        || detail.contains("too many tokens")
    {
        ProviderErrorCategory::ContextWindowExceeded
    } else if detail.contains("invalid schema for function")
        || detail.contains("unsupported tool")
        || detail.contains("unsupported function")
        || detail.contains("tool call")
        || detail.contains("function call")
    {
        ProviderErrorCategory::UnsupportedToolCall
    } else {
        ProviderErrorCategory::Other
    }
}

fn extract_provider_error_detail(body: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(body).ok()?;
    parsed
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| parsed.get("message").and_then(serde_json::Value::as_str))
        .map(str::to_string)
}

fn sanitize_provider_error_detail(detail: &str, api_key: &str) -> String {
    if detail.to_ascii_lowercase().contains("authorization") {
        return "provider error body redacted because it contained sensitive auth material"
            .to_string();
    }

    if api_key.is_empty() {
        return detail.to_string();
    }

    detail.replace(api_key, "[REDACTED]")
}
