use super::{
    codex::{AuthHttpRequest, AuthHttpResponse},
    *,
};
use harness_providers::ProviderErrorCategory;
use std::time::Duration;
mod client;
pub use client::CopilotOAuthClient;
pub const COPILOT_CLIENT_ID: &str = "Ov23li8tweQw6odWQebz";
pub const COPILOT_PUBLIC_DOMAIN: &str = "github.com";
pub const COPILOT_PUBLIC_API_BASE: &str = "https://api.githubcopilot.com";
pub const COPILOT_SCOPE: &str = "read:user";
pub const COPILOT_POLLING_SAFETY_MARGIN: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopilotDeployment {
    Public,
    Enterprise { domain: String },
}
impl CopilotDeployment {
    pub fn public() -> Self {
        Self::Public
    }
    pub fn enterprise(input: &str) -> Result<Self, CopilotOAuthError> {
        Ok(Self::Enterprise {
            domain: normalize_enterprise_domain(input)?,
        })
    }
    pub fn oauth_domain(&self) -> &str {
        match self {
            Self::Public => COPILOT_PUBLIC_DOMAIN,
            Self::Enterprise { domain } => domain,
        }
    }
    pub fn api_base_url(&self) -> String {
        match self {
            Self::Public => COPILOT_PUBLIC_API_BASE.into(),
            Self::Enterprise { domain } => format!("https://copilot-api.{domain}"),
        }
    }
}
pub fn normalize_enterprise_domain(input: &str) -> Result<String, CopilotOAuthError> {
    let invalid = || {
        CopilotOAuthError::InvalidEnterpriseDomain { input: "<redacted>".into(), reason: "expected a domain or HTTP(S) origin without credentials, port, path, query, or fragment".into() }
    };
    if input.chars().any(char::is_control) {
        return Err(invalid());
    }
    let input = input.trim();
    let raw = if input.contains("://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let url = reqwest::Url::parse(&raw).map_err(|_| invalid())?;
    let domain = url.domain().ok_or_else(invalid)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || domain.len() > 253
        || domain.split('.').any(|s| {
            s.is_empty()
                || s.len() > 63
                || s.starts_with('-')
                || s.ends_with('-')
                || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Err(invalid());
    }
    Ok(domain.to_owned())
}
#[async_trait::async_trait]
pub trait CopilotAuthHttpClient: Send + Sync {
    async fn send(&self, request: AuthHttpRequest) -> Result<AuthHttpResponse, CopilotOAuthError>;
}
#[derive(Clone)]
pub struct CopilotDeviceAuthorization {
    pub verification_uri: String,
    pub user_code: String,
    pub device_code: String,
    pub interval_seconds: u64,
}
pub enum CopilotDevicePoll {
    Pending {
        wait: Duration,
    },
    SlowDown {
        interval_seconds: u64,
        wait: Duration,
    },
    Authorized {
        access_token: String,
    },
}
#[derive(Debug, Clone, Copy)]
pub struct CopilotFallbackModel {
    pub id: &'static str,
    pub family: &'static str,
    pub context_window_tokens: u32,
    pub supports_vision: bool,
}
/// Compatibility hints for offline discovery; live model availability still requires the catalog.
pub fn copilot_offline_fallback_models() -> &'static [CopilotFallbackModel] {
    &[
        CopilotFallbackModel {
            id: "gpt-5.5",
            family: "gpt",
            context_window_tokens: 272_000,
            supports_vision: true,
        },
        CopilotFallbackModel {
            id: "claude-sonnet-4.5",
            family: "claude-sonnet",
            context_window_tokens: 200_000,
            supports_vision: true,
        },
    ]
}
#[derive(Debug, thiserror::Error)]
pub enum CopilotOAuthError {
    #[error("GitHub Copilot OAuth HTTP failure: {message}")]
    Http { message: String },
    #[error("GitHub Copilot OAuth {operation} failed with status {status}")]
    HttpStatus {
        operation: &'static str,
        status: u16,
    },
    #[error("GitHub Copilot OAuth {operation} returned malformed JSON: {message}")]
    Json {
        operation: &'static str,
        message: String,
    },
    #[error("GitHub Copilot OAuth {operation} response was malformed: {message}")]
    MalformedResponse {
        operation: &'static str,
        message: String,
    },
    #[error("GitHub Copilot authorization was denied")]
    AccessDenied,
    #[error("GitHub Copilot device code expired")]
    ExpiredToken,
    #[error("GitHub Copilot OAuth returned error `{error}`")]
    OAuthError { error: String },
    #[error("GitHub Copilot device authorization timed out for {provider}")]
    DevicePollingTimeout { provider: ProviderId },
    #[error("GitHub Copilot OAuth token response did not include an access token")]
    MissingAccessToken,
    #[error("invalid GitHub Enterprise URL or domain `{input}`: {reason}")]
    InvalidEnterpriseDomain { input: String, reason: String },
    #[error(transparent)]
    Store(#[from] CredentialStoreError),
}
impl CopilotOAuthError {
    pub fn category(&self) -> ProviderErrorCategory {
        match self {
            Self::HttpStatus {
                status: 401 | 403, ..
            }
            | Self::AccessDenied
            | Self::ExpiredToken
            | Self::MissingAccessToken => ProviderErrorCategory::InvalidCredentials,
            Self::HttpStatus { status: 429, .. } => ProviderErrorCategory::RateLimited,
            Self::Json { .. } | Self::MalformedResponse { .. } => {
                ProviderErrorCategory::MalformedStream
            }
            _ => ProviderErrorCategory::TransportFailure,
        }
    }
}
#[async_trait::async_trait]
impl CopilotAuthHttpClient for super::ReqwestAuthHttpClient {
    async fn send(&self, request: AuthHttpRequest) -> Result<AuthHttpResponse, CopilotOAuthError> {
        super::codex::AuthHttpClient::send(self, request)
            .await
            .map_err(|_| CopilotOAuthError::Http {
                message: "authentication request failed".into(),
            })
    }
}
