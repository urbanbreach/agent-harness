use crate::ProviderStreamEvent;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorCategory {
    MissingCredentials,
    InvalidCredentials,
    RateLimited,
    ContextWindowExceeded,
    UnsupportedToolCall,
    MalformedStream,
    TransportFailure,
    Other,
}

impl ProviderErrorCategory {
    pub const ALL: [Self; 8] = [
        Self::MissingCredentials,
        Self::InvalidCredentials,
        Self::RateLimited,
        Self::ContextWindowExceeded,
        Self::UnsupportedToolCall,
        Self::MalformedStream,
        Self::TransportFailure,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingCredentials => "missing_credentials",
            Self::InvalidCredentials => "invalid_credentials",
            Self::RateLimited => "rate_limited",
            Self::ContextWindowExceeded => "context_window_exceeded",
            Self::UnsupportedToolCall => "unsupported_tool_call",
            Self::MalformedStream => "malformed_stream",
            Self::TransportFailure => "transport_failure",
            Self::Other => "other",
        }
    }

    pub fn remediation(self) -> &'static str {
        match self {
            Self::MissingCredentials | Self::InvalidCredentials => "Check provider credentials.",
            Self::RateLimited => "Wait before retrying, or switch providers.",
            Self::ContextWindowExceeded => "Compact the session or use a larger context window.",
            Self::UnsupportedToolCall => "Check the tool schema and model capabilities.",
            Self::MalformedStream => "Retry the request. The provider returned an invalid stream.",
            Self::TransportFailure => "Check the provider URL and network connection.",
            Self::Other => "Inspect the provider status before retrying.",
        }
    }
}

impl ProviderStreamEvent {
    pub fn error(message: impl Into<String>) -> Self {
        Self::categorized_error(message, ProviderErrorCategory::Other)
    }

    pub fn categorized_error(message: impl Into<String>, category: ProviderErrorCategory) -> Self {
        Self::categorized_error_with_retry_after_ms(message, category, None)
    }

    pub fn categorized_error_with_retry_after_ms(
        message: impl Into<String>,
        category: ProviderErrorCategory,
        retry_after_ms: Option<u64>,
    ) -> Self {
        Self::Error {
            message: message.into(),
            category: Some(category),
            remediation: Some(category.remediation().into()),
            retry_after_ms,
        }
    }
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct ProviderCredentialError {
    pub category: ProviderErrorCategory,
    pub message: String,
}

impl ProviderCredentialError {
    pub fn new(category: ProviderErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCredentialKind {
    StoredOauth,
    StoredApiKey,
    EnvApiKey,
    InlineApiKey,
}

#[derive(Clone)]
pub struct ProviderBearerToken {
    pub token: String,
    pub kind: ProviderCredentialKind,
    pub account_id: Option<String>,
    pub enterprise_url: Option<String>,
}

#[async_trait::async_trait]
pub trait ProviderCredentialSource: Send + Sync {
    async fn bearer_token(&self) -> Result<ProviderBearerToken, ProviderCredentialError>;
}
