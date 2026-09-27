use super::{CliIo, Method};
use harness_core::auth::{
    codex::*, copilot::*, CredentialStore, ProviderId, ReqwestAuthHttpClient,
};
use std::{sync::Arc, time::Duration};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

pub(super) async fn login(
    id: &ProviderId,
    method: Method,
    enterprise: Option<&str>,
    store: &CredentialStore,
    io: &mut CliIo<'_>,
) -> Result<(), String> {
    let http = Arc::new(ReqwestAuthHttpClient::new().map_err(|e| e.to_string())?);
    tokio::time::timeout(Duration::from_secs(900), async {
        if id == &ProviderId::codex() {
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
