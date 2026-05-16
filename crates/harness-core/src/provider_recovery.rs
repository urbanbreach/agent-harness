use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorClass {
    Auth,
    RateLimit,
    Overload,
    ContextWindow,
    MalformedStream,
    UnsupportedTool,
    Transport,
    Unknown,
}

impl ProviderErrorClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::RateLimit => "rate_limit",
            Self::Overload => "overload",
            Self::ContextWindow => "context_window",
            Self::MalformedStream => "malformed_stream",
            Self::UnsupportedTool => "unsupported_tool",
            Self::Transport => "transport",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::RateLimit | Self::Overload | Self::ContextWindow | Self::Transport
        )
    }
}

pub fn classify_provider_error(reason: &str) -> ProviderErrorClass {
    let normalized = reason.to_ascii_lowercase();

    if contains_any(
        &normalized,
        &[
            "unauthorized",
            "forbidden",
            "invalid api key",
            "incorrect api key",
            "authentication",
            "permission denied",
            "401",
            "403",
        ],
    ) {
        ProviderErrorClass::Auth
    } else if contains_any(
        &normalized,
        &[
            "rate limit",
            "rate_limit",
            "quota",
            "too many requests",
            "429",
        ],
    ) {
        ProviderErrorClass::RateLimit
    } else if contains_any(
        &normalized,
        &[
            "overloaded",
            "server overloaded",
            "temporarily unavailable",
            "try again later",
            "service unavailable",
            "503",
            "529",
        ],
    ) {
        ProviderErrorClass::Overload
    } else if is_provider_context_overflow_reason(reason) {
        ProviderErrorClass::ContextWindow
    } else if contains_any(
        &normalized,
        &[
            "malformed stream",
            "invalid sse",
            "invalid json",
            "json parse",
            "unterminated",
            "unexpected eof",
            "stream ended",
        ],
    ) {
        ProviderErrorClass::MalformedStream
    } else if contains_any(
        &normalized,
        &[
            "unsupported tool",
            "tool calls not supported",
            "does not support tools",
            "unsupported function",
            "unmapped tool function",
            "invalid provider tool_call_id",
        ],
    ) {
        ProviderErrorClass::UnsupportedTool
    } else if contains_any(
        &normalized,
        &[
            "connection refused",
            "connection reset",
            "dns",
            "timeout",
            "timed out",
            "network",
            "transport",
            "tls",
            "http/2",
            "502",
            "504",
        ],
    ) {
        ProviderErrorClass::Transport
    } else {
        ProviderErrorClass::Unknown
    }
}

pub fn is_provider_context_overflow_reason(reason: &str) -> bool {
    let normalized = reason.to_ascii_lowercase();
    contains_any(
        &normalized,
        &[
            "context length",
            "context window",
            "too many tokens",
            "prompt token count",
            "maximum context",
            "input token",
            "reduce the length",
            "token count of",
            "exceeds the limit",
        ],
    )
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::{classify_provider_error, ProviderErrorClass};

    #[test]
    fn provider_errors_are_classified_for_recovery_policy() {
        let cases = [
            ("401 unauthorized invalid api key", ProviderErrorClass::Auth),
            ("429 rate limit exceeded", ProviderErrorClass::RateLimit),
            (
                "provider overloaded, try again later",
                ProviderErrorClass::Overload,
            ),
            (
                "maximum context length exceeded",
                ProviderErrorClass::ContextWindow,
            ),
            (
                "malformed stream invalid JSON",
                ProviderErrorClass::MalformedStream,
            ),
            (
                "tool calls not supported by this model",
                ProviderErrorClass::UnsupportedTool,
            ),
            (
                "network timeout while reading response",
                ProviderErrorClass::Transport,
            ),
            ("unrecognized provider failure", ProviderErrorClass::Unknown),
        ];

        for (reason, expected) in cases {
            assert_eq!(classify_provider_error(reason), expected, "{reason}");
        }
    }

    #[test]
    fn retryable_provider_error_classes_are_limited_to_safe_fallbacks() {
        assert!(ProviderErrorClass::RateLimit.is_retryable());
        assert!(ProviderErrorClass::Overload.is_retryable());
        assert!(ProviderErrorClass::ContextWindow.is_retryable());
        assert!(ProviderErrorClass::Transport.is_retryable());
        assert!(!ProviderErrorClass::Auth.is_retryable());
        assert!(!ProviderErrorClass::MalformedStream.is_retryable());
        assert!(!ProviderErrorClass::UnsupportedTool.is_retryable());
        assert!(!ProviderErrorClass::Unknown.is_retryable());
    }
}
