//! Anthropic OAuth flow (Claude Pro/Max) and its loopback callback listener.
use super::codex::{AuthHttpClient, AuthHttpMethod, AuthHttpRequest};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt},
    sync::oneshot,
};
mod callback;
pub use callback::*;

pub const ANTHROPIC_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const ANTHROPIC_AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
pub const ANTHROPIC_TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
/// Anthropic page that shows the authorization code to copy (browser on another machine).
pub const ANTHROPIC_COPY_CODE_REDIRECT_URI: &str =
    "https://platform.claude.com/oauth/code/callback";
pub const ANTHROPIC_SCOPES: &str = "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload";
pub const PREFERRED_CALLBACK_PORT: u16 = 53692;
pub const CALLBACK_PATH: &str = "/callback";
/// A login nobody finishes must not keep its listener (and its manual prompt) alive forever:
/// the stale listener would answer a later login's browser redirect with "State mismatch".
pub const LOGIN_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
pub const ANTHROPIC_BROWSER_LOGIN_METHOD: &str = "browser";
pub const ANTHROPIC_COPY_CODE_LOGIN_METHOD: &str = "copy_code";

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct AnthropicOAuthError(pub String);
fn error(message: impl Into<String>) -> AnthropicOAuthError {
    AnthropicOAuthError(message.into())
}

/// Token material in senpi's `OAuthCredential` shape; `expires` is epoch milliseconds.
#[derive(Clone, PartialEq, Eq)]
pub struct AnthropicOAuthCredential {
    pub access: String,
    pub refresh: String,
    pub expires: i64,
}
impl std::fmt::Debug for AnthropicOAuthCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicOAuthCredential")
            .field("expires", &self.expires)
            .finish_non_exhaustive()
    }
}

pub struct AnthropicPkce {
    pub verifier: String,
    pub challenge: String,
}
pub fn generate_anthropic_pkce() -> Result<AnthropicPkce, AnthropicOAuthError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| error("OS randomness unavailable"))?;
    let verifier = URL_SAFE_NO_PAD.encode(bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    Ok(AnthropicPkce {
        verifier,
        challenge,
    })
}

pub fn anthropic_authorize_url(redirect_uri: &str, pkce: &AnthropicPkce) -> String {
    let query = serde_urlencoded::to_string([
        ("code", "true"),
        ("client_id", ANTHROPIC_CLIENT_ID),
        ("response_type", "code"),
        ("redirect_uri", redirect_uri),
        ("scope", ANTHROPIC_SCOPES),
        ("code_challenge", &pkce.challenge),
        ("code_challenge_method", "S256"),
        ("state", &pkce.verifier),
    ])
    .unwrap_or_default();
    format!("{ANTHROPIC_AUTHORIZE_URL}?{query}")
}

pub fn callback_redirect_uri(port: u16) -> String {
    format!("http://localhost:{port}{CALLBACK_PATH}")
}

/// Parses a pasted redirect URL, `code#state` pair, query string, or bare code.
pub fn parse_authorization_input(input: &str) -> (Option<String>, Option<String>) {
    let value = input.trim();
    if value.is_empty() {
        return (None, None);
    }
    let pairs = |query: &str| {
        let mut code = None;
        let mut state = None;
        for (key, value) in form_pairs(query) {
            if key == "code" && code.is_none() {
                code = Some(value);
            } else if key == "state" && state.is_none() {
                state = Some(value);
            }
        }
        (code, state)
    };
    if let Ok(url) = reqwest::Url::parse(value) {
        return pairs(url.query().unwrap_or(""));
    }
    if let Some((code, state)) = value.split_once('#') {
        let state = state.split('#').next().unwrap_or("");
        return (Some(code.into()), Some(state.into()));
    }
    if value.contains("code=") {
        return pairs(value.strip_prefix('?').unwrap_or(value));
    }
    (Some(value.into()), None)
}
fn form_pairs(query: &str) -> Vec<(String, String)> {
    serde_urlencoded::from_str::<Vec<(String, String)>>(query).unwrap_or_default()
}

pub struct AnthropicOAuthClient {
    http: Arc<dyn AuthHttpClient>,
    token_url: String,
    now_ms: Arc<dyn Fn() -> i64 + Send + Sync>,
}
pub fn now_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}
impl AnthropicOAuthClient {
    pub fn new(http: Arc<dyn AuthHttpClient>) -> Self {
        Self {
            http,
            token_url: ANTHROPIC_TOKEN_URL.into(),
            now_ms: Arc::new(now_epoch_ms),
        }
    }
    pub fn with_token_url(mut self, url: impl Into<String>) -> Self {
        self.token_url = url.into();
        self
    }
    pub fn with_clock(mut self, now_ms: Arc<dyn Fn() -> i64 + Send + Sync>) -> Self {
        self.now_ms = now_ms;
        self
    }
    async fn post_json(&self, body: serde_json::Value) -> Result<String, AnthropicOAuthError> {
        let response = self
            .http
            .send(AuthHttpRequest {
                method: AuthHttpMethod::Post,
                url: self.token_url.clone(),
                headers: BTreeMap::from([
                    ("Content-Type".into(), "application/json".into()),
                    ("Accept".into(), "application/json".into()),
                ]),
                body: body.to_string(),
            })
            .await
            .map_err(|e| error(e.to_string()))?;
        if !(200..300).contains(&response.status) {
            return Err(error(format!(
                "HTTP request failed. status={}; url={}; body={}",
                response.status, self.token_url, response.body
            )));
        }
        Ok(response.body)
    }
    fn credential(&self, body: &str) -> Result<AnthropicOAuthCredential, serde_json::Error> {
        #[derive(serde::Deserialize)]
        struct Tokens {
            access_token: String,
            refresh_token: String,
            expires_in: i64,
        }
        let tokens: Tokens = serde_json::from_str(body)?;
        Ok(AnthropicOAuthCredential {
            refresh: tokens.refresh_token,
            access: tokens.access_token,
            expires: (self.now_ms)() + tokens.expires_in * 1000 - 5 * 60 * 1000,
        })
    }
    pub async fn exchange_authorization_code(
        &self,
        code: &str,
        state: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<AnthropicOAuthCredential, AnthropicOAuthError> {
        let body = self
            .post_json(serde_json::json!({
                "grant_type": "authorization_code",
                "client_id": ANTHROPIC_CLIENT_ID,
                "code": code,
                "state": state,
                "redirect_uri": redirect_uri,
                "code_verifier": verifier,
            }))
            .await
            .map_err(|e| {
                error(format!(
                    "Token exchange request failed. url={}; redirect_uri={redirect_uri}; response_type=authorization_code; details={e}",
                    self.token_url
                ))
            })?;
        self.credential(&body).map_err(|e| {
            error(format!(
                "Token exchange returned invalid JSON. url={}; body={body}; details={e}",
                self.token_url
            ))
        })
    }
    pub async fn refresh(
        &self,
        refresh_token: &str,
    ) -> Result<AnthropicOAuthCredential, AnthropicOAuthError> {
        let body = self
            .post_json(serde_json::json!({
                "grant_type": "refresh_token",
                "client_id": ANTHROPIC_CLIENT_ID,
                "refresh_token": refresh_token,
            }))
            .await
            .map_err(|e| {
                error(format!(
                    "Anthropic token refresh request failed. url={}; details={e}",
                    self.token_url
                ))
            })?;
        self.credential(&body).map_err(|e| {
            error(format!(
                "Anthropic token refresh returned invalid JSON. url={}; body={body}; details={e}",
                self.token_url
            ))
        })
    }
}

/// The caller's side of a login: where the URL and progress go, and where a pasted code comes from.
#[async_trait::async_trait]
pub trait AnthropicLoginInteraction: Send + Sync {
    fn auth_url(&self, url: &str, instructions: &str);
    fn progress(&self, message: &str);
    /// Resolves with pasted input. Dropped (never polled again) once the browser callback wins.
    async fn manual_code(
        &self,
        message: &str,
        placeholder: &str,
    ) -> Result<String, AnthropicOAuthError>;
}

fn login_timed_out() -> AnthropicOAuthError {
    error(format!(
        "Anthropic login timed out after {} minutes without a browser callback or a pasted redirect URL. Run the login again.",
        LOGIN_IDLE_TIMEOUT.as_secs() / 60
    ))
}

fn checked_input(
    input: &str,
    verifier: &str,
) -> Result<(Option<String>, String), AnthropicOAuthError> {
    let (code, state) = parse_authorization_input(input);
    if state.as_deref().is_some_and(|s| s != verifier) {
        return Err(error("OAuth state mismatch"));
    }
    Ok((code, state.unwrap_or_else(|| verifier.into())))
}

/// Browser login: the loopback callback and a pasted redirect URL race; the first wins.
pub async fn login_anthropic_browser(
    client: &AnthropicOAuthClient,
    interaction: &dyn AnthropicLoginInteraction,
    callback_host: &str,
) -> Result<AnthropicOAuthCredential, AnthropicOAuthError> {
    let pkce = generate_anthropic_pkce()?;
    let mut listener = start_callback_listener(callback_host, &pkce.verifier).await?;
    let instructions = match &listener.callback_unavailable {
        Some(code) => format!(
            "No local OAuth callback port could be opened (port {PREFERRED_CALLBACK_PORT} and an ephemeral port both failed: {code}). Complete login in your browser, then copy the final redirect URL from the address bar and paste it here."
        ),
        None => "Complete login in your browser. If the browser is on another machine, paste the final redirect URL here.".into(),
    };
    interaction.auth_url(
        &anthropic_authorize_url(&listener.redirect_uri, &pkce),
        &instructions,
    );
    let redirect_uri = listener.redirect_uri.clone();
    let manual = interaction.manual_code(
        "Complete login in your browser, or paste the authorization code / redirect URL here:",
        &redirect_uri,
    );
    let (code, state) = tokio::select! {
        callback = listener.wait_for_code() => match callback {
            Some(callback) => (Some(callback.code), callback.state),
            None => return Err(error("Missing authorization code")),
        },
        input = manual => checked_input(&input?, &pkce.verifier)?,
        () = tokio::time::sleep(LOGIN_IDLE_TIMEOUT) => return Err(login_timed_out()),
    };
    drop(listener);
    let code = code.ok_or_else(|| error("Missing authorization code"))?;
    interaction.progress("Exchanging authorization code for tokens...");
    client
        .exchange_authorization_code(&code, &state, &pkce.verifier, &redirect_uri)
        .await
}

/// Copy-code login: Anthropic shows the code on its own page; the user pastes `code#state`.
pub async fn login_anthropic_copy_code(
    client: &AnthropicOAuthClient,
    interaction: &dyn AnthropicLoginInteraction,
) -> Result<AnthropicOAuthCredential, AnthropicOAuthError> {
    let pkce = generate_anthropic_pkce()?;
    interaction.auth_url(
        &anthropic_authorize_url(ANTHROPIC_COPY_CODE_REDIRECT_URI, &pkce),
        "Complete login in your browser, then copy the code Anthropic shows and paste it here.",
    );
    let input = interaction
        .manual_code(
            "Paste the code Anthropic shows after you sign in:",
            "code#state",
        )
        .await?;
    let (code, state) = checked_input(&input, &pkce.verifier)?;
    let code = code.ok_or_else(|| error("Missing authorization code"))?;
    interaction.progress("Exchanging authorization code for tokens...");
    client
        .exchange_authorization_code(
            &code,
            &state,
            &pkce.verifier,
            ANTHROPIC_COPY_CODE_REDIRECT_URI,
        )
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authorization_input_accepts_every_paste_shape() {
        for (input, code, state) in [
            ("", None, None),
            ("abc", Some("abc"), None),
            ("abc#xyz", Some("abc"), Some("xyz")),
            (
                "http://localhost:53692/callback?code=c1&state=s1",
                Some("c1"),
                Some("s1"),
            ),
            ("code=c2&state=s2", Some("c2"), Some("s2")),
        ] {
            let (parsed_code, parsed_state) = parse_authorization_input(input);
            assert_eq!(parsed_code.as_deref(), code, "{input}");
            assert_eq!(parsed_state.as_deref(), state, "{input}");
        }
    }

    #[tokio::test]
    async fn callback_listener_answers_routes_and_settles_only_on_matching_state(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut listener = start_callback_listener("127.0.0.1", "expected").await?;
        let port = listener.port.ok_or("no port")?;
        assert_eq!(listener.redirect_uri, callback_redirect_uri(port));
        // Browsers may resolve the `localhost` redirect to `::1` first.
        let ipv6 = std::net::TcpListener::bind(("::1", 0)).is_ok();
        let get = |path: &'static str| async move {
            let host = if path.contains("state=expected") && ipv6 {
                "::1"
            } else {
                "127.0.0.1"
            };
            let mut socket = tokio::net::TcpStream::connect((host, port)).await?;
            socket
                .write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                .await?;
            let mut response = String::new();
            socket.read_to_string(&mut response).await?;
            Ok::<_, std::io::Error>(response)
        };
        assert!(get("/other").await?.starts_with("HTTP/1.1 404"));
        assert!(get("/callback?error=denied")
            .await?
            .contains("Error: denied"));
        assert!(get("/callback?code=x&state=foreign")
            .await?
            .contains("different session"));
        assert!(get("/callback?code=the-code&state=expected")
            .await?
            .starts_with("HTTP/1.1 200"));
        let code = listener.wait_for_code().await.ok_or("no code")?;
        assert_eq!(
            (code.code.as_str(), code.state.as_str()),
            ("the-code", "expected")
        );
        Ok(())
    }
}
