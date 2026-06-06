use super::*;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex as StdMutex;
use tokio::sync::oneshot;

#[derive(Debug)]
struct FixedClock(SystemTime);

impl CredentialClock for FixedClock {
    fn now(&self) -> SystemTime {
        self.0
    }
}

struct CountingRefresher {
    calls: AtomicUsize,
    expires_at: String,
    started: StdMutex<Option<oneshot::Sender<()>>>,
    release: StdMutex<Option<oneshot::Receiver<()>>>,
}

impl fmt::Debug for CountingRefresher {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CountingRefresher")
            .field("calls", &self.calls.load(Ordering::SeqCst))
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl OAuthTokenRefresher for CountingRefresher {
    async fn refresh(
        &self,
        provider: AuthProviderId,
        credential: &StoredCredential,
    ) -> Result<OAuthRefreshOutcome, CredentialRefreshError> {
        assert_eq!(provider, AuthProviderId::Codex);
        assert_eq!(credential.refresh_token.as_deref(), Some("refresh-old"));
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(started) = self.started.lock().expect("started lock").take() {
            let _ = started.send(());
        }
        let release = self.release.lock().expect("release lock").take();
        if let Some(release) = release {
            let _ = release.await;
        }
        Ok(OAuthRefreshOutcome {
            access_token: "access-new".to_string(),
            refresh_token: Some("refresh-new".to_string()),
            expires_at: Some(self.expires_at.clone()),
            account_id: Some("acct-new".to_string()),
            scopes: vec!["openid".to_string()],
        })
    }
}

#[test]
fn credential_store_round_trips_replaces_atomically_and_uses_restrictive_permissions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    let first = StoredCredential::api_key(
        AuthProviderId::Codex,
        "stored-api-key-old",
        "2026-05-30T00:00:00Z",
    );
    store.save(&first).expect("save first credential");
    let second = StoredCredential::api_key(
        AuthProviderId::Codex,
        "stored-api-key-new",
        "2026-05-30T00:00:01Z",
    );
    store.save(&second).expect("replace credential atomically");

    let loaded = store
        .load(AuthProviderId::Codex)
        .expect("load credential")
        .expect("stored credential");
    assert_eq!(loaded.api_key.as_deref(), Some("stored-api-key-new"));
    assert_eq!(loaded.secret_values(), vec!["stored-api-key-new"]);

    #[cfg(unix)]
    assert_eq!(
        credential_file_mode(&store.credential_path(AuthProviderId::Codex)).expect("mode"),
        0o600
    );
}

#[tokio::test]
async fn credential_resolution_precedence_prefers_stored_then_env_then_inline() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    let manager = ProviderCredentialManager::new(
        store.clone(),
        AuthProviderId::Codex,
        vec!["HARNESS_TEST_API_KEY".to_string()],
        "inline-key",
        |name| (name == "HARNESS_TEST_API_KEY").then(|| "env-key".to_string()),
    );

    let resolved = manager.resolve().await.expect("env credential");
    assert_eq!(
        resolved.source,
        ResolvedCredentialSource::EnvApiKey {
            env: "HARNESS_TEST_API_KEY".to_string()
        }
    );
    assert_eq!(resolved.token, "env-key");

    store
        .save(&StoredCredential::api_key(
            AuthProviderId::Codex,
            "stored-api-key",
            "2026-05-30T00:00:00Z",
        ))
        .expect("save api credential");
    let resolved = manager.resolve().await.expect("stored api credential");
    assert_eq!(resolved.source, ResolvedCredentialSource::StoredApiKey);
    assert_eq!(resolved.token, "stored-api-key");

    store
        .save(&StoredCredential::oauth(
            AuthProviderId::Codex,
            "stored-oauth-access",
            "stored-oauth-refresh",
            Some("2099-01-01T00:00:00Z".to_string()),
            "2026-05-30T00:00:00Z",
        ))
        .expect("replace with oauth credential");
    let resolved = manager.resolve().await.expect("stored oauth credential");
    assert_eq!(resolved.source, ResolvedCredentialSource::StoredOauth);
    assert_eq!(resolved.token, "stored-oauth-access");
}

#[tokio::test]
async fn credential_resolution_preserves_copilot_enterprise_url() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    let mut credential = StoredCredential::oauth(
        AuthProviderId::GithubCopilot,
        "stored-copilot-access",
        "stored-copilot-refresh",
        None,
        "2026-05-30T00:00:00Z",
    );
    credential.enterprise_url = Some("ghe.example.com".to_string());
    store.save(&credential).expect("save copilot credential");

    let manager = ProviderCredentialManager::new(
        store,
        AuthProviderId::GithubCopilot,
        Vec::new(),
        "",
        |_| None,
    );

    let bearer = manager.bearer_token().await.expect("bearer token");
    assert_eq!(bearer.token, "stored-copilot-access");
    assert_eq!(bearer.enterprise_url.as_deref(), Some("ghe.example.com"));
}

#[tokio::test]
async fn expired_oauth_refresh_is_single_flight_and_persisted() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    store
        .save(&StoredCredential::oauth(
            AuthProviderId::Codex,
            "access-old",
            "refresh-old",
            Some("2026-05-29T00:00:00Z".to_string()),
            "2026-05-29T00:00:00Z",
        ))
        .expect("save expired oauth credential");
    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let refresher = Arc::new(CountingRefresher {
        calls: AtomicUsize::new(0),
        expires_at: "2026-05-31T00:00:00Z".to_string(),
        started: StdMutex::new(Some(started_tx)),
        release: StdMutex::new(Some(release_rx)),
    });
    let manager = Arc::new(
        ProviderCredentialManager::new(
            store.clone(),
            AuthProviderId::Codex,
            Vec::new(),
            "",
            |_| None,
        )
        .with_clock(Arc::new(FixedClock(
            humantime::parse_rfc3339("2026-05-30T00:00:00Z").expect("clock"),
        )))
        .with_refresher(refresher.clone()),
    );

    let first = tokio::spawn({
        let manager = manager.clone();
        async move { manager.resolve().await.expect("first resolve") }
    });
    started_rx.await.expect("refresh started");
    let second = tokio::spawn({
        let manager = manager.clone();
        async move { manager.resolve().await.expect("second resolve") }
    });
    tokio::task::yield_now().await;
    release_tx.send(()).expect("release refresher");
    let first = first.await.expect("first join");
    let second = second.await.expect("second join");

    assert_eq!(first.token, "access-new");
    assert_eq!(second.token, "access-new");
    assert_eq!(refresher.calls.load(Ordering::SeqCst), 1);
    let stored = store
        .load(AuthProviderId::Codex)
        .expect("load refreshed")
        .expect("refreshed credential");
    assert_eq!(stored.access_token.as_deref(), Some("access-new"));
    assert_eq!(stored.refresh_token.as_deref(), Some("refresh-new"));
    assert_eq!(stored.account_id.as_deref(), Some("acct-new"));
}

#[test]
fn credential_store_manifest_excludes_secret_material() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    store
        .save(&StoredCredential::oauth(
            AuthProviderId::GithubCopilot,
            "access-secret-value",
            "refresh-secret-value",
            Some("2099-01-01T00:00:00Z".to_string()),
            "2026-05-30T00:00:00Z",
        ))
        .expect("save credential");

    let manifest = serde_json::to_string(&store.manifest_entries([AuthProviderId::GithubCopilot]))
        .expect("serialize manifest");
    assert!(manifest.contains("github-copilot"));
    assert!(!manifest.contains("access-secret-value"));
    assert!(!manifest.contains("refresh-secret-value"));
}

#[test]
fn windows_whoami_csv_sid_parser_accepts_quoted_user_rows() {
    assert_eq!(
        parse_whoami_user_sid("\"EXAMPLE\\\\user\",\"S-1-5-21-111-222-333-1001\"\r\n"),
        Some("S-1-5-21-111-222-333-1001".to_string())
    );
    assert_eq!(parse_whoami_user_sid("\"EXAMPLE\\\\user\",\"\""), None);
}
