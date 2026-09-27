use super::*;
#[derive(Debug, thiserror::Error)]
pub enum CodexOAuthError {
    #[error("failed to generate Codex PKCE entropy: {message}")]
    Random { message: String },
    #[error("Codex OAuth HTTP failure: {message}")]
    Http { message: String },
    #[error("Codex OAuth {operation} failed with status {status}")]
    HttpStatus {
        operation: &'static str,
        status: u16,
    },
    #[error("Codex OAuth {operation} returned malformed JSON: {message}")]
    Json {
        operation: &'static str,
        message: String,
    },
    #[error("Codex OAuth callback state was missing or did not match")]
    InvalidState,
    #[error("Codex OAuth callback did not include an authorization code")]
    MissingCode,
    #[error("Codex OAuth callback rejected authorization: {message}")]
    CallbackRejected { message: String },
    #[error("Codex OAuth callback timed out for {provider}")]
    CallbackTimeout { provider: ProviderId },
    #[error("Codex device authorization timed out for {provider}")]
    DevicePollingTimeout { provider: ProviderId },
    #[error("Codex OAuth token response did not include an access token")]
    MissingAccessToken,
    #[error("Codex OAuth token response did not include a refresh token")]
    MissingRefreshToken,
    #[error(transparent)]
    Store(#[from] CredentialStoreError),
}
impl CodexOAuthError {
    pub fn category(&self) -> ProviderErrorCategory {
        match self {
            Self::HttpStatus {
                status: 401 | 403, ..
            }
            | Self::MissingAccessToken
            | Self::MissingRefreshToken
            | Self::InvalidState
            | Self::MissingCode
            | Self::CallbackRejected { .. } => ProviderErrorCategory::InvalidCredentials,
            Self::HttpStatus { status: 429, .. } => ProviderErrorCategory::RateLimited,
            Self::Json { .. } => ProviderErrorCategory::MalformedStream,
            _ => ProviderErrorCategory::TransportFailure,
        }
    }
}
