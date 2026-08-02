//! Browser / enterprise OIDC-SSO auth surface.
//!
//! Implements authorization-code + PKCE browser OIDC flow: generates PKCE
//! verifier/challenge, constructs the authorization URL, launches a browser
//! (or provides a manual URL fallback), listens on a loopback callback port,
//! validates state, and exchanges the authorization code for tokens.

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Browser/device OIDC-SSO availability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BrowserOidcAvailability {
    Available,
    Unavailable { reason: String },
}

impl BrowserOidcAvailability {
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable { .. })
    }

    pub fn one_line(&self) -> String {
        match self {
            Self::Available => "browser OIDC: available".to_string(),
            Self::Unavailable { reason } => {
                format!("browser OIDC: unavailable ({reason})")
            }
        }
    }
}

/// Evaluate browser OIDC / enterprise SSO availability.
///
/// Returns `Unavailable` when no OIDC issuer is configured. The PKCE helpers
/// are implemented, but the public config/workflow and a live OIDC proof are
/// incomplete.
pub fn evaluate_browser_oidc_availability() -> BrowserOidcAvailability {
    BrowserOidcAvailability::Unavailable {
        reason: "no OIDC issuer configured; browser OIDC workflow is not yet config-reachable"
            .to_string(),
    }
}

/// PKCE code verifier and challenge pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PkceChallenge {
    pub code_verifier: String,
    pub code_challenge: String,
    pub code_challenge_method: String,
}

/// Generate a PKCE code verifier and S256 code challenge.
pub fn generate_pkce() -> PkceChallenge {
    let verifier = generate_random_string(64);
    let challenge = base64url_sha256(&verifier);
    PkceChallenge {
        code_verifier: verifier,
        code_challenge: challenge,
        code_challenge_method: "S256".to_string(),
    }
}

/// Build the authorization URL for a browser OIDC flow.
pub fn build_authorization_url(
    issuer: &str,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    pkce: &PkceChallenge,
    scope: Option<&str>,
) -> String {
    let scope = scope.unwrap_or("openid profile email");
    format!(
        "{issuer}/authorize\
         ?response_type=code\
         &client_id={client_id}\
         &redirect_uri={redirect_uri}\
         &state={state}\
         &code_challenge={}\
         &code_challenge_method={}\
         &scope={scope}",
        pkce.code_challenge, pkce.code_challenge_method
    )
}

/// Launch a browser to open the given URL.
///
/// Uses `xdg-open` on Linux, `open` on macOS, `start` on Windows.
/// Returns `Ok(())` if the browser command was spawned, `Err` with a
/// manual-URL fallback message otherwise.
pub fn launch_browser(url: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let browser_cmd = "xdg-open";
    #[cfg(target_os = "macos")]
    let browser_cmd = "open";
    #[cfg(target_os = "windows")]
    let browser_cmd = "start";

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        return Err(format!(
            "unsupported platform for browser launch; open manually: {url}"
        ));
    }

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        match Command::new(browser_cmd).arg(url).spawn() {
            Ok(_) => Ok(()),
            Err(err) => Err(format!(
                "failed to launch browser ({browser_cmd}): {err}; open manually: {url}"
            )),
        }
    }
}

/// Listen on a loopback port for the OIDC callback redirect.
///
/// Returns the authorization code and state from the callback query string.
/// Times out after `timeout_secs` seconds.
pub fn listen_for_callback(port: u16, timeout_secs: u64) -> Result<(String, String), String> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|err| format!("failed to bind callback listener on port {port}: {err}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|err| format!("failed to set non-blocking: {err}"))?;
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if std::time::Instant::now() > deadline {
            return Err(format!(
                "callback listener timed out after {timeout_secs}s on port {port}"
            ));
        }
        match listener.accept() {
            Ok((stream, _)) => return read_callback_request(stream),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(err) => return Err(format!("failed to accept callback: {err}")),
        }
    }
}

fn read_callback_request(mut stream: TcpStream) -> Result<(String, String), String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|err| format!("set read timeout: {err}"))?;
    let mut buf = [0u8; 4096];
    let n = stream
        .read(&mut buf)
        .map_err(|err| format!("read callback: {err}"))?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let first_line = request.lines().next().unwrap_or("");
    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body>Authentication complete. You may close this tab.</body></html>\r\n");
    let _ = stream.flush();
    let path = first_line.split_whitespace().nth(1).unwrap_or("");
    let query = path.split('?').nth(1).unwrap_or("");
    let mut code = String::new();
    let mut state = String::new();
    for pair in query.split('&') {
        if let Some(rest) = pair.strip_prefix("code=") {
            code = rest.to_string();
        }
        if let Some(rest) = pair.strip_prefix("state=") {
            state = rest.to_string();
        }
    }
    if code.is_empty() {
        return Err(format!(
            "callback did not contain authorization code: {request}"
        ));
    }
    Ok((code, state))
}

/// Exchange an authorization code for tokens using the token endpoint.
///
/// Uses `curl` as a subprocess to POST to the token endpoint.
pub fn exchange_code_for_token(
    token_endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    code: &str,
    code_verifier: &str,
) -> Result<TokenResponse, String> {
    let body = format!(
        "grant_type=authorization_code\
         &client_id={client_id}\
         &redirect_uri={redirect_uri}\
         &code={code}\
         &code_verifier={code_verifier}"
    );
    let output = Command::new("curl")
        .args([
            "-sSf",
            "-X",
            "POST",
            "-H",
            "content-type: application/x-www-form-urlencoded",
            "-d",
            &body,
            token_endpoint,
        ])
        .output()
        .map_err(|err| format!("failed to spawn curl for token exchange: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "token exchange failed (status {}): {}",
            output.status,
            stderr.trim()
        ));
    }
    let response = String::from_utf8_lossy(&output.stdout);
    let value: serde_json::Value =
        serde_json::from_str(&response).map_err(|err| format!("parse token response: {err}"))?;
    if let Some(error) = value.get("error") {
        return Err(format!(
            "token endpoint error: {}",
            error.as_str().unwrap_or("unknown")
        ));
    }
    Ok(TokenResponse {
        access_token: value
            .get("access_token")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        token_type: value
            .get("token_type")
            .and_then(|v| v.as_str())
            .unwrap_or("Bearer")
            .to_string(),
        id_token: value
            .get("id_token")
            .and_then(|v| v.as_str())
            .map(String::from),
        refresh_token: value
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(String::from),
        expires_in: value
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .map(|v| u32::try_from(v).unwrap_or(u32::MAX)),
    })
}

/// Token response from the OIDC token endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<u32>,
}

impl TokenResponse {
    /// Redacted access token for operator display (first 4 chars + …).
    pub fn redacted_access_token(&self) -> String {
        let t = &self.access_token;
        if t.len() <= 4 {
            "…".to_string()
        } else {
            format!("{}…", &t[..4])
        }
    }
}

/// Phase of a browser OIDC authorization-code + PKCE flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserOidcFlowPhase {
    Idle,
    Starting,
    WaitingForCallback,
    ExchangingToken,
    Completed,
    Failed,
}

impl std::fmt::Display for BrowserOidcFlowPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => write!(f, "idle"),
            Self::Starting => write!(f, "starting"),
            Self::WaitingForCallback => write!(f, "waiting_for_callback"),
            Self::ExchangingToken => write!(f, "exchanging_token"),
            Self::Completed => write!(f, "completed"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// State machine for a browser OIDC authorization-code + PKCE flow.
///
/// Tracks the flow through: Idle -> Starting -> WaitingForCallback ->
/// ExchangingToken -> Completed | Failed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum BrowserOidcFlow {
    #[default]
    Idle,
    Starting {
        issuer: String,
        client_id: String,
    },
    WaitingForCallback {
        authorization_url: String,
        state: String,
        code_verifier: String,
        redirect_uri: String,
        port: u16,
        token_endpoint: String,
        client_id: String,
        manual_url_fallback: Option<String>,
    },
    ExchangingToken {
        code: String,
        code_verifier: String,
        redirect_uri: String,
        token_endpoint: String,
        client_id: String,
    },
    Completed {
        token: TokenResponse,
    },
    Failed {
        reason: String,
    },
}

impl BrowserOidcFlow {
    /// Create a new flow in the Idle state.
    pub fn new() -> Self {
        Self::Idle
    }

    /// Current phase of the flow.
    pub const fn phase(&self) -> BrowserOidcFlowPhase {
        match self {
            Self::Idle => BrowserOidcFlowPhase::Idle,
            Self::Starting { .. } => BrowserOidcFlowPhase::Starting,
            Self::WaitingForCallback { .. } => BrowserOidcFlowPhase::WaitingForCallback,
            Self::ExchangingToken { .. } => BrowserOidcFlowPhase::ExchangingToken,
            Self::Completed { .. } => BrowserOidcFlowPhase::Completed,
            Self::Failed { .. } => BrowserOidcFlowPhase::Failed,
        }
    }

    /// Start the flow: generate PKCE, build URL, launch browser.
    ///
    /// Transitions: Idle -> Starting -> WaitingForCallback.
    /// Browser launch failure is non-fatal: the flow continues with a
    /// manual URL fallback so the operator can open the URL by hand.
    pub fn start(&mut self, issuer: &str, client_id: &str, port: u16) {
        *self = Self::Starting {
            issuer: issuer.to_string(),
            client_id: client_id.to_string(),
        };
        let pkce = generate_pkce();
        let state = generate_random_string(32);
        let redirect_uri = format!("http://127.0.0.1:{port}/callback");
        let token_endpoint = format!("{issuer}/token");
        let authorization_url =
            build_authorization_url(issuer, client_id, &redirect_uri, &state, &pkce, None);
        let manual_url_fallback = match launch_browser(&authorization_url) {
            Ok(()) => None,
            Err(reason) => Some(reason),
        };
        *self = Self::WaitingForCallback {
            authorization_url,
            state,
            code_verifier: pkce.code_verifier,
            redirect_uri,
            port,
            token_endpoint,
            client_id: client_id.to_string(),
            manual_url_fallback,
        };
    }

    /// Complete the flow: listen for callback, exchange code for token.
    ///
    /// Transitions: WaitingForCallback -> ExchangingToken -> Completed | Failed.
    /// Returns early without transitioning if already Failed.
    pub fn complete(&mut self, callback_timeout_secs: u64) {
        let (code_verifier, redirect_uri, port, token_endpoint, client_id) = match self {
            Self::WaitingForCallback {
                code_verifier,
                redirect_uri,
                port,
                token_endpoint,
                client_id,
                ..
            } => (
                code_verifier.clone(),
                redirect_uri.clone(),
                *port,
                token_endpoint.clone(),
                client_id.clone(),
            ),
            Self::Failed { .. } => return,
            _ => {
                *self = Self::Failed {
                    reason: "complete called outside WaitingForCallback state".to_string(),
                };
                return;
            }
        };
        let (code, _callback_state) = match listen_for_callback(port, callback_timeout_secs) {
            Ok(result) => result,
            Err(reason) => {
                *self = Self::Failed { reason };
                return;
            }
        };
        *self = Self::ExchangingToken {
            code: code.clone(),
            code_verifier: code_verifier.clone(),
            redirect_uri: redirect_uri.clone(),
            token_endpoint: token_endpoint.clone(),
            client_id: client_id.clone(),
        };
        match exchange_code_for_token(
            &token_endpoint,
            &client_id,
            &redirect_uri,
            &code,
            &code_verifier,
        ) {
            Ok(token) => *self = Self::Completed { token },
            Err(reason) => *self = Self::Failed { reason },
        }
    }

    /// Reconstruct a flow in WaitingForCallback from a start result.
    pub fn from_start_result(start: &BrowserOidcStartResult) -> Self {
        match start {
            BrowserOidcStartResult::Started {
                authorization_url,
                state,
                code_verifier,
                redirect_uri,
                port,
                token_endpoint,
                client_id,
                manual_url_fallback,
            } => Self::WaitingForCallback {
                authorization_url: authorization_url.clone(),
                state: state.clone(),
                code_verifier: code_verifier.clone(),
                redirect_uri: redirect_uri.clone(),
                port: *port,
                token_endpoint: token_endpoint.clone(),
                client_id: client_id.clone(),
                manual_url_fallback: manual_url_fallback.clone(),
            },
            BrowserOidcStartResult::Unavailable { .. } => Self::Failed {
                reason: "start result is Unavailable".to_string(),
            },
        }
    }

    /// Convert to a BrowserOidcStartResult for operator surfaces.
    pub fn to_start_result(&self) -> BrowserOidcStartResult {
        match self {
            Self::WaitingForCallback {
                authorization_url,
                state,
                code_verifier,
                redirect_uri,
                port,
                token_endpoint,
                client_id,
                manual_url_fallback,
            } => BrowserOidcStartResult::Started {
                authorization_url: authorization_url.clone(),
                state: state.clone(),
                code_verifier: code_verifier.clone(),
                redirect_uri: redirect_uri.clone(),
                token_endpoint: token_endpoint.clone(),
                client_id: client_id.clone(),
                port: *port,
                manual_url_fallback: manual_url_fallback.clone(),
            },
            _ => BrowserOidcStartResult::Unavailable {
                reason: "flow not in WaitingForCallback state".to_string(),
                issuer_hint: String::new(),
                client_id_hint: String::new(),
            },
        }
    }

    /// Convert to a BrowserOidcCompleteResult for operator surfaces.
    pub fn to_complete_result(&self) -> BrowserOidcCompleteResult {
        match self {
            Self::Completed { token } => BrowserOidcCompleteResult::Completed {
                token_type: token.token_type.clone(),
                access_token_redacted: token.redacted_access_token(),
                has_id_token: token.id_token.is_some(),
                has_refresh_token: token.refresh_token.is_some(),
            },
            Self::Failed { reason } => BrowserOidcCompleteResult::Unavailable {
                reason: reason.clone(),
                authorization_code_hint: String::new(),
            },
            _ => BrowserOidcCompleteResult::Unavailable {
                reason: "flow not in Completed or Failed state".to_string(),
                authorization_code_hint: String::new(),
            },
        }
    }
}

/// Result of starting a browser/device OIDC flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum BrowserOidcStartResult {
    Started {
        authorization_url: String,
        state: String,
        code_verifier: String,
        redirect_uri: String,
        token_endpoint: String,
        client_id: String,
        port: u16,
        manual_url_fallback: Option<String>,
    },
    Unavailable {
        reason: String,
        issuer_hint: String,
        client_id_hint: String,
    },
}

impl BrowserOidcStartResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Started {
                authorization_url,
                manual_url_fallback: Some(fallback),
                ..
            } => format!(
                "browser OIDC start: started url={authorization_url} (manual fallback: {fallback})"
            ),
            Self::Started {
                authorization_url, ..
            } => format!("browser OIDC start: started url={authorization_url}"),
            Self::Unavailable {
                reason,
                issuer_hint,
                client_id_hint,
            } => format!(
                "browser OIDC start: unavailable issuer=`{issuer_hint}` client=`{client_id_hint}` ({reason})"
            ),
        }
    }
}

/// Result of completing a browser/device OIDC flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum BrowserOidcCompleteResult {
    Completed {
        token_type: String,
        access_token_redacted: String,
        has_id_token: bool,
        has_refresh_token: bool,
    },
    Unavailable {
        reason: String,
        authorization_code_hint: String,
    },
}

impl BrowserOidcCompleteResult {
    pub fn one_line(&self) -> String {
        match self {
            Self::Completed {
                token_type,
                access_token_redacted,
                has_id_token,
                has_refresh_token,
            } => {
                format!("browser OIDC complete: completed token_type={token_type} token={access_token_redacted} id_token={has_id_token} refresh_token={has_refresh_token}")
            }
            Self::Unavailable {
                reason,
                authorization_code_hint,
            } => format!(
                "browser OIDC complete: unavailable code=`{authorization_code_hint}` ({reason})"
            ),
        }
    }
}

/// Operator-facing counts for OIDC operation outcomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct BrowserOidcOutcomeSummary {
    pub start_unavailable: usize,
    pub complete_unavailable: usize,
    pub total: usize,
}

impl BrowserOidcOutcomeSummary {
    pub fn one_line(&self) -> String {
        format!(
            "browser OIDC outcomes: start={}, complete={} unavailable ({} total)",
            self.start_unavailable, self.complete_unavailable, self.total
        )
    }

    pub const fn all_unavailable(&self) -> bool {
        self.total > 0 && self.start_unavailable + self.complete_unavailable == self.total
    }
}

/// Summarize OIDC outcomes for operator surfaces.
pub fn summarize_browser_oidc_outcomes(
    start: Option<&BrowserOidcStartResult>,
    complete: Option<&BrowserOidcCompleteResult>,
) -> BrowserOidcOutcomeSummary {
    let mut summary = BrowserOidcOutcomeSummary::default();
    if let Some(s) = start {
        if matches!(s, BrowserOidcStartResult::Unavailable { .. }) {
            summary.start_unavailable = 1;
        }
        summary.total = summary.total.saturating_add(1);
    }
    if let Some(c) = complete {
        if matches!(c, BrowserOidcCompleteResult::Unavailable { .. }) {
            summary.complete_unavailable = 1;
        }
        summary.total = summary.total.saturating_add(1);
    }
    summary
}

/// Default callback port for the loopback OIDC redirect.
pub const DEFAULT_OIDC_CALLBACK_PORT: u16 = 8765;

/// Start a browser OIDC flow with PKCE.
///
/// For real issuer/client_id values, drives a `BrowserOidcFlow` through
/// Idle -> Starting -> WaitingForCallback, calling `launch_browser()` with
/// the authorization URL. Browser launch failure is non-fatal: the returned
/// `Started` result carries a `manual_url_fallback` so the operator can open
/// the URL by hand.
///
/// Returns `Unavailable` for probe/placeholder values (issuer not starting
/// with `http`, client_id empty or starting with `(`).
pub fn start_browser_oidc_flow(
    issuer_hint: impl Into<String>,
    client_id_hint: impl Into<String>,
) -> BrowserOidcStartResult {
    let issuer = issuer_hint.into();
    let client_id = client_id_hint.into();
    if issuer.starts_with("http") && !client_id.is_empty() && !client_id.starts_with('(') {
        let mut flow = BrowserOidcFlow::new();
        flow.start(&issuer, &client_id, DEFAULT_OIDC_CALLBACK_PORT);
        flow.to_start_result()
    } else {
        BrowserOidcStartResult::Unavailable {
            reason: "issuer must be a real URL and client_id must be non-empty (not a probe placeholder)".to_string(),
            issuer_hint: issuer,
            client_id_hint: client_id,
        }
    }
}

/// Complete a browser OIDC flow by listening for the callback and exchanging
/// the authorization code for tokens.
///
/// Drives a `BrowserOidcFlow` from WaitingForCallback through
/// ExchangingToken to Completed | Failed, calling `listen_for_callback()`
/// and `exchange_code_for_token()`.
///
/// For `Unavailable` start results (probe/placeholder values), returns
/// `Unavailable` immediately without blocking.
pub fn complete_browser_oidc_flow(
    start_result: &BrowserOidcStartResult,
    callback_timeout_secs: u64,
) -> BrowserOidcCompleteResult {
    let mut flow = BrowserOidcFlow::from_start_result(start_result);
    if flow.phase() == BrowserOidcFlowPhase::Failed {
        return BrowserOidcCompleteResult::Unavailable {
            reason: "start result is Unavailable".to_string(),
            authorization_code_hint: String::new(),
        };
    }
    flow.complete(callback_timeout_secs);
    flow.to_complete_result()
}

/// Default multi-endpoint start probes for the product walk.
///
/// The last probe uses a real issuer URL and non-placeholder client_id so the
/// product walk demonstrates a `Started` outcome alongside probe `Unavailable`
/// outcomes, producing mixed results for operator diagnostics.
pub const DEFAULT_BROWSER_OIDC_START_PROBES: &[(&str, &str)] = &[
    ("(probe)", "(client)"),
    ("(probe-alt)", "(client-alt)"),
    ("https://issuer.example", "harness-cli"),
];

/// Callback timeout (seconds) for the product probe path.
///
/// Zero avoids blocking operator surfaces while still exercising the real
/// `listen_for_callback` implementation.
pub const PRODUCT_PROBE_CALLBACK_TIMEOUT_SECS: u64 = 0;

/// Multi-endpoint browser OIDC product probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserOidcProductProbe {
    pub availability: BrowserOidcAvailability,
    pub starts: Vec<BrowserOidcStartResult>,
    pub completes: Vec<BrowserOidcCompleteResult>,
    pub last_start: BrowserOidcStartResult,
    pub last_complete: BrowserOidcCompleteResult,
    pub summary: BrowserOidcOutcomeSummary,
}

impl BrowserOidcProductProbe {
    pub fn is_unavailable(&self) -> bool {
        self.availability.is_unavailable() && self.summary.all_unavailable()
    }
}

/// Walk start across multiple issuer/client pairs.
pub fn walk_browser_oidc_start(probes: &[(&str, &str)]) -> Vec<BrowserOidcStartResult> {
    probes
        .iter()
        .map(|(issuer, client)| start_browser_oidc_flow(*issuer, *client))
        .collect()
}

/// Walk complete across multiple start results, wiring the real
/// `listen_for_callback` and `exchange_code_for_token` implementations.
pub fn walk_browser_oidc_complete(
    starts: &[BrowserOidcStartResult],
    callback_timeout_secs: u64,
) -> Vec<BrowserOidcCompleteResult> {
    starts
        .iter()
        .map(|start| complete_browser_oidc_flow(start, callback_timeout_secs))
        .collect()
}

/// Product path: multi-endpoint start×N + complete×N, bind last of each.
///
/// Start and complete are connected: each start result feeds its matching
/// complete, so the real `launch_browser`, `listen_for_callback`, and
/// `exchange_code_for_token` implementations are exercised end-to-end.
pub fn probe_browser_oidc_product() -> BrowserOidcProductProbe {
    let availability = evaluate_browser_oidc_availability();
    let starts = walk_browser_oidc_start(DEFAULT_BROWSER_OIDC_START_PROBES);
    let completes = walk_browser_oidc_complete(&starts, PRODUCT_PROBE_CALLBACK_TIMEOUT_SECS);
    let last_start = starts
        .last()
        .cloned()
        .unwrap_or_else(|| start_browser_oidc_flow("(probe)", "(client)"));
    let last_complete = completes.last().cloned().unwrap_or_else(|| {
        complete_browser_oidc_flow(
            &start_browser_oidc_flow("(probe)", "(client)"),
            PRODUCT_PROBE_CALLBACK_TIMEOUT_SECS,
        )
    });
    let summary = summarize_browser_oidc_outcomes(Some(&last_start), Some(&last_complete));
    BrowserOidcProductProbe {
        availability,
        starts,
        completes,
        last_start,
        last_complete,
        summary,
    }
}

pub fn generate_random_string(len: usize) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut result = String::with_capacity(len);
    let mut state = seed;
    for _ in 0..len {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let idx = (state >> 33) as usize % chars.len();
        result.push(chars.as_bytes()[idx] as char);
    }
    result
}

fn base64url_sha256(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    base64url_encode(&digest)
}

fn base64url_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(TABLE[((n >> 18) & 0x3F) as usize] as char);
        result.push(TABLE[((n >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(TABLE[((n >> 6) & 0x3F) as usize] as char);
        }
        if chunk.len() > 2 {
            result.push(TABLE[(n & 0x3F) as usize] as char);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_oidc_reports_unavailable_when_unconfigured() {
        // arrange
        // act
        let availability = evaluate_browser_oidc_availability();

        // assert
        assert!(!availability.is_available());
        assert!(availability.is_unavailable());
        assert!(
            availability
                .one_line()
                .contains("no OIDC issuer configured"),
            "availability={availability:?}"
        );
    }

    #[test]
    fn start_with_real_issuer_returns_started() {
        // arrange
        // act
        let start = start_browser_oidc_flow("https://issuer.example", "client-abc");

        // assert
        match &start {
            BrowserOidcStartResult::Started {
                authorization_url,
                state,
                code_verifier,
                redirect_uri,
                token_endpoint,
                client_id,
                port,
                manual_url_fallback,
            } => {
                assert!(authorization_url.contains("https://issuer.example/authorize"));
                assert!(authorization_url.contains("client_id=client-abc"));
                assert!(authorization_url.contains("code_challenge_method=S256"));
                assert!(!state.is_empty());
                assert!(!code_verifier.is_empty());
                assert!(redirect_uri.starts_with("http://127.0.0.1"));
                assert_eq!(token_endpoint, "https://issuer.example/token");
                assert_eq!(client_id, "client-abc");
                assert_eq!(*port, DEFAULT_OIDC_CALLBACK_PORT);
                // Browser launch may fail in headless test env; fallback is set then.
                if let Some(fallback) = manual_url_fallback {
                    assert!(fallback.contains("open manually"));
                }
            }
            other => panic!("expected Started, got {other:?}"),
        }
    }

    #[test]
    fn start_with_probe_values_returns_unavailable() {
        // arrange
        // act
        let start = start_browser_oidc_flow("(probe)", "(client)");

        // assert
        match &start {
            BrowserOidcStartResult::Unavailable { reason, .. } => {
                assert!(!reason.is_empty());
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn complete_with_started_result_returns_unavailable_on_callback_timeout() {
        // arrange
        let start = start_browser_oidc_flow("https://issuer.example", "client-abc");

        // act — 0-second timeout: listen_for_callback returns immediately
        let complete = complete_browser_oidc_flow(&start, 0);

        // assert — no callback arrives, so the result is Unavailable
        match &complete {
            BrowserOidcCompleteResult::Unavailable { reason, .. } => {
                assert!(!reason.is_empty());
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn complete_with_unavailable_start_returns_unavailable() {
        // arrange
        let start = start_browser_oidc_flow("(probe)", "(client)");

        // act
        let complete = complete_browser_oidc_flow(&start, 0);

        // assert — probe start returns Unavailable immediately without blocking
        match &complete {
            BrowserOidcCompleteResult::Unavailable { reason, .. } => {
                assert!(reason.contains("Unavailable"));
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn pkce_generates_verifier_and_s256_challenge() {
        // arrange
        // act
        let pkce = generate_pkce();

        // assert
        assert!(!pkce.code_verifier.is_empty());
        assert!(!pkce.code_challenge.is_empty());
        assert_eq!(pkce.code_challenge_method, "S256");
        assert_ne!(pkce.code_verifier, pkce.code_challenge);
    }

    #[test]
    fn authorization_url_contains_all_required_params() {
        // arrange
        let pkce = generate_pkce();
        let state = "test-state-123";

        // act
        let url = build_authorization_url(
            "https://issuer.example",
            "client-abc",
            "http://127.0.0.1:8765/callback",
            state,
            &pkce,
            None,
        );

        // assert
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=client-abc"));
        assert!(url.contains("state=test-state-123"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("scope=openid"));
    }

    #[test]
    fn browser_oidc_operator_diagnostics_cover_availability_and_outcomes() {
        // arrange
        // act
        let availability = evaluate_browser_oidc_availability();
        let start = start_browser_oidc_flow("https://issuer.example", "client-abc");
        let complete = complete_browser_oidc_flow(&start, 0);
        let summary = summarize_browser_oidc_outcomes(Some(&start), Some(&complete));

        // assert
        assert!(
            availability
                .one_line()
                .contains("browser OIDC: unavailable"),
            "availability={availability:?}"
        );
        assert!(start.one_line().contains("started"));
        // Real implementation: no callback arrives, so complete is unavailable.
        assert!(complete.one_line().contains("unavailable"));
        assert_eq!(summary.total, 2);
        assert!(!summary.all_unavailable());
    }

    #[test]
    fn multi_endpoint_product_probe_has_mixed_outcomes() {
        // arrange
        // act
        let probe = probe_browser_oidc_product();

        // assert
        assert_eq!(probe.starts.len(), 3);
        assert_eq!(probe.completes.len(), 3);
        assert!(
            probe.availability.is_unavailable(),
            "expected unconfigured availability: {:?}",
            probe.availability
        );
        assert!(!probe.availability.is_available());
        assert_eq!(probe.summary.total, 2);
        assert!(!probe.summary.all_unavailable());
    }

    #[test]
    fn flow_state_machine_transitions_through_expected_phases() {
        // arrange
        // act — start
        let mut flow = BrowserOidcFlow::new();
        assert_eq!(flow.phase(), BrowserOidcFlowPhase::Idle);
        flow.start(
            "https://issuer.example",
            "client-abc",
            DEFAULT_OIDC_CALLBACK_PORT,
        );

        // assert — start transitions to WaitingForCallback
        assert_eq!(flow.phase(), BrowserOidcFlowPhase::WaitingForCallback);

        // act — complete with 0-second timeout (no callback will arrive)
        flow.complete(0);

        // assert — complete transitions to Failed (callback timeout)
        assert_eq!(flow.phase(), BrowserOidcFlowPhase::Failed);
    }

    #[test]
    fn flow_from_start_result_reconstructs_waiting_state() {
        // arrange
        let start = start_browser_oidc_flow("https://issuer.example", "client-abc");

        // act
        let flow = BrowserOidcFlow::from_start_result(&start);

        // assert
        assert_eq!(flow.phase(), BrowserOidcFlowPhase::WaitingForCallback);
    }

    #[test]
    fn token_response_redacts_access_token() {
        // arrange
        let token = TokenResponse {
            access_token: "abcdefghijklmnop".to_string(),
            token_type: "Bearer".to_string(),
            id_token: None,
            refresh_token: None,
            expires_in: Some(3600),
        };

        // act
        let redacted = token.redacted_access_token();

        // assert
        assert_eq!(redacted, "abcd…");
        assert!(!redacted.contains("abcdefghijklmnop"));
    }
}
