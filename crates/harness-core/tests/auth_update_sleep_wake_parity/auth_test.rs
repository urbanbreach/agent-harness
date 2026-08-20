use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use harness_core::auth::codex::{
    generate_pkce_from_entropy, AuthHttpClient, AuthHttpResponse, CodexLoopbackSession,
    CodexOAuthClient, CodexOAuthError,
};
use harness_core::auth::copilot::{
    CopilotAuthHttpClient, CopilotDeployment, CopilotOAuthClient, CopilotOAuthError,
};
use harness_core::auth::{
    CredentialClock, CredentialStore, OAuthTokenRefresher, ProviderCredentialManager, ProviderId,
    ResolvedCredentialSource, StoredCredential,
};
use harness_core::UnwrapOrAbort;
use harness_providers::ProviderErrorCategory;

struct FixedClock(SystemTime);

impl CredentialClock for FixedClock {
    fn now(&self) -> SystemTime {
        self.0
    }
}

struct CodexHttp(Mutex<VecDeque<AuthHttpResponse>>);

#[async_trait]
impl AuthHttpClient for CodexHttp {
    async fn send(
        &self,
        _: harness_core::auth::codex::AuthHttpRequest,
    ) -> Result<AuthHttpResponse, CodexOAuthError> {
        self.0
            .lock()
            .unwrap_or_abort()
            .pop_front()
            .ok_or_else(|| CodexOAuthError::Http {
                message: "missing test response".to_string(),
            })
    }
}

struct CopilotHttp(Mutex<VecDeque<AuthHttpResponse>>);

#[async_trait]
impl CopilotAuthHttpClient for CopilotHttp {
    async fn send(
        &self,
        _: harness_core::auth::codex::AuthHttpRequest,
    ) -> Result<AuthHttpResponse, CopilotOAuthError> {
        self.0
            .lock()
            .unwrap_or_abort()
            .pop_front()
            .ok_or_else(|| CopilotOAuthError::Http {
                message: "missing test response".to_string(),
            })
    }
}

#[tokio::test]
async fn public_codex_login_refresh_and_manifest_are_secret_safe() {
    // arrange — a public loopback authorization callback and expired OAuth credential.
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = CredentialStore::new(temp.path());
    let login = CodexOAuthClient::new(Arc::new(CodexHttp(Mutex::new(VecDeque::from([
        AuthHttpResponse {
            status: 200,
            body: r#"{"access_token":"access-secret","refresh_token":"refresh-secret","expires_in":1}"#.to_string(),
        },
    ])))));
    let session = CodexLoopbackSession::with_redirect_uri(
        generate_pkce_from_entropy(&[7; 43]),
        "state",
        "http://localhost/callback",
        "https://issuer.test",
    );

    // act — login completes and resolution refreshes its expired credential.
    login
        .complete_loopback_callback(&session, "?code=code&state=state", &store)
        .await
        .unwrap_or_abort();
    let refresher: Arc<dyn OAuthTokenRefresher> = Arc::new(
        CodexOAuthClient::new(Arc::new(CodexHttp(Mutex::new(VecDeque::from([
            AuthHttpResponse {
                status: 200,
                body: r#"{"access_token":"access-new","refresh_token":"refresh-new","expires_in":3600}"#.to_string(),
            },
        ])))))
        .with_clock(Arc::new(FixedClock(SystemTime::UNIX_EPOCH))),
    );
    let resolved =
        ProviderCredentialManager::new(store.clone(), ProviderId::codex(), Vec::new(), "", |_| {
            None
        })
        .with_clock(Arc::new(FixedClock(
            SystemTime::UNIX_EPOCH + Duration::from_secs(4_000_000_000),
        )))
        .with_refresher(refresher)
        .resolve()
        .await
        .unwrap_or_abort();

    // assert — refreshed credentials are usable while the persisted manifest is redacted.
    assert_eq!(resolved.token, "access-new");
    let manifest =
        serde_json::to_string(&store.manifest_entries([ProviderId::codex()])).unwrap_or_abort();
    assert!(!manifest.contains("access-secret") && !manifest.contains("refresh-secret"));
    assert_eq!(
        CodexOAuthError::HttpStatus {
            operation: "refresh",
            status: 401
        }
        .category(),
        ProviderErrorCategory::InvalidCredentials
    );
    assert_eq!(
        CodexOAuthError::HttpStatus {
            operation: "refresh",
            status: 429
        }
        .category(),
        ProviderErrorCategory::RateLimited
    );
}

#[tokio::test]
async fn public_copilot_and_api_key_credentials_are_typed_and_http_errors_are_categorized() {
    // arrange — the public Copilot device flow and a stored API key.
    let temp = tempfile::tempdir().unwrap_or_abort();
    let store = CredentialStore::new(temp.path());
    let client = CopilotOAuthClient::new(Arc::new(CopilotHttp(Mutex::new(VecDeque::from([
        AuthHttpResponse { status: 200, body: r#"{"verification_uri":"https://github.com/login/device","user_code":"CODE","device_code":"device","interval":1}"#.to_string() },
        AuthHttpResponse { status: 200, body: r#"{"access_token":"copilot-secret"}"#.to_string() },
    ])))));

    // act — the public device flow completes and API-key resolution runs.
    let copilot = client
        .complete_device_flow(&CopilotDeployment::public(), &store, 1)
        .await
        .unwrap_or_abort();
    let api_store = CredentialStore::new(temp.path().join("api"));
    api_store
        .save(&StoredCredential::api_key(
            ProviderId::codex(),
            "api-secret",
            "2026-01-01T00:00:00Z",
        ))
        .unwrap_or_abort();
    let api =
        ProviderCredentialManager::new(api_store, ProviderId::codex(), Vec::new(), "", |_| None)
            .resolve()
            .await
            .unwrap_or_abort();

    // assert — both public credential sources resolve, and auth failures have actionable categories.
    assert_eq!(copilot.provider, ProviderId::github_copilot());
    assert_eq!(api.source, ResolvedCredentialSource::StoredApiKey);
    assert_eq!(
        CopilotOAuthError::HttpStatus {
            operation: "poll",
            status: 401
        }
        .category(),
        ProviderErrorCategory::InvalidCredentials
    );
    assert_eq!(
        CopilotOAuthError::HttpStatus {
            operation: "poll",
            status: 429
        }
        .category(),
        ProviderErrorCategory::RateLimited
    );
}
