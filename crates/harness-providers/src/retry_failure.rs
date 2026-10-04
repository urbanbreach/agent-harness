//! Safe, structured retry diagnostics. Raw provider errors stay out of durable UI metadata.
use crate::ProviderErrorCategory;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRetryFailure {
    HttpStatus(u16),
    Connection,
    Server,
    Authentication,
    Serialization,
    IdleTimeout,
    EmptyResponse,
    Truncated,
    RateLimited,
    #[serde(other)]
    Other,
}

impl ProviderRetryFailure {
    pub fn from_error(category: Option<ProviderErrorCategory>, message: &str) -> Self {
        if let Some(status) = http_status(message) {
            return Self::HttpStatus(status);
        }
        match category {
            Some(ProviderErrorCategory::RateLimited) => Self::RateLimited,
            Some(ProviderErrorCategory::TransportFailure) => Self::Connection,
            Some(ProviderErrorCategory::MalformedStream) => Self::Serialization,
            None if message.contains("response truncated by max_tokens") => Self::Truncated,
            None if message.trim().starts_with("request error:") => Self::Connection,
            _ => Self::Other,
        }
    }

    pub fn headline(self) -> Option<String> {
        let headline = match self {
            Self::HttpStatus(code) => {
                let prefix = match code {
                    400 | 422 => "Bad request",
                    403 => "Request denied",
                    404 => "Not found",
                    408 | 504 => "Request timed out",
                    409 => "Conflict",
                    413 => "Request too large",
                    429 => "Rate limited",
                    502 | 503 => "Service unavailable",
                    100..=499 => "Request failed",
                    _ => "Server error",
                };
                return Some(format!("{prefix} ({code})"));
            }
            Self::Connection => "Connection failed",
            Self::Server => "Server error",
            Self::Authentication => "Authentication temporarily unavailable",
            Self::Serialization => "Couldn't read the response",
            Self::IdleTimeout => "No response from the model",
            Self::EmptyResponse => "Empty response",
            Self::Truncated => "Response truncated",
            Self::RateLimited => "Rate limited",
            Self::Other => return None,
        };
        Some(headline.into())
    }
}

fn http_status(raw: &str) -> Option<u16> {
    let lower = raw.to_ascii_lowercase();
    // The first form is our HTTP transport's normalized error; the remaining
    // forms also accept provider API and already classified status messages.
    for marker in [
        "http ",
        "status ",
        "unauthorized (",
        "forbidden (",
        "not found (",
        "bad request (",
        "payment required (",
        "too many requests (",
        "internal server error (",
        "bad gateway (",
        "service unavailable (",
        "gateway timeout (",
        "payload too large (",
        "request entity too large (",
        "server error (",
        "request denied (",
        "request failed (",
        "request too large (",
        "rate limited (",
        "request timed out (",
        "conflict (",
    ] {
        for (index, _) in lower.match_indices(marker) {
            let tail = &raw[index + marker.len()..];
            let Some(digits) = tail.get(..3) else {
                continue;
            };
            if !digits.bytes().all(|b| b.is_ascii_digit())
                || tail.as_bytes().get(3).is_some_and(u8::is_ascii_digit)
                || (marker.ends_with('(') && tail.as_bytes().get(3) != Some(&b')'))
            {
                continue;
            }
            if let Ok(code @ 400..=599) = digits.parse::<u16>() {
                return Some(code);
            }
        }
    }
    None
}
