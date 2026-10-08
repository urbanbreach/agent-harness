//! Loopback callback listener and result pages.
use super::*;

pub struct CallbackCode {
    pub code: String,
    pub state: String,
}
pub struct CallbackListener {
    /// Bound loopback port; `None` when no port could be bound (manual-only login).
    pub port: Option<u16>,
    pub redirect_uri: String,
    pub callback_unavailable: Option<String>,
    code: Option<oneshot::Receiver<CallbackCode>>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl CallbackListener {
    /// The browser's code, or `None` in manual-only mode.
    pub async fn wait_for_code(&mut self) -> Option<CallbackCode> {
        match self.code.as_mut() {
            Some(code) => code.await.ok(),
            None => std::future::pending().await,
        }
    }
}
impl Drop for CallbackListener {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Binds the preferred port, then an ephemeral one.
///
/// When neither can be bound the login continues in manual mode with the registered
/// preferred-port redirect URI, so a pasted redirect URL still exchanges. The OAuth client
/// accepts any `localhost` port on `/callback`.
pub async fn start_callback_listener(
    host: &str,
    expected_state: &str,
) -> Result<CallbackListener, AnthropicOAuthError> {
    let mut last_failure = None;
    for port in [PREFERRED_CALLBACK_PORT, 0] {
        match tokio::net::TcpListener::bind((host, port)).await {
            Ok(listener) => {
                let port = listener.local_addr().map_or(port, |a| a.port());
                // The redirect names `localhost`, which browsers may resolve to `::1` first:
                // an IPv4 loopback listener also answers on IPv6 loopback when it can.
                let ipv6 = match listener.local_addr() {
                    Ok(address) if address.ip().is_loopback() && address.is_ipv4() => {
                        tokio::net::TcpListener::bind(("::1", port)).await.ok()
                    }
                    _ => None,
                };
                let (tx, rx) = oneshot::channel();
                let state = expected_state.to_owned();
                let task = tokio::spawn(serve_callbacks(listener, ipv6, port, state, tx));
                return Ok(CallbackListener {
                    port: Some(port),
                    redirect_uri: callback_redirect_uri(port),
                    callback_unavailable: None,
                    code: Some(rx),
                    task: Some(task),
                });
            }
            Err(e) => {
                let code = match e.kind() {
                    std::io::ErrorKind::PermissionDenied => "EACCES",
                    std::io::ErrorKind::AddrInUse => "EADDRINUSE",
                    _ if e.raw_os_error() == Some(1) => "EPERM",
                    _ => {
                        return Err(error(format!(
                            "Could not open OAuth callback listener at {host}:{port}: {e}"
                        )));
                    }
                };
                last_failure = Some(code);
            }
        }
    }
    Ok(CallbackListener {
        port: None,
        redirect_uri: callback_redirect_uri(PREFERRED_CALLBACK_PORT),
        callback_unavailable: Some(last_failure.unwrap_or("EADDRINUSE").into()),
        code: None,
        task: None,
    })
}

async fn accept_either(
    ipv4: &tokio::net::TcpListener,
    ipv6: Option<&tokio::net::TcpListener>,
) -> std::io::Result<tokio::net::TcpStream> {
    match ipv6 {
        Some(ipv6) => tokio::select! {
            accepted = ipv4.accept() => accepted.map(|(socket, _)| socket),
            accepted = ipv6.accept() => accepted.map(|(socket, _)| socket),
        },
        None => ipv4.accept().await.map(|(socket, _)| socket),
    }
}

async fn serve_callbacks(
    listener: tokio::net::TcpListener,
    ipv6: Option<tokio::net::TcpListener>,
    port: u16,
    expected_state: String,
    tx: oneshot::Sender<CallbackCode>,
) {
    let mut tx = Some(tx);
    while let Ok(mut socket) = accept_either(&listener, ipv6.as_ref()).await {
        let mut line = String::new();
        let read = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::io::BufReader::new((&mut socket).take(16_385)).read_line(&mut line),
        )
        .await;
        if !matches!(read, Ok(Ok(_))) || line.len() > 16_384 {
            continue;
        }
        let target = line.split_whitespace().nth(1).unwrap_or("");
        let (status, html, settle) = handle_callback(target, port, &expected_state);
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
            html.len()
        );
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            socket.write_all(response.as_bytes()),
        )
        .await;
        if let Some(code) = settle
            && let Some(tx) = tx.take()
        {
            let _ = tx.send(code);
        }
    }
}

fn handle_callback(
    target: &str,
    port: u16,
    expected_state: &str,
) -> (&'static str, String, Option<CallbackCode>) {
    let Ok(url) =
        reqwest::Url::parse(&format!("http://localhost:{port}")).and_then(|base| base.join(target))
    else {
        return ("500 Internal Server Error", "Internal error".into(), None);
    };
    if url.path() != CALLBACK_PATH {
        return (
            "404 Not Found",
            oauth_error_html("Callback route not found.", None),
            None,
        );
    }
    let get = |name: &str| {
        url.query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    if let Some(error) = get("error").filter(|e| !e.is_empty()) {
        return (
            "400 Bad Request",
            oauth_error_html(
                "Anthropic authentication did not complete.",
                Some(&format!("Error: {error}")),
            ),
            None,
        );
    }
    let (Some(code), Some(state)) = (
        get("code").filter(|c| !c.is_empty()),
        get("state").filter(|s| !s.is_empty()),
    ) else {
        return (
            "400 Bad Request",
            oauth_error_html("Missing code or state parameter.", None),
            None,
        );
    };
    if state != expected_state {
        return ("400 Bad Request", foreign_login_html(url.as_str()), None);
    }
    (
        "200 OK",
        oauth_success_html("Anthropic authentication completed. You can close this window."),
        Some(CallbackCode { code, state }),
    )
}

/// A callback whose `state` belongs to another login: either a different session's login
/// could not bind its own port and waits for a pasted redirect URL, or this tab is stale.
fn foreign_login_html(request_url: &str) -> String {
    oauth_error_html(
        "This browser login belongs to a different session of this app, or to an earlier login attempt.",
        Some(
            &[
                "If a session is still waiting for this login (it shows a prompt to paste the redirect URL), copy the full address from the browser's address bar and paste it there.",
                "Otherwise this attempt is stale: close this tab and run the login again from the session that needs it.",
                "",
                request_url,
            ]
            .join("\n"),
        ),
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn render_page(heading: &str, message: &str, details: Option<&str>) -> String {
    let heading = escape_html(heading);
    let details = details
        .map(|d| format!("<div class=\"details\">{}</div>", escape_html(d)))
        .unwrap_or_default();
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\" />\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />\n  <title>{heading}</title>\n  <style>\n    body {{ margin: 0; min-height: 100vh; display: flex; align-items: center; justify-content: center; padding: 24px; background: #09090b; color: #fafafa; font-family: ui-sans-serif, system-ui, sans-serif; text-align: center; }}\n    main {{ max-width: 560px; }}\n    h1 {{ margin: 0 0 10px; font-size: 28px; }}\n    p {{ margin: 0; line-height: 1.7; color: #a1a1aa; font-size: 15px; }}\n    .details {{ margin-top: 16px; font-family: ui-monospace, monospace; font-size: 13px; color: #a1a1aa; white-space: pre-wrap; word-break: break-word; }}\n  </style>\n</head>\n<body>\n  <main>\n    <h1>{heading}</h1>\n    <p>{}</p>\n    {details}\n  </main>\n</body>\n</html>",
        escape_html(message)
    )
}
pub fn oauth_success_html(message: &str) -> String {
    render_page("Authentication successful", message, None)
}
pub fn oauth_error_html(message: &str, details: Option<&str>) -> String {
    render_page("Authentication failed", message, details)
}
