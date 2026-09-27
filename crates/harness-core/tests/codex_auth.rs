use harness_core::auth::{codex::*, CredentialClock, CredentialStore, ProviderId};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

struct Clock;
impl CredentialClock for Clock {
    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000)
    }
}
#[derive(Default)]
struct Http {
    responses: Mutex<VecDeque<AuthHttpResponse>>,
    requests: Mutex<Vec<AuthHttpRequest>>,
}
#[async_trait::async_trait]
impl AuthHttpClient for Http {
    async fn send(&self, request: AuthHttpRequest) -> Result<AuthHttpResponse, CodexOAuthError> {
        self.requests
            .lock()
            .map_err(|_| CodexOAuthError::MissingCode)?
            .push(request);
        self.responses
            .lock()
            .map_err(|_| CodexOAuthError::MissingCode)?
            .pop_front()
            .ok_or(CodexOAuthError::MissingCode)
    }
}
fn response(status: u16, body: &str) -> AuthHttpResponse {
    AuthHttpResponse {
        status,
        body: body.into(),
    }
}
#[tokio::test]
async fn loopback_login_checks_state_before_exchange_and_stores_only_valid_tokens(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let store = CredentialStore::new(temp.path());
    let http = Arc::new(Http::default());
    let client = CodexOAuthClient::new(Arc::clone(&http) as Arc<dyn AuthHttpClient>)
        .with_clock(Arc::new(Clock));
    let pkce = generate_pkce()?;
    assert!(pkce.verifier.len() >= 43);
    assert_eq!(pkce.challenge, pkce_challenge(&pkce.verifier));
    assert_eq!(
        pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    let session = CodexLoopbackSession::new(pkce, "known-state");
    for callback in [
        "http://localhost:1455/auth/callback?state=wrong&code=stolen",
        "http://localhost:1455/auth/callback?state=known-state&state=wrong&code=stolen",
        "http://elsewhere.test/auth/callback?state=known-state&code=stolen",
    ] {
        assert!(client
            .complete_loopback_callback(&session, callback, &store)
            .await
            .is_err());
    }
    assert!(http.requests.lock().map_err(|_| "poison")?.is_empty());
    assert!(store.load(&ProviderId::codex())?.is_none());
    http.responses.lock().map_err(|_| "poison")?.push_back(response(200, r#"{"access_token":"private-access","refresh_token":"private-refresh","expires_in":60}"#));
    let saved = client
        .complete_loopback_callback(
            &session,
            "http://localhost:1455/auth/callback?state=known-state&code=a%2Bb%26c",
            &store,
        )
        .await?;
    assert_eq!(saved.access_token.as_deref(), Some("private-access"));
    assert_eq!(
        saved.expires_at,
        Some(humantime::format_rfc3339(Clock.now() + Duration::from_secs(60)).to_string())
    );
    assert!(!format!("{saved:?}").contains("private-"));
    let request = http.requests.lock().map_err(|_| "poison")?.remove(0);
    assert_eq!(request.url, format!("{CODEX_ISSUER}/oauth/token"));
    assert!(request.body.contains("code=a%2Bb%26c"));
    let before = std::fs::read(store.credential_path(&ProviderId::codex()))?;
    http.responses
        .lock()
        .map_err(|_| "poison")?
        .push_back(response(
            200,
            r#"{"access_token":"","refresh_token":"private-refresh"}"#,
        ));
    assert!(client
        .complete_loopback_callback(
            &session,
            "http://localhost:1455/auth/callback?state=known-state&code=valid",
            &store
        )
        .await
        .is_err());
    assert_eq!(
        std::fs::read(store.credential_path(&ProviderId::codex()))?,
        before
    );
    assert!(!codex_callback_error_html("<script>private</script>").contains("<script>"));
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn device_login_honors_pending_and_refresh_preserves_unrotated_token(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let store = CredentialStore::new(temp.path());
    let http = Arc::new(Http::default());
    http.responses.lock().map_err(|_| "poison")?.extend([
        response(200, r#"{"device_auth_id":"device","user_code":"123-456","interval":"1"}"#),
        response(403, "not authorized yet"),
        response(200, r#"{"authorization_code":"grant","code_verifier":"device-verifier"}"#),
        response(200, r#"{"access_token":"private-access","refresh_token":"private-refresh","expires_in":100}"#),
        response(200, r#"{"access_token":"private-new","expires_in":100}"#),
        response(401, "private-server-response"),
    ]);
    let client = CodexOAuthClient::new(Arc::clone(&http) as Arc<dyn AuthHttpClient>)
        .with_clock(Arc::new(Clock));
    let stored = client.complete_device_flow(&store, 3).await?;
    assert_eq!(stored.refresh_token.as_deref(), Some("private-refresh"));
    let refreshed = client.refresh_access_token("private-refresh").await?;
    assert_eq!(refreshed.refresh_token, "private-refresh");
    assert_eq!(refreshed.access_token, "private-new");
    assert!(!format!("{refreshed:?}").contains("private-"));
    let error = client
        .refresh_access_token("private-refresh")
        .await
        .err()
        .ok_or("expected refusal")?;
    assert!(!error.to_string().contains("private-"));
    assert_eq!(
        error.category(),
        harness_providers::ProviderErrorCategory::InvalidCredentials
    );
    assert_eq!(http.requests.lock().map_err(|_| "poison")?.len(), 6);
    Ok(())
}

#[tokio::test]
async fn oauth_transport_refuses_redirects_and_oversized_responses_before_reading_body(
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/oauth/token", listener.local_addr()?);
    let redirected = url.clone();
    let server = async {
        for response in [format!("HTTP/1.1 307 Temporary Redirect\r\nLocation: {redirected}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"), "HTTP/1.1 200 OK\r\nContent-Length: 1048577\r\nConnection: close\r\n\r\n".into()] {
            let (mut socket, _) = listener.accept().await?;
            let mut request = [0u8; 4096];
            let _ = socket.read(&mut request).await?;
            socket.write_all(response.as_bytes()).await?;
        }
        Ok::<_, std::io::Error>(())
    };
    let client = async {
        let http = harness_core::auth::ReqwestAuthHttpClient::new()?;
        let request = || AuthHttpRequest {
            method: AuthHttpMethod::Post,
            url: url.clone(),
            headers: Default::default(),
            body: "private-test-token".into(),
        };
        assert_eq!(http.send(request()).await?.status, 307);
        let error = http
            .send(request())
            .await
            .err()
            .ok_or("oversized response accepted")?;
        assert!(error.to_string().contains("exceeds"));
        assert!(!error.to_string().contains("private-test-token"));
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let (server, client) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(server, client)
    })
    .await?;
    server?;
    client?;
    Ok(())
}
