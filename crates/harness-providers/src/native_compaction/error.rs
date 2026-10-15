use crate::{ProviderErrorCategory, ProviderErrorEnvelope};
use std::collections::BTreeMap;

/// Native failure classification retained across protocol fallback.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct NativeCompactionError {
    pub message: String,
    pub category: Option<ProviderErrorCategory>,
    pub status: Option<u16>,
    pub auth: bool,
    /// Response headers the provider sent with the failure.
    pub headers: BTreeMap<String, String>,
    /// Bounded, redacted response body or the lower-level failure text.
    pub cause: Option<String>,
}

impl NativeCompactionError {
    pub fn new(message: impl Into<String>) -> Self {
        let message = message.into();
        let auth = message.contains("auth_unavailable");
        Self {
            message,
            category: auth.then_some(ProviderErrorCategory::InvalidCredentials),
            status: None,
            auth,
            headers: BTreeMap::new(),
            cause: None,
        }
    }

    /// A failure the credential source reported before any request went out.
    pub(crate) fn credentials(category: ProviderErrorCategory, cause: String) -> Self {
        Self {
            message: "Remote compaction credentials unavailable".into(),
            category: Some(category),
            status: None,
            auth: true,
            headers: BTreeMap::new(),
            cause: Some(cause),
        }
    }

    /// An HTTP failure with `message`, classified from the status and the response body.
    pub(crate) fn http(message: impl Into<String>, envelope: ProviderErrorEnvelope) -> Self {
        let status = envelope.status;
        let body = envelope.body.unwrap_or_default();
        let auth = matches!(status, Some(401 | 403)) || body.contains("auth_unavailable");
        let category = if auth {
            ProviderErrorCategory::InvalidCredentials
        } else if status == Some(429) {
            ProviderErrorCategory::RateLimited
        } else if body.to_lowercase().contains("context") {
            ProviderErrorCategory::ContextWindowExceeded
        } else {
            ProviderErrorCategory::Other
        };
        Self {
            message: message.into(),
            category: Some(category),
            status,
            auth,
            headers: envelope.headers,
            cause: Some(body).filter(|body| !body.is_empty()),
        }
    }

    /// A native lane's failed response: the message names the status only; the body words
    /// stay in `cause`.
    pub(crate) fn lane_http(envelope: ProviderErrorEnvelope) -> Self {
        let message = format!(
            "provider returned HTTP {}",
            envelope.status.unwrap_or_default()
        );
        Self::http(message, envelope)
    }

    pub(crate) fn transient(&self) -> bool {
        !self.auth
            && (self
                .status
                .is_some_and(|s| matches!(s, 408 | 409 | 429 | 500 | 502 | 503 | 504 | 529))
                || self.status.is_none()
                    && [
                        "connection",
                        "timeout",
                        "closed-before-completed",
                        "parse failed",
                        "server_error",
                        "internal_error",
                        "overloaded",
                        "service unavailable",
                    ]
                    .iter()
                    .any(|s| self.message.contains(s)))
    }
}
