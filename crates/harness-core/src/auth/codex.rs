use super::*;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use harness_providers::ProviderErrorCategory;
use serde_json::Value;
use sha2::{Digest, Sha256};
mod client;
mod error;
pub use client::CodexOAuthClient;
pub use error::CodexOAuthError;
pub const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const CODEX_ISSUER: &str = "https://auth.openai.com";
pub const CODEX_OAUTH_PORT: u16 = 1455;
pub const CODEX_DEVICE_VERIFICATION_URL: &str = "https://auth.openai.com/codex/device";

#[derive(Clone)]
pub struct PkceCodes {
    pub verifier: String,
    pub challenge: String,
}
pub fn generate_pkce() -> Result<PkceCodes, CodexOAuthError> {
    let mut entropy = [0u8; 32];
    getrandom::fill(&mut entropy).map_err(|_| CodexOAuthError::Random {
        message: "OS randomness unavailable".into(),
    })?;
    Ok(generate_pkce_from_entropy(&entropy))
}
pub fn generate_pkce_from_entropy(entropy: &[u8]) -> PkceCodes {
    let verifier = URL_SAFE_NO_PAD.encode(Sha256::digest(entropy));
    PkceCodes {
        challenge: pkce_challenge(&verifier),
        verifier,
    }
}
pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}
pub struct CodexLoopbackSession {
    pub redirect_uri: String,
    pub pkce: PkceCodes,
    pub state: String,
    pub authorize_url: String,
}
impl CodexLoopbackSession {
    pub fn new(pkce: PkceCodes, state: impl Into<String>) -> Self {
        Self::with_redirect_uri(
            pkce,
            state,
            format!("http://localhost:{CODEX_OAUTH_PORT}/auth/callback"),
            CODEX_ISSUER,
        )
    }
    pub fn with_redirect_uri(
        pkce: PkceCodes,
        state: impl Into<String>,
        redirect_uri: impl Into<String>,
        issuer: &str,
    ) -> Self {
        let state = state.into();
        let redirect_uri = redirect_uri.into();
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
    pub(super) fn callback_code(&self, callback: &str) -> Result<String, CodexOAuthError> {
        let rejected = || CodexOAuthError::CallbackRejected {
            message: "invalid callback URL or encoding".into(),
        };
        let base = reqwest::Url::parse(&self.redirect_uri).map_err(|_| rejected())?;
        let url = base.join(callback).map_err(|_| rejected())?;
        if callback.len() > 16_384
            || callback.chars().any(char::is_control)
            || url.origin() != base.origin()
            || url.path() != base.path()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(rejected());
        }
        let query = url.query().unwrap_or("");
        if query.match_indices('%').any(|(i, _)| {
            !query
                .as_bytes()
                .get(i + 1..i + 3)
                .is_some_and(|bytes| bytes.iter().all(u8::is_ascii_hexdigit))
        }) {
            return Err(rejected());
        }
        let mut values = BTreeMap::new();
        for (key, value) in url.query_pairs() {
            if value.contains('\u{fffd}')
                || values
                    .insert(key.into_owned(), value.into_owned())
                    .is_some()
            {
                return Err(rejected());
            }
        }
        let state = values.get("state").ok_or(CodexOAuthError::InvalidState)?;
        if self.state.is_empty()
            || state.len() != self.state.len()
            || state
                .bytes()
                .zip(self.state.bytes())
                .fold(0u8, |diff, (a, b)| diff | (a ^ b))
                != 0
        {
            return Err(CodexOAuthError::InvalidState);
        }
        if values.contains_key("error") {
            return Err(CodexOAuthError::CallbackRejected {
                message: "authorization was rejected".into(),
            });
        }
        values
            .remove("code")
            .filter(|v| usable_token(v))
            .ok_or(CodexOAuthError::MissingCode)
    }
}
pub fn codex_authorize_url(
    redirect_uri: &str,
    pkce: &PkceCodes,
    state: &str,
    issuer: &str,
) -> String {
    let parameters = [
        ("response_type", "code"),
        ("client_id", CODEX_CLIENT_ID),
        ("redirect_uri", redirect_uri),
        ("scope", "openid profile email offline_access"),
        ("code_challenge", &pkce.challenge),
        ("code_challenge_method", "S256"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
        ("state", state),
        ("originator", "agent-harness"),
    ];
    // Serializing string pairs cannot fail; an empty URL still fails validation before any request.
    serde_urlencoded::to_string(parameters)
        .map(|query| format!("{}/oauth/authorize?{query}", issuer.trim_end_matches('/')))
        .unwrap_or_default()
}
pub fn codex_callback_success_html() -> &'static str {
    "<!doctype html><title>Signed in</title><p>Signed in. You can return to the terminal.</p>"
}
pub fn codex_callback_error_html(error: &str) -> String {
    let escaped = error
        .chars()
        .take(1024)
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;");
    format!("<!doctype html><title>Sign-in failed</title><p>{escaped}</p>")
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthHttpMethod {
    Post,
}
pub struct AuthHttpRequest {
    pub method: AuthHttpMethod,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}
pub struct AuthHttpResponse {
    pub status: u16,
    pub body: String,
}
#[async_trait::async_trait]
pub trait AuthHttpClient: Send + Sync {
    async fn send(&self, request: AuthHttpRequest) -> Result<AuthHttpResponse, CodexOAuthError>;
}
#[derive(Clone)]
pub struct CodexDeviceAuthorization {
    pub device_auth_id: String,
    pub user_code: String,
    pub interval_seconds: u64,
    pub verification_uri: String,
}
pub enum CodexDevicePoll {
    Pending,
    Authorized {
        authorization_code: String,
        code_verifier: String,
    },
}
#[derive(Clone, Deserialize)]
pub struct CodexTokenResponse {
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub expires_in: Option<u64>,
}
impl fmt::Debug for CodexTokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodexTokenResponse")
            .field("expires_in", &self.expires_in)
            .finish_non_exhaustive()
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
    if token.len() > 65_536 {
        return None;
    }
    let mut parts = token.split('.');
    let _ = parts.next()?;
    let body = parts.next()?;
    let _ = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let claims = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(body).ok()?).ok()?;
    extract_account_id_from_claims(&claims)
}
pub fn extract_account_id_from_claims(claims: &Value) -> Option<String> {
    [
        claims.get("chatgpt_account_id"),
        claims
            .get("https://api.openai.com/auth")
            .and_then(|v| v.get("chatgpt_account_id")),
        claims.pointer("/organizations/0/id"),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .find(|v| usable_token(v))
    .map(str::to_owned)
}
