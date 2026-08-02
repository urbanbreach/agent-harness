use super::*;
use crate::UnwrapOrAbort;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

#[derive(Debug)]
struct FixedClock(SystemTime);

impl CredentialClock for FixedClock {
    fn now(&self) -> SystemTime {
        self.0
    }
}

#[derive(Debug)]
struct MockAuthHttpClient {
    responses: Mutex<VecDeque<AuthHttpResponse>>,
    requests: Mutex<Vec<AuthHttpRequest>>,
    calls: AtomicUsize,
}

impl MockAuthHttpClient {
    fn new(responses: impl IntoIterator<Item = AuthHttpResponse>) -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(responses.into_iter().collect()),
            requests: Mutex::new(Vec::new()),
            calls: AtomicUsize::new(0),
        })
    }

    fn requests(&self) -> Vec<AuthHttpRequest> {
        self.requests.lock().unwrap_or_abort().clone()
    }
}

#[async_trait]
impl AuthHttpClient for MockAuthHttpClient {
    async fn send(&self, request: AuthHttpRequest) -> Result<AuthHttpResponse, CodexOAuthError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().unwrap_or_abort().push(request);
        self.responses
            .lock()
            .unwrap_or_abort()
            .pop_front()
            .ok_or_else(|| CodexOAuthError::Http {
                message: "no mocked auth response".to_string(),
            })
    }
}

fn response(status: u16, body: impl Into<String>) -> AuthHttpResponse {
    AuthHttpResponse {
        status,
        body: body.into(),
    }
}

fn client(http: Arc<MockAuthHttpClient>) -> CodexOAuthClient {
    CodexOAuthClient::new(http).with_clock(Arc::new(FixedClock(
        humantime::parse_rfc3339("2026-05-30T00:00:00Z").unwrap_or_abort(),
    )))
}

fn fake_jwt(claims: Value) -> String {
    format!(
        "header.{}.sig",
        base64_url_encode(serde_json::to_string(&claims).unwrap_or_abort().as_bytes())
    )
}

fn token_body(access: &str, refresh: &str, account_id: &str) -> String {
    serde_json::json!({
        "id_token": fake_jwt(serde_json::json!({ "chatgpt_account_id": account_id })),
        "access_token": access,
        "refresh_token": refresh,
        "expires_in": 3600
    })
    .to_string()
}

#[test]
fn codex_pkce_verifier_and_challenge_match_s256_base64url() {
    // arrange
    // act
    // assert
    let pkce = generate_pkce_from_entropy(&(0_u8..43).collect::<Vec<_>>());
    assert_eq!(pkce.verifier.len(), 43);
    assert!(pkce
        .verifier
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "-._~".contains(ch)));
    assert_eq!(
        pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    assert_eq!(pkce.challenge, pkce_challenge(&pkce.verifier));
}

#[tokio::test]
async fn codex_loopback_callback_validates_state_and_stores_tokens() {
    // arrange
    // act
    // assert
    let http = MockAuthHttpClient::new([response(
        200,
        token_body("access-new", "refresh-new", "acct-new"),
    )]);
    let client = client(Arc::clone(&http));
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = CredentialStore::new(temp.path());
    let session = CodexLoopbackSession::new(
        PkceCodes {
            verifier: "verifier-123".to_string(),
            challenge: "challenge-123".to_string(),
        },
        "state-123",
    );

    let credential = client
        .complete_loopback_callback(
            &session,
            "http://localhost:1455/auth/callback?code=code-123&state=state-123",
            &store,
        )
        .await
        .unwrap_or_abort();

    assert_eq!(credential.access_token.as_deref(), Some("access-new"));
    assert_eq!(credential.refresh_token.as_deref(), Some("refresh-new"));
    assert_eq!(credential.account_id.as_deref(), Some("acct-new"));
    let requests = http.requests();
    assert_eq!(requests.len(), 1);
    assert!(requests[0].url.ends_with("/oauth/token"));
    assert!(requests[0].body.contains("grant_type=authorization_code"));
    assert!(requests[0].body.contains("code=code-123"));
    assert!(requests[0].body.contains("code_verifier=verifier-123"));
}

#[tokio::test]
async fn codex_loopback_rejects_bad_state_missing_code_and_timeout_without_storing() {
    // arrange
    // act
    // assert
    let http = MockAuthHttpClient::new([]);
    let client = client(Arc::clone(&http));
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = CredentialStore::new(temp.path());
    let session = CodexLoopbackSession::new(
        PkceCodes {
            verifier: "verifier-123".to_string(),
            challenge: "challenge-123".to_string(),
        },
        "state-123",
    );

    let bad_state = client
        .complete_loopback_callback(
            &session,
            "http://localhost:1455/auth/callback?code=code-123&state=wrong",
            &store,
        )
        .await
        .expect_err("bad state");
    assert!(matches!(bad_state, CodexOAuthError::InvalidState));
    let missing_code = client
        .complete_loopback_callback(
            &session,
            "http://localhost:1455/auth/callback?state=state-123",
            &store,
        )
        .await
        .expect_err("missing code");
    assert!(matches!(missing_code, CodexOAuthError::MissingCode));
    assert!(matches!(
        session.timeout_error(),
        CodexOAuthError::CallbackTimeout { .. }
    ));
    assert_eq!(http.calls.load(Ordering::SeqCst), 0);
    assert!(store.load(&ProviderId::codex()).unwrap_or_abort().is_none());
}

#[tokio::test]
async fn codex_device_flow_polls_pending_then_exchanges_and_stores_credential() {
    // arrange
    // act
    // assert
    let http = MockAuthHttpClient::new([
        response(
            200,
            serde_json::json!({
                "device_auth_id": "device-123",
                "user_code": "USER-123",
                "interval": 1
            })
            .to_string(),
        ),
        response(403, "{}"),
        response(
            200,
            serde_json::json!({
                "authorization_code": "auth-code-123",
                "code_verifier": "device-verifier-123"
            })
            .to_string(),
        ),
        response(
            200,
            token_body("device-access", "device-refresh", "acct-device"),
        ),
    ]);
    let client = client(Arc::clone(&http));
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = CredentialStore::new(temp.path());

    let credential = client
        .complete_device_flow(&store, 3)
        .await
        .unwrap_or_abort();

    assert_eq!(credential.access_token.as_deref(), Some("device-access"));
    assert_eq!(credential.refresh_token.as_deref(), Some("device-refresh"));
    assert_eq!(
        credential.expires_at.as_deref(),
        Some("2026-05-30T01:00:00Z")
    );
    assert_eq!(credential.account_id.as_deref(), Some("acct-device"));
    let requests = http.requests();
    assert_eq!(requests.len(), 4);
    assert!(requests[0]
        .url
        .ends_with("/api/accounts/deviceauth/usercode"));
    assert!(requests[1].url.ends_with("/api/accounts/deviceauth/token"));
    assert!(requests[3].body.contains("code=auth-code-123"));
    assert!(requests[3]
        .body
        .contains("code_verifier=device-verifier-123"));
}

#[test]
fn codex_account_id_extracts_claim_precedence() {
    // arrange
    // act
    // assert
    assert_eq!(
        extract_account_id_from_claims(&serde_json::json!({
            "chatgpt_account_id": "acct-direct",
            "organizations": [{ "id": "org-fallback" }]
        })),
        Some("acct-direct".to_string())
    );
    assert_eq!(
        extract_account_id_from_claims(&serde_json::json!({
            "https://api.openai.com/auth": { "chatgpt_account_id": "acct-namespaced" },
            "organizations": [{ "id": "org-fallback" }]
        })),
        Some("acct-namespaced".to_string())
    );
    assert_eq!(
        extract_account_id_from_claims(&serde_json::json!({
            "organizations": [{ "id": "org-only" }]
        })),
        Some("org-only".to_string())
    );
}

#[test]
fn codex_oauth_model_filter_matches_reference_gpt5_family() {
    // arrange
    // act
    // assert
    assert!(codex_oauth_model_allowed("gpt-5.5"));
    assert!(codex_oauth_model_allowed("gpt-5.6-experimental"));
    assert!(codex_oauth_model_allowed("gpt-5.3-codex"));
    assert!(!codex_oauth_model_allowed("gpt-4.1"));
    assert!(!codex_oauth_model_allowed("claude-sonnet-4"));
}
