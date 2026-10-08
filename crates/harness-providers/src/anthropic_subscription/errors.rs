//! Failure shapes, classification and operator guidance.
use super::accounts::{iso, rate_limit_model_family, AllAccountsBlockedError};
use super::executable::{ClaudeCodeRun, ExecutableSource};
use regex::Regex;
use serde_json::Value;
use std::{
    collections::HashSet,
    sync::{LazyLock, Mutex},
};
mod guidance;
pub use guidance::*;

const PROVIDER: &str = "anthropic-subscription";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SdkErrorKind {
    RateLimit,
    Overloaded,
    AuthError,
    Billing,
    OrgNotAllowed,
    Entitlement,
    Other,
}
impl SdkErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RateLimit => "rate_limit",
            Self::Overloaded => "overloaded",
            Self::AuthError => "auth_error",
            Self::Billing => "billing",
            Self::OrgNotAllowed => "org_not_allowed",
            Self::Entitlement => "entitlement",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub kind: SdkErrorKind,
    pub retryable: bool,
    /// Set on a rate limit that binds one model family, so only that family is blocked.
    pub model_family: Option<String>,
}
const fn class(kind: SdkErrorKind, retryable: bool) -> Classification {
    Classification {
        kind,
        retryable,
        model_family: None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LaneError {
    /// A plain `Error(message)`.
    Message(String),
    /// A failed SDK result; keeps the usage it billed.
    SdkResult {
        message: String,
        usage: Option<Value>,
    },
    Refusal {
        message: String,
        category: Option<String>,
    },
    AllAccountsBlocked(AllAccountsBlockedError),
    /// Failover's classification wrapper (`ClassifiedSdkError`).
    Classified {
        classification: Classification,
        original: Box<LaneError>,
        suppress_turn_retry: bool,
    },
    ColdSeedOverflow {
        estimated_tokens: u64,
        context_window: u64,
    },
}
impl LaneError {
    pub fn message(&self) -> String {
        match self {
            Self::Message(message) | Self::SdkResult { message, .. } | Self::Refusal { message, .. } => {
                message.clone()
            }
            Self::AllAccountsBlocked(error) => error.message.clone(),
            Self::Classified { original, .. } => original.message(),
            Self::ColdSeedOverflow {
                estimated_tokens,
                context_window,
            } => format!(
                "{OWN_REFUSAL_PREFIX} (about {estimated_tokens} tokens, limit {context_window}). Compacting it and retrying."
            ),
        }
    }
    /// The usage a failed SDK result billed, looking through failover's wrapper.
    pub fn result_usage(&self) -> Option<&Value> {
        match self {
            Self::SdkResult { usage, .. } => usage.as_ref(),
            Self::Classified { original, .. } => original.result_usage(),
            _ => None,
        }
    }
    pub fn is_refusal(&self) -> bool {
        match self {
            Self::Refusal { .. } => true,
            Self::Classified { original, .. } => original.is_refusal(),
            _ => false,
        }
    }
}
impl std::fmt::Display for LaneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}
impl std::error::Error for LaneError {}
impl From<String> for LaneError {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

pub const OWN_REFUSAL_PREFIX: &str = "The conversation is too long to resend";

const SDK_ERROR_CLASSIFICATIONS: [(&str, Classification); 10] = [
    (
        "authentication_failed",
        class(SdkErrorKind::AuthError, true),
    ),
    (
        "oauth_org_not_allowed",
        class(SdkErrorKind::OrgNotAllowed, true),
    ),
    ("billing_error", class(SdkErrorKind::Billing, true)),
    ("rate_limit", class(SdkErrorKind::RateLimit, true)),
    ("overloaded", class(SdkErrorKind::Overloaded, true)),
    ("invalid_request", class(SdkErrorKind::Other, false)),
    ("server_error", class(SdkErrorKind::Other, true)),
    ("account_on_hold", class(SdkErrorKind::Billing, true)),
    ("model_not_found", class(SdkErrorKind::Other, false)),
    ("max_output_tokens", class(SdkErrorKind::Other, false)),
];

fn re(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}
fn matches(regex: &LazyLock<Option<Regex>>, text: &str) -> bool {
    regex.as_ref().is_some_and(|re| re.is_match(text))
}
static NETWORK: LazyLock<Option<Regex>> = LazyLock::new(|| {
    re(
        r"\b(enotfound|eai_again|econnreset|econnrefused|etimedout|enetunreach|ehostunreach|und_err_connect_timeout|und_err_socket)\b|fetch failed|socket hang up|connection reset by peer",
    )
});
static RATE_LIMIT: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r"\b(?:http\s*)?429\b|too many requests|rate[ _-]?limit"));
static BLOCKING_LIMIT: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r"\bblocking_limit\b|\brapid_refill_breaker\b"));
static USAGE_LIMIT: LazyLock<Option<Regex>> = LazyLock::new(|| {
    re(r"\b(?:hit|reached|exceeded)\b[^.]*\blimit\b|\b(?:weekly|daily|hourly|usage)\s+limit\b")
});
static OVERLOADED: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r"\b(?:http\s*)?529\b|overloaded"));
static AUTH: LazyLock<Option<Regex>> = LazyLock::new(|| {
    re(r"\binvalid_grant\b|\binvalid_token\b|\b(?:http\s*)?401\b|\bunauthorized\b")
});
static ENTITLEMENT: LazyLock<Option<Regex>> =
    LazyLock::new(|| re(r"\brequires usage credits\b|/usage-credits\b|\bcredits_required\b"));

/// Classifies Anthropic Subscription error codes and HTTP-shaped fallback text in one place.
pub fn classify_sdk_error(text: &str) -> Classification {
    let classification = classify_error_kind(text);
    if classification.kind != SdkErrorKind::RateLimit {
        return classification;
    }
    Classification {
        model_family: rate_limit_model_family(text),
        ..classification
    }
}

fn classify_error_kind(text: &str) -> Classification {
    let text = text.to_lowercase();
    if matches(&NETWORK, &text) {
        return class(SdkErrorKind::Other, true);
    }
    for (code, classification) in &SDK_ERROR_CLASSIFICATIONS {
        if re(&format!(r"\b{code}\b")).is_some_and(|re| re.is_match(&text)) {
            return classification.clone();
        }
    }
    if matches(&RATE_LIMIT, &text) {
        return class(SdkErrorKind::RateLimit, true);
    }
    // Subscription exhaustion arrives as `terminal_reason` or as prose in `errors[0]`.
    if matches(&BLOCKING_LIMIT, &text) || matches(&USAGE_LIMIT, &text) {
        return class(SdkErrorKind::RateLimit, true);
    }
    if matches(&OVERLOADED, &text) {
        return class(SdkErrorKind::Overloaded, true);
    }
    if matches(&AUTH, &text) {
        return class(SdkErrorKind::AuthError, true);
    }
    // Models outside the subscription ask for usage credits: an entitlement, not a rate limit.
    if matches(&ENTITLEMENT, &text) {
        return class(SdkErrorKind::Entitlement, false);
    }
    class(SdkErrorKind::Other, false)
}

pub fn classify_lane_error(error: &LaneError) -> Classification {
    classify_sdk_error(&error.message())
}

/// `sdkResultFailure`: a non-success or `is_error` result as an error that keeps its usage.
pub fn sdk_result_failure(message: &Value) -> Option<LaneError> {
    if message["subtype"] == "success" && message["is_error"] != true {
        return None;
    }
    let first_error = message["errors"]
        .as_array()
        .and_then(|errors| {
            errors
                .iter()
                .filter_map(Value::as_str)
                .find(|e| !e.is_empty())
        })
        .map(str::to_owned);
    let result_text = message["result"].as_str().map(str::trim).unwrap_or("");
    let detail = first_error
        .or_else(|| {
            (message["is_error"] == true && !result_text.is_empty()).then(|| result_text.to_owned())
        })
        .unwrap_or_else(|| {
            format!(
                "Claude Code {}",
                message["subtype"].as_str().unwrap_or("undefined")
            )
        });
    let status = match &message["api_error_status"] {
        Value::Null => String::new(),
        Value::Number(n) => format!("HTTP {n}"),
        Value::String(s) => format!("HTTP {s}"),
        _ => String::new(),
    };
    let reason = message["terminal_reason"].as_str().unwrap_or("");
    let suffix = [status.as_str(), reason]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    Some(LaneError::SdkResult {
        message: if suffix.is_empty() {
            detail
        } else {
            format!("{detail} ({suffix})")
        },
        usage: message.get("usage").filter(|u| !u.is_null()).cloned(),
    })
}

/// `sdkAssistantFailure`: an assistant message Claude Code tagged with `error`.
pub fn sdk_assistant_failure(message: &Value) -> Option<LaneError> {
    let error = match &message["error"] {
        Value::Null => return None,
        Value::String(s) if s.is_empty() => return None,
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let content = &message["message"]["content"];
    let text = match content {
        Value::String(s) => s.trim().to_owned(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_owned(),
        _ => String::new(),
    };
    Some(LaneError::Message(if text.is_empty() {
        error
    } else if error == "unknown" {
        text
    } else {
        format!("{text} ({error})")
    }))
}

/// `refusalError`: a policy refusal (`model_refusal_no_fallback`, or `stop_reason: refusal`).
pub fn refusal_error(message: &Value) -> Option<LaneError> {
    let (content, category) =
        if message["type"] == "system" && message["subtype"] == "model_refusal_no_fallback" {
            (
                message["api_refusal_explanation"]
                    .as_str()
                    .or_else(|| message["content"].as_str())
                    .unwrap_or("")
                    .to_owned(),
                message["api_refusal_category"]
                    .as_str()
                    .filter(|c| !c.is_empty())
                    .map(str::to_owned),
            )
        } else if message["type"] == "assistant" && message["message"]["stop_reason"] == "refusal" {
            let details = &message["message"]["stop_details"];
            (
                details["explanation"]
                    .as_str()
                    .unwrap_or("The request was blocked by policy.")
                    .to_owned(),
                details["category"].as_str().map(str::to_owned),
            )
        } else {
            return None;
        };
    let label = category
        .as_ref()
        .map(|c| format!(" ({c})"))
        .unwrap_or_default();
    Some(LaneError::Refusal {
        message: format!("Claude refused this request{label}: {content}"),
        category,
    })
}

/// `sdkFailure`: the failure an SDK message carries, if any.
pub fn sdk_failure(message: &Value) -> Option<LaneError> {
    refusal_error(message).or_else(|| match message["type"].as_str() {
        Some("assistant") => sdk_assistant_failure(message),
        Some("result") => sdk_result_failure(message),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classification_covers_codes_limits_and_prose() {
        for (text, kind, retryable) in [
            ("connect ECONNRESET", SdkErrorKind::Other, true),
            ("authentication_failed: nope", SdkErrorKind::AuthError, true),
            (
                "API Error: 429 Too Many Requests",
                SdkErrorKind::RateLimit,
                true,
            ),
            (
                "You've hit your weekly limit · resets 5am (Asia/Seoul)",
                SdkErrorKind::RateLimit,
                true,
            ),
            (
                "Claude Code error_during_execution (blocking_limit)",
                SdkErrorKind::RateLimit,
                true,
            ),
            ("API Error: 529 overloaded", SdkErrorKind::Overloaded, true),
            ("refresh: invalid_grant", SdkErrorKind::AuthError, true),
            (
                "This model requires usage credits",
                SdkErrorKind::Entitlement,
                false,
            ),
            ("something else", SdkErrorKind::Other, false),
        ] {
            let classification = classify_sdk_error(text);
            assert_eq!(
                (classification.kind, classification.retryable),
                (kind, retryable),
                "{text}"
            );
        }
        assert_eq!(
            classify_sdk_error("You've hit your Opus limit")
                .model_family
                .as_deref(),
            Some("opus")
        );
    }

    #[test]
    fn result_failures_name_status_and_terminal_reason() {
        let failure = sdk_result_failure(&json!({
            "type": "result", "subtype": "error_during_execution", "is_error": true,
            "errors": ["", "You've hit your limit"], "api_error_status": 429,
            "terminal_reason": "blocking_limit", "usage": {"input_tokens": 3}
        }));
        assert_eq!(
            failure.as_ref().map(LaneError::message).as_deref(),
            Some("You've hit your limit (HTTP 429, blocking_limit)")
        );
        assert!(failure.and_then(|f| f.result_usage().cloned()).is_some());
        assert!(sdk_result_failure(&json!({"subtype": "success", "is_error": false})).is_none());
        assert_eq!(
            refusal_error(&json!({"type": "assistant", "message": {"stop_reason": "refusal", "stop_details": {"category": "cyber"}}}))
                .map(|e| e.message()).as_deref(),
            Some("Claude refused this request (cyber): The request was blocked by policy.")
        );
    }
}
