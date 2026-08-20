// allow: SIZE_OK — Codex OAuth device flow authentication (token exchange + polling + credential storage)
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use harness_providers::ProviderErrorCategory;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::{
    AuthProviderId, CredentialClock, CredentialRefreshError, CredentialStore, CredentialStoreError,
    OAuthRefreshOutcome, OAuthTokenRefresher, ProviderId, StoredCredential, SystemCredentialClock,
};

pub const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const CODEX_ISSUER: &str = "https://auth.openai.com";
pub const CODEX_OAUTH_PORT: u16 = 1455;
pub const CODEX_DEVICE_VERIFICATION_URL: &str = "https://auth.openai.com/codex/device";

const PKCE_VERIFIER_LEN: usize = 43;
const PKCE_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
const CODEX_ALLOWED_MODELS: &[&str] = &[
    "gpt-5.5",
    "gpt-5.2",
    "gpt-5.3-codex",
    "gpt-5.3-codex-spark",
    "gpt-5.4",
    "gpt-5.4-mini",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PkceCodes {
    pub verifier: String,
    pub challenge: String,
}

pub fn generate_pkce() -> Result<PkceCodes, CodexOAuthError> {
    let mut entropy = [0_u8; PKCE_VERIFIER_LEN];
    getrandom::fill(&mut entropy).map_err(|source| CodexOAuthError::Random {
        message: source.to_string(),
    })?;
    Ok(generate_pkce_from_entropy(&entropy))
}

pub fn generate_pkce_from_entropy(entropy: &[u8]) -> PkceCodes {
    let verifier = entropy
        .iter()
        .take(PKCE_VERIFIER_LEN)
        .map(|byte| PKCE_CHARS[*byte as usize % PKCE_CHARS.len()] as char)
        .collect::<String>();
    PkceCodes {
        challenge: pkce_challenge(&verifier),
        verifier,
    }
}

pub fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    base64_url_encode(&digest)
}

pub fn codex_oauth_model_allowed(model_id: &str) -> bool {
    if CODEX_ALLOWED_MODELS.contains(&model_id) {
        return true;
    }
    let Some(version) = model_id
        .strip_prefix("gpt-")
        .and_then(|rest| rest.split_once('.'))
        .and_then(|(major, rest)| {
            let minor = rest
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>();
            (!minor.is_empty()).then(|| format!("{major}.{minor}"))
        })
        .and_then(|version| version.parse::<f32>().ok())
    else {
        return false;
    };
    version > 5.4
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexLoopbackSession {
    pub redirect_uri: String,
    pub pkce: PkceCodes,
    pub state: String,
    pub authorize_url: String,
}

impl CodexLoopbackSession {
    pub fn new(pkce: PkceCodes, state: impl Into<String>) -> Self {
        let redirect_uri = format!("http://localhost:{CODEX_OAUTH_PORT}/auth/callback");
        Self::with_redirect_uri(pkce, state, redirect_uri, CODEX_ISSUER)
    }

    pub fn with_redirect_uri(
        pkce: PkceCodes,
        state: impl Into<String>,
        redirect_uri: impl Into<String>,
        issuer: &str,
    ) -> Self {
        let redirect_uri = redirect_uri.into();
        let state = state.into();
        let authorize_url = codex_authorize_url(&redirect_uri, &pkce, &state, issuer);
        Self {
            redirect_uri,
            pkce,
            state,
            authorize_url,
        }
    }

    pub fn timeout_error(&self) -> CodexOAuthError {
        CodexOAuthError::CallbackTimeout {
            provider: ProviderId::codex(),
        }
    }
}

pub fn codex_callback_success_html() -> &'static str {
    "<!doctype html><html><head><title>Harness Codex Authorization Successful</title></head><body><h1>Authorization Successful</h1><p>You can close this window and return to Harness.</p></body></html>"
}

pub fn codex_callback_error_html(error: &str) -> String {
    format!(
        "<!doctype html><html><head><title>Harness Codex Authorization Failed</title></head><body><h1>Authorization Failed</h1><p>An error occurred during authorization.</p><pre>{}</pre></body></html>",
        escape_html(error)
    )
}

pub fn codex_authorize_url(
    redirect_uri: &str,
    pkce: &PkceCodes,
    state: &str,
    issuer: &str,
) -> String {
    let params = form_urlencode(&[
        ("response_type", "code"),
        ("client_id", CODEX_CLIENT_ID),
        ("redirect_uri", redirect_uri),
        ("scope", "openid profile email offline_access"),
        ("code_challenge", &pkce.challenge),
        ("code_challenge_method", "S256"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("state", state),
        ("originator", "harness"),
    ]);
    format!("{}/oauth/authorize?{params}", issuer.trim_end_matches('/'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthHttpMethod {
    Post,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthHttpRequest {
    pub method: AuthHttpMethod,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthHttpResponse {
    pub status: u16,
    pub body: String,
}

#[async_trait]
pub trait AuthHttpClient: Send + Sync {
    async fn send(&self, request: AuthHttpRequest) -> Result<AuthHttpResponse, CodexOAuthError>;
}

#[derive(Clone)]
pub struct CodexOAuthClient {
    issuer: String,
    http: Arc<dyn AuthHttpClient>,
    clock: Arc<dyn CredentialClock>,
}

impl CodexOAuthClient {
    pub fn new(http: Arc<dyn AuthHttpClient>) -> Self {
        Self {
            issuer: CODEX_ISSUER.to_string(),
            http,
            clock: Arc::new(SystemCredentialClock),
        }
    }

    pub fn with_issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = issuer.into();
        self
    }

    pub fn with_clock(mut self, clock: Arc<dyn CredentialClock>) -> Self {
        self.clock = clock;
        self
    }

    pub async fn complete_loopback_callback(
        &self,
        session: &CodexLoopbackSession,
        callback_url: &str,
        store: &CredentialStore,
    ) -> Result<StoredCredential, CodexOAuthError> {
        let query = parse_query(callback_url);
        if let Some(error) = query.get("error") {
            return Err(CodexOAuthError::CallbackRejected {
                message: query
                    .get("error_description")
                    .cloned()
                    .unwrap_or_else(|| error.clone()),
            });
        }
        let code = query
            .get("code")
            .and_then(|value| non_empty(value))
            .ok_or(CodexOAuthError::MissingCode)?;
        let state = query
            .get("state")
            .and_then(|value| non_empty(value))
            .ok_or(CodexOAuthError::InvalidState)?;
        if state != session.state {
            return Err(CodexOAuthError::InvalidState);
        }

        let tokens = self
            .exchange_authorization_code(code, &session.redirect_uri, &session.pkce)
            .await?;
        self.store_tokens(store, tokens).await
    }

    pub async fn exchange_authorization_code(
        &self,
        code: &str,
        redirect_uri: &str,
        pkce: &PkceCodes,
    ) -> Result<CodexTokenResponse, CodexOAuthError> {
        let body = form_urlencode(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", CODEX_CLIENT_ID),
            ("code_verifier", &pkce.verifier),
        ]);
        self.post_form("/oauth/token", body, "token exchange").await
    }

    pub async fn refresh_access_token(
        &self,
        refresh_token: &str,
    ) -> Result<CodexTokenResponse, CodexOAuthError> {
        let body = form_urlencode(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", CODEX_CLIENT_ID),
        ]);
        self.post_form("/oauth/token", body, "token refresh").await
    }

    pub async fn start_device_authorization(
        &self,
    ) -> Result<CodexDeviceAuthorization, CodexOAuthError> {
        let body = serde_json::json!({ "client_id": CODEX_CLIENT_ID }).to_string();
        let response = self
            .send_json(
                "/api/accounts/deviceauth/usercode",
                body,
                "device authorization",
            )
            .await?;
        let device = serde_json::from_str::<CodexDeviceAuthorizationResponse>(&response.body)
            .map_err(|source| CodexOAuthError::Json {
                operation: "device authorization",
                message: source.to_string(),
            })?;
        let interval_seconds = device.interval.parse::<u64>().unwrap_or(5).max(1);
        Ok(CodexDeviceAuthorization {
            device_auth_id: device.device_auth_id,
            user_code: device.user_code,
            interval_seconds,
            verification_uri: format!("{}/codex/device", self.issuer.trim_end_matches('/')),
        })
    }

    pub async fn complete_device_flow(
        &self,
        store: &CredentialStore,
        max_polls: usize,
    ) -> Result<StoredCredential, CodexOAuthError> {
        let device = self.start_device_authorization().await?;
        for _ in 0..max_polls {
            match self.poll_device_authorization(&device).await? {
                CodexDevicePoll::Pending => continue,
                CodexDevicePoll::Authorized {
                    authorization_code,
                    code_verifier,
                } => {
                    let pkce = PkceCodes {
                        verifier: code_verifier,
                        challenge: String::new(),
                    };
                    let tokens = self
                        .exchange_authorization_code(
                            &authorization_code,
                            &format!("{}/deviceauth/callback", self.issuer.trim_end_matches('/')),
                            &pkce,
                        )
                        .await?;
                    return self.store_tokens(store, tokens).await;
                }
            }
        }
        Err(CodexOAuthError::DevicePollingTimeout {
            provider: ProviderId::codex(),
        })
    }

    pub async fn poll_device_authorization(
        &self,
        device: &CodexDeviceAuthorization,
    ) -> Result<CodexDevicePoll, CodexOAuthError> {
        let body = serde_json::json!({
            "device_auth_id": device.device_auth_id,
            "user_code": device.user_code,
        })
        .to_string();
        let request = AuthHttpRequest {
            method: AuthHttpMethod::Post,
            url: format!(
                "{}/api/accounts/deviceauth/token",
                self.issuer.trim_end_matches('/')
            ),
            headers: json_headers(),
            body,
        };
        let response = self.http.send(request).await?;
        if matches!(response.status, 403 | 404) {
            return Ok(CodexDevicePoll::Pending);
        }
        if !(200..300).contains(&response.status) {
            return Err(CodexOAuthError::HttpStatus {
                operation: "device poll",
                status: response.status,
            });
        }
        let data = serde_json::from_str::<CodexDeviceAuthorizedResponse>(&response.body).map_err(
            |source| CodexOAuthError::Json {
                operation: "device poll",
                message: source.to_string(),
            },
        )?;
        Ok(CodexDevicePoll::Authorized {
            authorization_code: data.authorization_code,
            code_verifier: data.code_verifier,
        })
    }

    async fn post_form(
        &self,
        path: &str,
        body: String,
        operation: &'static str,
    ) -> Result<CodexTokenResponse, CodexOAuthError> {
        let response = self
            .http
            .send(AuthHttpRequest {
                method: AuthHttpMethod::Post,
                url: format!("{}{}", self.issuer.trim_end_matches('/'), path),
                headers: form_headers(),
                body,
            })
            .await?;
        if !(200..300).contains(&response.status) {
            return Err(CodexOAuthError::HttpStatus {
                operation,
                status: response.status,
            });
        }
        serde_json::from_str::<CodexTokenResponse>(&response.body).map_err(|source| {
            CodexOAuthError::Json {
                operation,
                message: source.to_string(),
            }
        })
    }

    async fn send_json(
        &self,
        path: &str,
        body: String,
        operation: &'static str,
    ) -> Result<AuthHttpResponse, CodexOAuthError> {
        let response = self
            .http
            .send(AuthHttpRequest {
                method: AuthHttpMethod::Post,
                url: format!("{}{}", self.issuer.trim_end_matches('/'), path),
                headers: json_headers(),
                body,
            })
            .await?;
        if !(200..300).contains(&response.status) {
            return Err(CodexOAuthError::HttpStatus {
                operation,
                status: response.status,
            });
        }
        Ok(response)
    }

    pub async fn store_tokens(
        &self,
        store: &CredentialStore,
        tokens: CodexTokenResponse,
    ) -> Result<StoredCredential, CodexOAuthError> {
        let access_token =
            non_empty(&tokens.access_token).ok_or(CodexOAuthError::MissingAccessToken)?;
        let refresh_token =
            non_empty(&tokens.refresh_token).ok_or(CodexOAuthError::MissingRefreshToken)?;
        let expires_at = tokens
            .expires_in
            .and_then(|seconds| self.clock.now().checked_add(Duration::from_secs(seconds)))
            .map(format_rfc3339);
        let mut credential = StoredCredential::oauth(
            ProviderId::codex(),
            access_token,
            refresh_token,
            expires_at,
            self.clock.now_rfc3339(),
        );
        credential.account_id = extract_account_id(&tokens);
        store.save(&credential).map_err(CodexOAuthError::Store)?;
        Ok(credential)
    }
}

#[async_trait]
impl OAuthTokenRefresher for CodexOAuthClient {
    async fn refresh(
        &self,
        provider: &ProviderId,
        credential: &StoredCredential,
    ) -> Result<OAuthRefreshOutcome, CredentialRefreshError> {
        if provider != &ProviderId::codex() {
            return Err(CredentialRefreshError::new(
                ProviderErrorCategory::InvalidCredentials,
                format!("codex refresher cannot refresh {provider} credentials"),
            ));
        }
        let refresh_token = credential
            .refresh_token
            .as_deref()
            .and_then(non_empty)
            .ok_or_else(|| {
                CredentialRefreshError::new(
                    ProviderErrorCategory::InvalidCredentials,
                    "codex OAuth credential is missing a refresh token",
                )
            })?;
        let tokens = self
            .refresh_access_token(refresh_token)
            .await
            .map_err(|err| {
                CredentialRefreshError::new(
                    err.category(),
                    format!("codex token refresh failed: {err}"),
                )
            })?;
        let account_id = extract_account_id(&tokens).or_else(|| credential.account_id.clone());
        let expires_at = tokens
            .expires_in
            .and_then(|seconds| self.clock.now().checked_add(Duration::from_secs(seconds)))
            .map(format_rfc3339);
        Ok(OAuthRefreshOutcome {
            access_token: tokens.access_token,
            refresh_token: Some(tokens.refresh_token),
            expires_at,
            account_id,
            scopes: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexDeviceAuthorization {
    pub device_auth_id: String,
    pub user_code: String,
    pub interval_seconds: u64,
    pub verification_uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexDevicePoll {
    Pending,
    Authorized {
        authorization_code: String,
        code_verifier: String,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CodexTokenResponse {
    #[serde(default)]
    pub id_token: Option<String>,
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct CodexDeviceAuthorizationResponse {
    device_auth_id: String,
    user_code: String,
    #[serde(default)]
    interval: String,
}

#[derive(Debug, Deserialize)]
struct CodexDeviceAuthorizedResponse {
    authorization_code: String,
    code_verifier: String,
}

#[derive(Debug, Error)]
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
    CallbackTimeout { provider: AuthProviderId },
    #[error("Codex device authorization timed out for {provider}")]
    DevicePollingTimeout { provider: AuthProviderId },
    #[error("Codex OAuth token response did not include an access token")]
    MissingAccessToken,
    #[error("Codex OAuth token response did not include a refresh token")]
    MissingRefreshToken,
    #[error("credential store error: {0}")]
    Store(#[from] CredentialStoreError),
}

impl CodexOAuthError {
    pub fn category(&self) -> ProviderErrorCategory {
        match self {
            Self::HttpStatus {
                status: 401 | 403, ..
            } => ProviderErrorCategory::InvalidCredentials,
            Self::HttpStatus { status: 429, .. } => ProviderErrorCategory::RateLimited,
            Self::Http { .. } | Self::HttpStatus { .. } | Self::Json { .. } => {
                ProviderErrorCategory::TransportFailure
            }
            Self::InvalidState
            | Self::MissingCode
            | Self::CallbackRejected { .. }
            | Self::CallbackTimeout { .. }
            | Self::DevicePollingTimeout { .. }
            | Self::MissingAccessToken
            | Self::MissingRefreshToken => ProviderErrorCategory::InvalidCredentials,
            Self::Random { .. } | Self::Store(_) => ProviderErrorCategory::Other,
        }
    }
}

pub fn extract_account_id(tokens: &CodexTokenResponse) -> Option<String> {
    tokens
        .id_token
        .as_deref()
        .and_then(extract_account_id_from_jwt)
        .or_else(|| extract_account_id_from_jwt(&tokens.access_token))
}

pub fn extract_account_id_from_jwt(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let decoded = base64_url_decode(payload).ok()?;
    let claims = serde_json::from_slice::<Value>(&decoded).ok()?;
    extract_account_id_from_claims(&claims)
}

pub fn extract_account_id_from_claims(claims: &Value) -> Option<String> {
    claims
        .get("chatgpt_account_id")
        .and_then(Value::as_str)
        .and_then(non_empty)
        .map(str::to_string)
        .or_else(|| {
            claims
                .get("https://api.openai.com/auth")
                .and_then(|auth| auth.get("chatgpt_account_id"))
                .and_then(Value::as_str)
                .and_then(non_empty)
                .map(str::to_string)
        })
        .or_else(|| {
            claims
                .get("organizations")
                .and_then(Value::as_array)
                .and_then(|organizations| organizations.first())
                .and_then(|organization| organization.get("id"))
                .and_then(Value::as_str)
                .and_then(non_empty)
                .map(str::to_string)
        })
}

fn form_headers() -> BTreeMap<String, String> {
    BTreeMap::from([
        (
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        ),
        (
            "User-Agent".to_string(),
            concat!("agent-harness/", env!("CARGO_PKG_VERSION")).to_string(),
        ),
    ])
}

fn json_headers() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("Content-Type".to_string(), "application/json".to_string()),
        (
            "User-Agent".to_string(),
            concat!("agent-harness/", env!("CARGO_PKG_VERSION")).to_string(),
        ),
    ])
}

fn parse_query(url: &str) -> BTreeMap<String, String> {
    let query = url
        .split_once('?')
        .map(|(_, query)| query)
        .unwrap_or(url)
        .split('#')
        .next()
        .unwrap_or_default();
    let mut parsed = BTreeMap::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        parsed.insert(percent_decode(key), percent_decode(value));
    }
    parsed
}

fn form_urlencode(items: &[(&str, &str)]) -> String {
    items
        .iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            b' ' => vec!['+'],
            other => format!("%{other:02X}").chars().collect::<Vec<_>>(),
        })
        .collect()
}

fn percent_decode(value: &str) -> String {
    let mut output = Vec::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = &value[index + 1..index + 3];
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    output.push(byte);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).into_owned()
}

fn base64_url_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity((bytes.len() * 4).div_ceil(3));
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = *chunk.get(1).unwrap_or(&0);
        let third = *chunk.get(2).unwrap_or(&0);
        out.push(TABLE[(first >> 2) as usize] as char);
        out.push(TABLE[(((first & 0b0000_0011) << 4) | (second >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((second & 0b0000_1111) << 2) | (third >> 6)) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(TABLE[(third & 0b0011_1111) as usize] as char);
        }
    }
    out
}

fn base64_url_decode(value: &str) -> Result<Vec<u8>, ()> {
    let mut output = Vec::with_capacity(value.len() * 3 / 4);
    let mut buffer = 0_u32;
    let mut bits = 0_u8;
    for byte in value.bytes() {
        let value = match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a' + 26),
            b'0'..=b'9' => u32::from(byte - b'0' + 52),
            b'-' => 62,
            b'_' => 63,
            b'=' => break,
            _ => return Err(()),
        };
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push(u8::try_from(buffer >> bits).unwrap_or(u8::MAX));
            buffer &= (1 << bits) - 1;
        }
    }
    Ok(output)
}

fn format_rfc3339(time: SystemTime) -> String {
    humantime::format_rfc3339(time).to_string()
}

fn non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn escape_html(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| match ch {
            '&' => "&amp;".chars().collect::<Vec<_>>(),
            '<' => "&lt;".chars().collect::<Vec<_>>(),
            '>' => "&gt;".chars().collect::<Vec<_>>(),
            '"' => "&quot;".chars().collect::<Vec<_>>(),
            '\'' => "&#39;".chars().collect::<Vec<_>>(),
            other => vec![other],
        })
        .collect()
}

#[cfg(test)]
mod tests;
