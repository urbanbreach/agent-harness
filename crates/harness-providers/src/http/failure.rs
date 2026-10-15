use crate::{
    envelope, ProviderBearerToken, ProviderErrorCategory as Category, ProviderErrorEnvelope,
    ProviderStreamEvent as Event,
};

/// Most of the body read to classify a failure; the envelope keeps only its redacted head.
const CLASSIFY_BYTES: usize = 65_536;

/// The error event of a non-success response: classified from the status and, for a bad
/// request, the error code; carrying the status, headers and a bounded redacted body.
pub(super) async fn event(
    mut response: reqwest::Response,
    token: Option<&ProviderBearerToken>,
) -> Event {
    let status = response.status().as_u16();
    let retry_after = retry_after_ms(response.headers());
    let mut category = match status {
        401 | 403 => Category::InvalidCredentials,
        429 => Category::RateLimited,
        500..=599 => Category::TransportFailure,
        _ => Category::Other,
    };
    let mut bytes = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        if bytes.len() + chunk.len() > CLASSIFY_BYTES {
            break;
        }
        bytes.extend_from_slice(&chunk);
    }
    if (status == 400 || status == 413)
        && let Ok(error) = serde_json::from_slice::<serde_json::Value>(&bytes)
    {
        let code = error
            .pointer("/error/code")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let message = error
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if matches!(code, "context_length_exceeded" | "prompt_too_long")
            || message.contains("prompt is too long")
        {
            category = Category::ContextWindowExceeded;
        }
    }
    let secrets: Vec<&str> = token.iter().map(|token| token.token.as_str()).collect();
    let body = envelope::redact(&String::from_utf8_lossy(&bytes), &secrets);
    Event::categorized_error_with_retry_after_ms(
        format!("provider returned HTTP {status}"),
        category,
        retry_after,
    )
    .with_envelope(ProviderErrorEnvelope::from_response(
        status,
        response.headers(),
        body,
    ))
}

/// The longer of `retry-after` (seconds) and `retry-after-ms`.
fn retry_after_ms(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let read = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
    };
    read("retry-after")
        .map(|seconds| seconds.saturating_mul(1000))
        .into_iter()
        .chain(read("retry-after-ms"))
        .max()
}
