use super::{CliIo, Method};
use harness_core::auth::{
    anthropic::{
        login_anthropic_browser, login_anthropic_copy_code, AnthropicLoginInteraction,
        AnthropicOAuthClient, AnthropicOAuthError,
    },
    anthropic_subscription::{account_name_prompt, commit_login, existing_accounts},
    codex::*,
    copilot::*,
    CredentialStore, ProviderId, ReqwestAuthHttpClient,
};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

/// Where a running login reads pasted input: the TUI's channel, the terminal, or piped stdin.
pub(super) struct LoginInput {
    pub(super) lines: tokio::sync::mpsc::UnboundedReceiver<String>,
    pub(super) callback_host: String,
    pub(super) environment: std::collections::BTreeMap<String, String>,
    /// The TUI closes its input channel to cancel; a terminal's or pipe's end is not a cancel.
    pub(super) closed_cancels: bool,
}

pub(super) fn input_lines(
    io: &mut CliIo<'_>,
    deps: &crate::CliDeps,
) -> tokio::sync::mpsc::UnboundedReceiver<String> {
    use std::io::{BufRead, Read};
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    if let Some(input) = deps.interactive_input() {
        std::thread::spawn(move || {
            while let Some(line) = input.lock().ok().and_then(|receiver| receiver.recv().ok()) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    } else if io.stdin_is_terminal {
        // A login that finishes through the browser leaves this read pending until exit.
        std::thread::spawn(move || {
            for line in std::io::stdin().lock().lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
    } else {
        let mut text = String::new();
        let _ = io.stdin.take(65_536).read_to_string(&mut text);
        for line in text.lines() {
            let _ = tx.send(line.to_owned());
        }
    }
    rx
}

struct CliAnthropicInteraction {
    output: tokio::sync::mpsc::UnboundedSender<String>,
    lines: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<String>>,
    /// Copy-code needs the paste; the browser flow can also finish through its callback.
    paste_required: bool,
    closed_cancels: bool,
}

#[async_trait::async_trait]
impl AnthropicLoginInteraction for CliAnthropicInteraction {
    fn auth_url(&self, url: &str, instructions: &str) {
        let _ = self
            .output
            .send(format!("Open this URL to sign in:\n{url}\n{instructions}"));
        if !self.paste_required
            && let Err(error) = harness_core::browser_oidc::launch_browser(url)
        {
            let _ = self.output.send(error);
        }
    }
    fn progress(&self, message: &str) {
        let _ = self.output.send(message.to_owned());
    }
    async fn manual_code(
        &self,
        message: &str,
        placeholder: &str,
    ) -> Result<String, AnthropicOAuthError> {
        let _ = self.output.send(format!("{message} ({placeholder})"));
        match self.lines.lock().await.recv().await {
            Some(line) => Ok(line),
            None if self.paste_required || self.closed_cancels => {
                Err(AnthropicOAuthError("Login cancelled".into()))
            }
            None => std::future::pending().await,
        }
    }
}

use harness_providers::anthropic_subscription::executable::{
    describe_claude_code_executable, INSTALL_GUIDANCE,
};

async fn anthropic_subscription(
    method: Method,
    store: &CredentialStore,
    io: &mut CliIo<'_>,
    input: LoginInput,
    http: Arc<ReqwestAuthHttpClient>,
) -> Result<(), String> {
    let (output, mut printed) = tokio::sync::mpsc::unbounded_channel::<String>();
    let interaction = CliAnthropicInteraction {
        output,
        lines: tokio::sync::Mutex::new(input.lines),
        paste_required: !matches!(method, Method::Browser),
        closed_cancels: input.closed_cancels,
    };
    let client = AnthropicOAuthClient::new(http);
    let flow = async {
        match method {
            Method::Browser => {
                login_anthropic_browser(&client, &interaction, &input.callback_host).await
            }
            _ => login_anthropic_copy_code(&client, &interaction).await,
        }
    };
    tokio::pin!(flow);
    let credential = loop {
        tokio::select! {
            Some(line) = printed.recv() => {
                writeln!(io.stdout, "{line}").and_then(|()| io.stdout.flush()).map_err(|e| e.to_string())?;
            }
            result = &mut flow => break result,
        }
    };
    while let Ok(line) = printed.try_recv() {
        writeln!(io.stdout, "{line}").map_err(|e| e.to_string())?;
    }
    let credential = credential.map_err(|e| e.to_string())?;
    let name = match account_name_prompt(&existing_accounts(store)?) {
        None => "default".to_owned(),
        Some((message, default)) => {
            writeln!(io.stdout, "{message}")
                .and_then(|()| io.stdout.flush())
                .map_err(|e| e.to_string())?;
            let answer = tokio::time::timeout(Duration::from_secs(300), async {
                interaction.lines.lock().await.recv().await
            })
            .await
            .ok()
            .flatten()
            .map(|answer| answer.trim().to_owned())
            .filter(|answer| !answer.is_empty());
            answer.unwrap_or(default)
        }
    };
    commit_login(store, credential, &name)?;
    writeln!(io.stdout, "Anthropic Subscription account saved: {name}")
        .map_err(|e| e.to_string())?;
    if describe_claude_code_executable(&|name| input.environment.get(name).cloned()).is_err() {
        writeln!(
            io.stdout,
            "Claude Code is not installed, and this provider runs every turn through it. {INSTALL_GUIDANCE}"
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub(super) async fn login(
    id: &ProviderId,
    method: Method,
    enterprise: Option<&str>,
    store: &CredentialStore,
    io: &mut CliIo<'_>,
    input: LoginInput,
) -> Result<(), String> {
    let http = Arc::new(ReqwestAuthHttpClient::new().map_err(|e| e.to_string())?);
    tokio::time::timeout(Duration::from_secs(900), async {
        if id == &ProviderId::anthropic_subscription() {
            anthropic_subscription(method, store, io, input, http).await
        } else if id == &ProviderId::codex() {
            let client = CodexOAuthClient::new(http);
            match method {
                Method::Browser => browser(&client, store, io).await,
                _ => codex_device(&client, store, io).await,
            }
        } else if id == &ProviderId::github_copilot() && matches!(method, Method::Device) {
            let client = CopilotOAuthClient::new(http);
            let deployment = enterprise
                .map(CopilotDeployment::enterprise)
                .transpose()
                .map_err(|e| e.to_string())?
                .unwrap_or_else(CopilotDeployment::public);
            let device = client
                .start_device_authorization(&deployment)
                .await
                .map_err(|e| e.to_string())?;
            writeln!(
                io.stdout,
                "Open {} and enter {}",
                device.verification_uri, device.user_code
            )
            .and_then(|()| io.stdout.flush())
            .map_err(|e| e.to_string())?;
            let mut interval = device.interval_seconds;
            let mut wait = Duration::from_secs(interval);
            loop {
                tokio::time::sleep(wait).await;
                match client
                    .poll_device_token(&deployment, &device, interval)
                    .await
                    .map_err(|e| e.to_string())?
                {
                    CopilotDevicePoll::Pending { wait: next } => wait = next,
                    CopilotDevicePoll::SlowDown {
                        interval_seconds,
                        wait: next,
                    } => {
                        interval = interval_seconds;
                        wait = next;
                    }
                    CopilotDevicePoll::Authorized { access_token } => {
                        client
                            .store_access_token(&deployment, store, &access_token)
                            .map_err(|e| e.to_string())?;
                        return Ok(());
                    }
                }
            }
        } else {
            Err("this provider does not support the selected OAuth method; use api-key".into())
        }
    })
    .await
    .map_err(|_| "login timed out after 15 minutes".to_owned())?
}
async fn codex_device(
    client: &CodexOAuthClient,
    store: &CredentialStore,
    io: &mut CliIo<'_>,
) -> Result<(), String> {
    let device = client
        .start_device_authorization()
        .await
        .map_err(|e| e.to_string())?;
    writeln!(
        io.stdout,
        "Open {} and enter {}",
        device.verification_uri, device.user_code
    )
    .and_then(|()| io.stdout.flush())
    .map_err(|e| e.to_string())?;
    loop {
        match client
            .poll_device_authorization(&device)
            .await
            .map_err(|e| e.to_string())?
        {
            CodexDevicePoll::Pending => {
                tokio::time::sleep(Duration::from_secs(
                    device.interval_seconds.saturating_add(3),
                ))
                .await
            }
            CodexDevicePoll::Authorized {
                authorization_code,
                code_verifier,
            } => {
                let tokens = client
                    .exchange_authorization_code(
                        &authorization_code,
                        &format!("{CODEX_ISSUER}/deviceauth/callback"),
                        &PkceCodes {
                            verifier: code_verifier,
                            challenge: String::new(),
                        },
                    )
                    .await
                    .map_err(|e| e.to_string())?;
                client
                    .store_tokens(store, tokens)
                    .map_err(|e| e.to_string())?;
                return Ok(());
            }
        }
    }
}
async fn browser(
    client: &CodexOAuthClient,
    store: &CredentialStore,
    io: &mut CliIo<'_>,
) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, CODEX_OAUTH_PORT))
        .await
        .map_err(|_| "cannot bind the login callback on localhost:1455; use --method device")?;
    let state = generate_pkce().map_err(|e| e.to_string())?.verifier;
    let session = CodexLoopbackSession::new(generate_pkce().map_err(|e| e.to_string())?, state);
    writeln!(
        io.stdout,
        "Open this URL to sign in:\n{}",
        session.authorize_url
    )
    .and_then(|()| io.stdout.flush())
    .map_err(|e| e.to_string())?;
    if let Err(error) = harness_core::browser_oidc::launch_browser(&session.authorize_url) {
        writeln!(io.stderr, "{error}").map_err(|e| e.to_string())?;
    }
    loop {
        let (mut socket, _) = listener.accept().await.map_err(|e| e.to_string())?;
        let mut request = String::new();
        let read = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::io::BufReader::new((&mut socket).take(16_385)).read_line(&mut request),
        )
        .await;
        if !matches!(read, Ok(Ok(_))) || request.len() > 16_384 {
            continue;
        }
        let mut parts = request.split_whitespace();
        let method = parts.next();
        let path = parts.next().unwrap_or("");
        if method != Some("GET") || !path.starts_with("/auth/callback?") {
            let _ = socket
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
            continue;
        }
        let result = client
            .complete_loopback_callback(&session, path, store)
            .await;
        let body = if result.is_ok() {
            codex_callback_success_html().to_owned()
        } else {
            codex_callback_error_html("Sign-in was rejected. Return to the terminal.")
        };
        let status = if result.is_ok() {
            "200 OK"
        } else {
            "400 Bad Request"
        };
        let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Security-Policy: default-src 'none'\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            socket.write_all(response.as_bytes()),
        )
        .await;
        match result {
            Err(CodexOAuthError::InvalidState | CodexOAuthError::CallbackRejected { .. }) => {
                continue
            }
            result => return result.map(|_| ()).map_err(|e| e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The browser flow may finish through its callback, so a closed pipe leaves the paste wait
    /// pending; the TUI closes its channel to cancel, which must end the login and free the port.
    #[tokio::test]
    async fn a_closed_tui_channel_cancels_even_the_browser_flow() {
        for (closed_cancels, cancelled) in [(true, true), (false, false)] {
            let (output, _printed) = tokio::sync::mpsc::unbounded_channel();
            let (lines_tx, lines) = tokio::sync::mpsc::unbounded_channel::<String>();
            drop(lines_tx);
            let interaction = CliAnthropicInteraction {
                output,
                lines: tokio::sync::Mutex::new(lines),
                paste_required: false,
                closed_cancels,
            };
            let waited = tokio::time::timeout(
                Duration::from_millis(200),
                interaction.manual_code("Paste", "code"),
            )
            .await;
            assert_eq!(matches!(waited, Ok(Err(_))), cancelled);
        }
    }
}
