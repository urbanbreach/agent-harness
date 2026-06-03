use std::net::IpAddr;

use super::non_empty_string;

pub const CODEX_API_ENDPOINT: &str = "https://chatgpt.com/backend-api/codex/responses";
pub const COPILOT_API_BASE: &str = "https://api.githubcopilot.com";

pub(super) fn chat_completions_endpoint(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    format!("{base}/chat/completions")
}

pub(super) fn responses_endpoint(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    format!("{base}/responses")
}

pub(super) fn is_loopback_base_url(base_url: &str) -> bool {
    reqwest::Url::parse(base_url.trim())
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<IpAddr>()
                    .map(|ip| ip.is_loopback())
                    .unwrap_or(false)
        })
}

pub(super) fn supports_long_prompt_cache_retention(base_url: &str) -> bool {
    reqwest::Url::parse(base_url.trim())
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .is_some_and(|host| host.eq_ignore_ascii_case("api.openai.com"))
}

pub(super) fn rewrite_codex_endpoint(endpoint: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(endpoint).ok()?;
    let path = parsed.path();
    (path.ends_with("/v1/responses")
        || path.ends_with("/responses")
        || path.ends_with("/chat/completions"))
    .then(|| CODEX_API_ENDPOINT.to_string())
}

pub(super) fn apply_codex_gpt5_response_defaults(
    body: &mut serde_json::Map<String, serde_json::Value>,
) {
    if !body.contains_key("input") {
        return;
    }
    let Some(model_id) = body.get("model").and_then(serde_json::Value::as_str) else {
        return;
    };
    let model_id = model_id.to_ascii_lowercase();
    if !model_id.contains("gpt-5")
        || model_id.contains("gpt-5-chat")
        || model_id.contains("gpt-5-pro")
    {
        return;
    }

    body.entry("include".to_string()).or_insert_with(|| {
        serde_json::Value::Array(vec![serde_json::Value::String(
            "reasoning.encrypted_content".to_string(),
        )])
    });
    body.entry("reasoning".to_string()).or_insert_with(|| {
        serde_json::json!({
            "effort": "medium",
            "summary": "auto"
        })
    });
    if model_id.contains("gpt-5.") && !model_id.contains("codex") {
        body.entry("text".to_string()).or_insert_with(|| {
            serde_json::json!({
                "verbosity": "low"
            })
        });
    }
}

pub(super) fn copilot_base_url(enterprise_url: Option<&str>) -> Result<String, String> {
    enterprise_url
        .and_then(non_empty_string)
        .map(normalize_copilot_enterprise_domain)
        .transpose()
        .map(|domain| {
            domain
                .map(|domain| format!("https://copilot-api.{domain}"))
                .unwrap_or_else(|| COPILOT_API_BASE.to_string())
        })
}

fn normalize_copilot_enterprise_domain(input: &str) -> Result<String, String> {
    let trimmed = input.trim().trim_end_matches('/');
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    if without_scheme.is_empty()
        || without_scheme.contains('/')
        || without_scheme.contains('\\')
        || without_scheme.contains('?')
        || without_scheme.contains('#')
        || without_scheme.chars().any(char::is_whitespace)
        || without_scheme.starts_with('.')
        || without_scheme.ends_with('.')
    {
        return Err(format!(
            "invalid github-copilot enterprise URL or domain `{input}`"
        ));
    }
    Ok(without_scheme.to_ascii_lowercase())
}

pub(super) fn rewrite_endpoint_base(endpoint: &str, base: &str) -> String {
    let Ok(parsed) = reqwest::Url::parse(endpoint) else {
        return endpoint.to_string();
    };
    let path = parsed.path().strip_prefix("/v1").unwrap_or(parsed.path());
    let query = parsed
        .query()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    format!("{}{}{}", base.trim_end_matches('/'), path, query)
}
