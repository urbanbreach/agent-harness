use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use harness_core::auth::codex::{
    codex_callback_error_html, codex_callback_success_html, pkce_challenge,
    AuthHttpClient as CodexAuthHttpClient, AuthHttpRequest, AuthHttpResponse, CodexDevicePoll,
    CodexLoopbackSession, CodexOAuthClient, CodexOAuthError, PkceCodes, CODEX_ISSUER,
    CODEX_OAUTH_PORT,
};
use harness_core::auth::copilot::{
    CopilotAuthHttpClient, CopilotDeployment, CopilotDevicePoll, CopilotOAuthClient,
    CopilotOAuthError, COPILOT_SCOPE,
};
use harness_core::auth::{
    AuthProviderId, CredentialClock, CredentialStore, StoredCredential, SystemCredentialClock,
};

use crate::CliIo;

use super::prompt_ui::{
    auth_prompt_io_error, auth_prompt_terminal_events_enabled, clack_log_info, clack_log_success,
    clack_outro, prompt_input, RawModeGuard,
};
use super::support::{auth_oauth_error, credential_store_error, non_empty};
use super::{AuthLoginCommand, AuthLoginUi};

const DEFAULT_DEVICE_POLLS: usize = 120;
const CODEX_BROWSER_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MANUAL_CALLBACK_POLL_INTERVAL: Duration = Duration::from_millis(50);

pub(super) fn store_api_key_login(
    auth_provider: AuthProviderId,
    command: AuthLoginCommand,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
) -> i32 {
    if auth_provider != AuthProviderId::Codex {
        let _ = writeln!(
            io.stderr,
            "auth login failed: api-key login is supported for codex only in V1"
        );
        return 2;
    }
    if !command.api_key_stdin {
        let _ = writeln!(
            io.stderr,
            "auth login failed: pass --api-key-stdin and provide the key on stdin"
        );
        return 2;
    }
    let mut body = String::new();
    if let Err(err) = io.stdin.read_to_string(&mut body) {
        let _ = writeln!(io.stderr, "auth login failed: failed to read stdin: {err}");
        return 1;
    }
    let Some(api_key) = non_empty(&body).map(str::to_string) else {
        let _ = writeln!(
            io.stderr,
            "auth login failed: stdin did not contain an API key"
        );
        return 2;
    };
    store_api_key_value(auth_provider, api_key, io, store, AuthLoginUi::Plain)
}

pub(super) fn store_interactive_api_key_login(
    auth_provider: AuthProviderId,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
) -> i32 {
    if auth_provider != AuthProviderId::Codex {
        let _ = writeln!(
            io.stderr,
            "auth login failed: api-key login is supported for codex only in V1"
        );
        return 2;
    }
    let api_key = match prompt_input(io, "Enter your API key", None, true) {
        Ok(Some(api_key)) => api_key,
        Ok(None) => return 1,
        Err(err) => return auth_prompt_io_error(err, io.stderr),
    };
    store_api_key_value(auth_provider, api_key, io, store, AuthLoginUi::Interactive)
}

fn store_api_key_value(
    auth_provider: AuthProviderId,
    api_key: String,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    ui: AuthLoginUi,
) -> i32 {
    let credential =
        StoredCredential::api_key(auth_provider, api_key, SystemCredentialClock.now_rfc3339());
    match store.save(&credential) {
        Ok(()) => {
            if ui == AuthLoginUi::Interactive {
                let _ = clack_outro(io.stdout, "Done");
            } else {
                let _ = writeln!(
                    io.stdout,
                    "stored api_key credential for {} (secret redacted)",
                    auth_provider
                );
            }
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "auth login failed: {err}");
            1
        }
    }
}

pub(super) fn store_mock_oauth_login(
    auth_provider: AuthProviderId,
    command: AuthLoginCommand,
    token: &str,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    ui: AuthLoginUi,
) -> i32 {
    let Some(token) = non_empty(token).map(str::to_string) else {
        let _ = writeln!(io.stderr, "auth login failed: mock token was empty");
        return 2;
    };
    let refresh = command
        .mock_refresh_token
        .as_deref()
        .and_then(non_empty)
        .unwrap_or(&token)
        .to_string();
    let mut credential = StoredCredential::oauth(
        auth_provider,
        token,
        refresh,
        command.expires_at.clone(),
        SystemCredentialClock.now_rfc3339(),
    );
    credential.account_id = command.account_id.clone();
    if auth_provider == AuthProviderId::GithubCopilot {
        credential.scopes = vec![COPILOT_SCOPE.to_string()];
        if let Some(input) = command.enterprise_url.as_deref() {
            match CopilotDeployment::enterprise(input) {
                Ok(CopilotDeployment::Enterprise { domain }) => {
                    credential.enterprise_url = Some(domain);
                }
                Ok(CopilotDeployment::Public) => {}
                Err(err) => {
                    let _ = writeln!(io.stderr, "auth login failed: {err}");
                    return 2;
                }
            }
        }
    }
    match store.save(&credential) {
        Ok(()) => {
            if ui == AuthLoginUi::Interactive {
                let _ = clack_log_success(io.stdout, "Login successful");
                let _ = clack_outro(io.stdout, "Done");
            } else {
                let _ = writeln!(
                    io.stdout,
                    "stored oauth credential for {} (secret redacted)",
                    auth_provider
                );
            }
            0
        }
        Err(err) => {
            let _ = writeln!(io.stderr, "auth login failed: {err}");
            1
        }
    }
}

pub(super) fn run_device_login(
    auth_provider: AuthProviderId,
    enterprise_url: Option<String>,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    ui: AuthLoginUi,
) -> i32 {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "auth login failed: failed to create runtime: {err}"
            );
            return 1;
        }
    };

    match auth_provider {
        AuthProviderId::Codex => runtime.block_on(run_codex_device_login(io, store, ui)),
        AuthProviderId::GithubCopilot => {
            runtime.block_on(run_copilot_device_login(enterprise_url, io, store, ui))
        }
    }
}

async fn run_codex_device_login(
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    ui: AuthLoginUi,
) -> i32 {
    let client = CodexOAuthClient::new(Arc::new(ReqwestCodexAuthClient::new()));
    let device = match client.start_device_authorization().await {
        Ok(device) => device,
        Err(err) => return auth_oauth_error("auth login failed", err, io.stderr),
    };
    if ui == AuthLoginUi::Interactive {
        let _ = clack_log_info(io.stdout, &format!("Go to: {}", device.verification_uri));
        let _ = clack_log_info(io.stdout, &format!("Enter code: {}", device.user_code));
        let _ = clack_log_info(io.stdout, "Waiting for authorization...");
    } else {
        let _ = writeln!(io.stdout, "Harness Codex device login");
        let _ = writeln!(io.stdout, "Open {}", device.verification_uri);
        let _ = writeln!(io.stdout, "Enter code {}", device.user_code);
    }

    for _ in 0..DEFAULT_DEVICE_POLLS {
        match client.poll_device_authorization(&device).await {
            Ok(CodexDevicePoll::Pending) => {
                tokio::time::sleep(Duration::from_secs(device.interval_seconds.max(1))).await;
            }
            Ok(CodexDevicePoll::Authorized {
                authorization_code,
                code_verifier,
            }) => {
                let token_response = match client
                    .exchange_authorization_code(
                        &authorization_code,
                        "https://auth.openai.com/deviceauth/callback",
                        &PkceCodes {
                            verifier: code_verifier,
                            challenge: String::new(),
                        },
                    )
                    .await
                {
                    Ok(tokens) => tokens,
                    Err(err) => return auth_oauth_error("auth login failed", err, io.stderr),
                };
                let _credential = match client.store_tokens(store, token_response).await {
                    Ok(credential) => credential,
                    Err(err) => return auth_oauth_error("auth login failed", err, io.stderr),
                };
                if ui == AuthLoginUi::Interactive {
                    let _ = clack_log_success(io.stdout, "Login successful");
                    let _ = clack_outro(io.stdout, "Done");
                } else {
                    let _ = writeln!(
                        io.stdout,
                        "stored oauth credential for codex (secret redacted)"
                    );
                }
                return 0;
            }
            Err(err) => return auth_oauth_error("auth login failed", err, io.stderr),
        }
    }

    let _ = writeln!(io.stderr, "auth login failed: Codex device login timed out");
    1
}

async fn run_copilot_device_login(
    enterprise_url: Option<String>,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    ui: AuthLoginUi,
) -> i32 {
    let deployment = match enterprise_url.as_deref() {
        Some(input) => match CopilotDeployment::enterprise(input) {
            Ok(deployment) => deployment,
            Err(err) => return auth_oauth_error("auth login failed", err, io.stderr),
        },
        None => CopilotDeployment::public(),
    };
    let client = CopilotOAuthClient::new(Arc::new(ReqwestCopilotAuthClient::new()));
    let device = match client.start_device_authorization(&deployment).await {
        Ok(device) => device,
        Err(err) => return auth_oauth_error("auth login failed", err, io.stderr),
    };
    if ui == AuthLoginUi::Interactive {
        let _ = clack_log_info(io.stdout, &format!("Go to: {}", device.verification_uri));
        let _ = clack_log_info(io.stdout, &format!("Enter code: {}", device.user_code));
        let _ = clack_log_info(io.stdout, "Waiting for authorization...");
    } else {
        let _ = writeln!(io.stdout, "Harness GitHub Copilot device login");
        let _ = writeln!(io.stdout, "Open {}", device.verification_uri);
        let _ = writeln!(io.stdout, "Enter code {}", device.user_code);
    }

    let mut interval = device.interval_seconds;
    for _ in 0..DEFAULT_DEVICE_POLLS {
        match client
            .poll_device_token(&deployment, &device, interval)
            .await
        {
            Ok(CopilotDevicePoll::Pending { wait }) => tokio::time::sleep(wait).await,
            Ok(CopilotDevicePoll::SlowDown {
                interval_seconds,
                wait,
            }) => {
                interval = interval_seconds;
                tokio::time::sleep(wait).await;
            }
            Ok(CopilotDevicePoll::Authorized { access_token }) => {
                let mut credential = StoredCredential::oauth(
                    AuthProviderId::GithubCopilot,
                    access_token.clone(),
                    access_token,
                    None,
                    SystemCredentialClock.now_rfc3339(),
                );
                credential.scopes = vec![COPILOT_SCOPE.to_string()];
                if let CopilotDeployment::Enterprise { domain } = deployment {
                    credential.enterprise_url = Some(domain);
                }
                return match store.save(&credential) {
                    Ok(()) => {
                        if ui == AuthLoginUi::Interactive {
                            let _ = clack_log_success(io.stdout, "Login successful");
                            let _ = clack_outro(io.stdout, "Done");
                        } else {
                            let _ = writeln!(
                                io.stdout,
                                "stored oauth credential for github-copilot (secret redacted)"
                            );
                        }
                        0
                    }
                    Err(err) => credential_store_error("auth login failed", err, io.stderr),
                };
            }
            Err(err) => return auth_oauth_error("auth login failed", err, io.stderr),
        }
    }

    let _ = writeln!(
        io.stderr,
        "auth login failed: GitHub Copilot device login timed out"
    );
    1
}

pub(super) fn run_codex_browser_login(
    auth_provider: AuthProviderId,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    ui: AuthLoginUi,
) -> i32 {
    if auth_provider != AuthProviderId::Codex {
        let _ = writeln!(
            io.stderr,
            "auth login failed: browser login is supported for codex only in V1"
        );
        return 2;
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "auth login failed: failed to create runtime: {err}"
            );
            return 1;
        }
    };

    runtime.block_on(run_codex_browser_login_with_client(
        CODEX_ISSUER,
        Arc::new(ReqwestCodexAuthClient::new()),
        CODEX_OAUTH_PORT,
        io,
        store,
        ui,
    ))
}

pub(super) async fn run_codex_browser_login_with_client(
    issuer: &str,
    http: Arc<dyn CodexAuthHttpClient>,
    port: u16,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    ui: AuthLoginUi,
) -> i32 {
    let listener = match tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))
        .await
    {
        Ok(listener) => listener,
        Err(err) => {
            let _ = writeln!(
                io.stderr,
                "auth login failed: could not bind Codex loopback callback on 127.0.0.1:{port}: {err}"
            );
            return 1;
        }
    };
    let redirect_uri = format!(
        "http://localhost:{}/auth/callback",
        listener
            .local_addr()
            .map(|addr| addr.port())
            .unwrap_or(port)
    );
    let pkce = match harness_core::auth::codex::generate_pkce() {
        Ok(pkce) => pkce,
        Err(err) => return auth_oauth_error("auth login failed", err, io.stderr),
    };
    let state = codex_browser_state(&pkce);
    let session = CodexLoopbackSession::with_redirect_uri(pkce, state, redirect_uri, issuer);
    let client = CodexOAuthClient::new(http).with_issuer(issuer);
    complete_codex_browser_loopback(
        listener,
        client,
        session,
        io,
        store,
        CODEX_BROWSER_TIMEOUT,
        ui,
    )
    .await
}

async fn complete_codex_browser_loopback(
    listener: tokio::net::TcpListener,
    client: CodexOAuthClient,
    session: CodexLoopbackSession,
    io: &mut CliIo<'_>,
    store: &CredentialStore,
    timeout: Duration,
    ui: AuthLoginUi,
) -> i32 {
    let terminal_manual_callback = auth_prompt_terminal_events_enabled();
    if ui == AuthLoginUi::Interactive {
        let _ = clack_log_info(io.stdout, &format!("Go to: {}", session.authorize_url));
        let _ = clack_log_info(
            io.stdout,
            "Complete authorization in your browser. This window will close automatically.",
        );
        if terminal_manual_callback {
            let _ = clack_log_info(
                io.stdout,
                "If the browser cannot reach this SSH host, paste the final localhost callback URL here and press Enter.",
            );
        }
        let _ = clack_log_info(io.stdout, "Waiting for authorization...");
    } else {
        let _ = writeln!(io.stdout, "Harness Codex browser login");
        let _ = writeln!(io.stdout, "Open {}", session.authorize_url);
        if terminal_manual_callback {
            let _ = writeln!(
                io.stdout,
                "If the browser cannot reach this SSH host, paste the final localhost callback URL here and press Enter."
            );
        }
        let _ = writeln!(
            io.stdout,
            "Waiting for callback on {} (timeout {}s)",
            session.redirect_uri,
            timeout.as_secs()
        );
    }

    let deadline = tokio::time::Instant::now() + timeout;
    let completion = if terminal_manual_callback {
        let _raw_mode = RawModeGuard::new(true);
        tokio::select! {
            result = tokio::time::timeout_at(
                deadline,
                receive_codex_loopback_callback(&listener, &client, &session, store),
            ) => match result {
                Ok(result) => CodexBrowserCompletion::Credential(result),
                Err(_) => CodexBrowserCompletion::Timeout,
            },
            manual = read_terminal_manual_callback_url(deadline) => match manual {
                Ok(ManualCallbackInput::Url(callback_url)) => match tokio::time::timeout_at(
                    deadline,
                    complete_codex_pasted_callback(&client, &session, store, &callback_url),
                ).await {
                    Ok(result) => CodexBrowserCompletion::Credential(result),
                    Err(_) => CodexBrowserCompletion::Timeout,
                },
                Ok(ManualCallbackInput::Cancelled) => CodexBrowserCompletion::Cancelled,
                Ok(ManualCallbackInput::Timeout) => CodexBrowserCompletion::Timeout,
                Err(err) => CodexBrowserCompletion::InputError(err),
            },
        }
    } else {
        match tokio::time::timeout(
            timeout,
            receive_codex_loopback_callback(&listener, &client, &session, store),
        )
        .await
        {
            Ok(result) => CodexBrowserCompletion::Credential(result),
            Err(_) => CodexBrowserCompletion::Timeout,
        }
    };

    match completion {
        CodexBrowserCompletion::Credential(Ok(_credential)) => {
            if ui == AuthLoginUi::Interactive {
                let _ = clack_log_success(io.stdout, "Login successful");
                let _ = clack_outro(io.stdout, "Done");
            } else {
                let _ = writeln!(
                    io.stdout,
                    "stored oauth credential for codex (secret redacted)"
                );
            }
            0
        }
        CodexBrowserCompletion::Credential(Err(err)) => {
            auth_oauth_error("auth login failed", err, io.stderr)
        }
        CodexBrowserCompletion::InputError(err) => auth_prompt_io_error(err, io.stderr),
        CodexBrowserCompletion::Cancelled => 1,
        CodexBrowserCompletion::Timeout => {
            auth_oauth_error("auth login failed", session.timeout_error(), io.stderr)
        }
    }
}

enum CodexBrowserCompletion {
    Credential(Result<StoredCredential, CodexOAuthError>),
    InputError(io::Error),
    Cancelled,
    Timeout,
}

enum ManualCallbackInput {
    Url(String),
    Cancelled,
    Timeout,
}

async fn read_terminal_manual_callback_url(
    deadline: tokio::time::Instant,
) -> io::Result<ManualCallbackInput> {
    let mut value = String::new();

    loop {
        if tokio::time::Instant::now() >= deadline {
            return Ok(ManualCallbackInput::Timeout);
        }
        if crossterm::event::poll(Duration::ZERO).map_err(io::Error::other)? {
            match crossterm::event::read().map_err(io::Error::other)? {
                Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                    KeyCode::Enter => {
                        let callback_url = value.trim().to_string();
                        if !callback_url.is_empty() {
                            return Ok(ManualCallbackInput::Url(callback_url));
                        }
                    }
                    KeyCode::Esc => return Ok(ManualCallbackInput::Cancelled),
                    KeyCode::Backspace => {
                        value.pop();
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(ManualCallbackInput::Cancelled);
                    }
                    KeyCode::Char(ch) if !ch.is_control() => {
                        value.push(ch);
                    }
                    _ => {}
                },
                Event::Paste(text) => {
                    let callback_url = text.trim().to_string();
                    if !callback_url.is_empty() {
                        return Ok(ManualCallbackInput::Url(callback_url));
                    }
                }
                _ => {}
            }
        }
        tokio::time::sleep(MANUAL_CALLBACK_POLL_INTERVAL).await;
    }
}

pub(super) async fn complete_codex_pasted_callback(
    client: &CodexOAuthClient,
    session: &CodexLoopbackSession,
    store: &CredentialStore,
    callback_url: &str,
) -> Result<StoredCredential, CodexOAuthError> {
    client
        .complete_loopback_callback(session, callback_url, store)
        .await
}

async fn receive_codex_loopback_callback(
    listener: &tokio::net::TcpListener,
    client: &CodexOAuthClient,
    session: &CodexLoopbackSession,
    store: &CredentialStore,
) -> Result<StoredCredential, CodexOAuthError> {
    let (stream, _) = listener
        .accept()
        .await
        .map_err(|err| CodexOAuthError::Http {
            message: format!("loopback accept failed: {err}"),
        })?;
    handle_codex_loopback_stream(stream, client, session, store).await
}

pub(super) async fn handle_codex_loopback_stream<S>(
    mut stream: S,
    client: &CodexOAuthClient,
    session: &CodexLoopbackSession,
    store: &CredentialStore,
) -> Result<StoredCredential, CodexOAuthError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut buffer = [0_u8; 16 * 1024];
    let bytes_read = stream
        .read(&mut buffer)
        .await
        .map_err(|err| CodexOAuthError::Http {
            message: format!("loopback read failed: {err}"),
        })?;
    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let target = loopback_request_target(&request)?;
    let callback_url = if target.starts_with("http://") || target.starts_with("https://") {
        target
    } else {
        format!("{}{}", loopback_origin(&session.redirect_uri), target)
    };

    let result = client
        .complete_loopback_callback(session, &callback_url, store)
        .await;
    let (status, body) = match &result {
        Ok(_) => (200_u16, codex_callback_success_html().to_string()),
        Err(err) => (400_u16, codex_callback_error_html(&err.to_string())),
    };
    let response = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        if status == 200 { "OK" } else { "Bad Request" },
        body.len(),
        body
    );
    stream
        .write_all(response.as_bytes())
        .await
        .map_err(|err| CodexOAuthError::Http {
            message: format!("loopback response failed: {err}"),
        })?;
    let _ = stream.shutdown().await;
    result
}

fn loopback_request_target(request: &str) -> Result<String, CodexOAuthError> {
    let line = request
        .lines()
        .next()
        .ok_or_else(|| CodexOAuthError::Http {
            message: "loopback request was empty".to_string(),
        })?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    if method != "GET" || target.is_empty() {
        return Err(CodexOAuthError::Http {
            message: "loopback request was not a GET callback".to_string(),
        });
    }
    Ok(target.to_string())
}

fn loopback_origin(redirect_uri: &str) -> String {
    redirect_uri
        .split_once("/auth/callback")
        .map(|(origin, _)| origin.to_string())
        .unwrap_or_else(|| "http://localhost".to_string())
}

fn codex_browser_state(pkce: &PkceCodes) -> String {
    let seed = format!("harness-codex-oauth-state:{}", pkce.verifier);
    pkce_challenge(&seed).chars().take(32).collect()
}

#[derive(Debug, Default)]
struct ReqwestCodexAuthClient {
    client: reqwest::Client,
}

impl ReqwestCodexAuthClient {
    fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl CodexAuthHttpClient for ReqwestCodexAuthClient {
    async fn send(&self, request: AuthHttpRequest) -> Result<AuthHttpResponse, CodexOAuthError> {
        let mut builder = self.client.post(&request.url).body(request.body);
        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        let response = builder.send().await.map_err(|err| CodexOAuthError::Http {
            message: err.to_string(),
        })?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(|err| CodexOAuthError::Http {
            message: err.to_string(),
        })?;
        Ok(AuthHttpResponse { status, body })
    }
}

#[derive(Debug, Default)]
struct ReqwestCopilotAuthClient {
    client: reqwest::Client,
}

impl ReqwestCopilotAuthClient {
    fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl CopilotAuthHttpClient for ReqwestCopilotAuthClient {
    async fn send(&self, request: AuthHttpRequest) -> Result<AuthHttpResponse, CopilotOAuthError> {
        let mut builder = self.client.post(&request.url).body(request.body);
        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        let response = builder
            .send()
            .await
            .map_err(|err| CopilotOAuthError::Http {
                message: err.to_string(),
            })?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|err| CopilotOAuthError::Http {
                message: err.to_string(),
            })?;
        Ok(AuthHttpResponse { status, body })
    }
}
