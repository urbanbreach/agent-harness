//! What a failed provider call keeps beyond its message: the HTTP status, the response
//! headers retry logic reads, and a bounded body with secrets masked.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::LazyLock};

/// Longest body kept on an envelope.
pub const ERROR_BODY_LIMIT: usize = 8 * 1024;
const HEADER_VALUE_LIMIT: usize = 1024;
const HEADER_COUNT_LIMIT: usize = 64;

/// Never retained: they carry credentials or session state, not retry facts.
const SENSITIVE_HEADERS: [&str; 6] = [
    "authorization",
    "proxy-authorization",
    "proxy-authenticate",
    "set-cookie",
    "set-cookie2",
    "www-authenticate",
];

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderErrorEnvelope {
    pub status: Option<u16>,
    /// Lowercase response header names with redacted, bounded values.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// At most [`ERROR_BODY_LIMIT`] bytes of the response body, redacted.
    #[serde(default)]
    pub body: Option<String>,
    /// Usage the stream had reported before it failed; `None` for HTTP-level failures.
    #[serde(default)]
    pub usage: Option<crate::CompletionUsage>,
}

/// The body is withheld from `Debug` output so logging an event never prints it.
impl std::fmt::Debug for ProviderErrorEnvelope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderErrorEnvelope")
            .field("status", &self.status)
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .field("body_bytes", &self.body.as_ref().map(String::len))
            .finish()
    }
}

impl ProviderErrorEnvelope {
    pub(crate) fn from_response(
        status: u16,
        headers: &reqwest::header::HeaderMap,
        body: String,
    ) -> Self {
        Self {
            status: Some(status),
            headers: header_map(headers),
            body: Some(body).filter(|body| !body.is_empty()),
            usage: None,
        }
    }
}

/// Response headers safe to keep, in a stable order.
pub(crate) fn header_map(headers: &reqwest::header::HeaderMap) -> BTreeMap<String, String> {
    headers
        .iter()
        .filter(|(name, _)| !SENSITIVE_HEADERS.contains(&name.as_str()))
        .filter_map(|(name, value)| {
            let value = redact(value.to_str().ok()?, &[]);
            Some((
                name.as_str().to_owned(),
                truncate(value, HEADER_VALUE_LIMIT),
            ))
        })
        .take(HEADER_COUNT_LIMIT)
        .collect()
}

/// Reads at most [`ERROR_BODY_LIMIT`] bytes of an error response and masks secrets,
/// including `secrets` the request itself carried.
pub(crate) async fn read_body(response: &mut reqwest::Response, secrets: &[&str]) -> String {
    let mut bytes = Vec::new();
    while bytes.len() < ERROR_BODY_LIMIT {
        match response.chunk().await {
            Ok(Some(chunk)) => bytes.extend_from_slice(&chunk),
            _ => break,
        }
    }
    redact(&String::from_utf8_lossy(&bytes), secrets)
}

/// Masks bearer tokens, key-shaped strings, JWTs, credential fields and every non-empty
/// entry of `secrets`, then cuts the text to [`ERROR_BODY_LIMIT`].
pub(crate) fn redact(text: &str, secrets: &[&str]) -> String {
    static PATTERNS: LazyLock<Vec<(regex::Regex, &'static str)>> = LazyLock::new(|| {
        [
            (r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]{8,}", "Bearer [redacted]"),
            (
                r"\b(?:sk|pk|rk|xai|ghp|gho|ghu|github_pat)[-_][A-Za-z0-9_-]{16,}",
                "[redacted]",
            ),
            (
                r"\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]*",
                "[redacted]",
            ),
            (
                r#"(?i)("?(?:api[_-]?key|token|secret|password|authorization)"?\s*[:=]\s*"?)[^\s",}]{8,}"#,
                "${1}[redacted]",
            ),
        ]
        .into_iter()
        .filter_map(|(pattern, replacement)| Some((regex::Regex::new(pattern).ok()?, replacement)))
        .collect()
    });
    let mut text = text.to_owned();
    for secret in secrets.iter().filter(|secret| !secret.is_empty()) {
        text = text.replace(secret, "[redacted]");
    }
    for (pattern, replacement) in PATTERNS.iter() {
        text = pattern.replace_all(&text, *replacement).into_owned();
    }
    truncate(text, ERROR_BODY_LIMIT)
}

fn truncate(mut text: String, limit: usize) -> String {
    if text.len() > limit {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies_are_masked_and_bounded() {
        let body = format!(
            "bad key sk-abcdefghijklmnopqrstuvwxyz, header Bearer abcdefgh12345678, own-secret {}",
            "x".repeat(ERROR_BODY_LIMIT)
        );
        let masked = redact(&body, &["own-secret"]);
        assert!(!masked.contains("abcdefghijklmnop") && !masked.contains("own-secret"));
        assert!(masked.len() <= ERROR_BODY_LIMIT);
    }

    #[test]
    fn credential_headers_are_dropped() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            "retry-after",
            reqwest::header::HeaderValue::from_static("3"),
        );
        headers.insert(
            "set-cookie",
            reqwest::header::HeaderValue::from_static("a=b"),
        );
        let kept = header_map(&headers);
        assert_eq!(kept.keys().collect::<Vec<_>>(), ["retry-after"]);
    }
}
