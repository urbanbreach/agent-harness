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
        provider: &ProviderId,
        credential: &StoredCredential,
    ) -> Result<OAuthRefreshOutcome, CredentialRefreshError> {
        assert_eq!(provider, &ProviderId::codex());
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

fn provider_id(value: &str) -> ProviderId {
    ProviderId::parse(value).expect("valid provider id")
}

#[test]
fn credential_store_round_trips_replaces_atomically_and_uses_restrictive_permissions() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    let first = StoredCredential::api_key(
        ProviderId::codex(),
        "stored-api-key-old",
        "2026-05-30T00:00:00Z",
    );
    store.save(&first).expect("save first credential");
    let second = StoredCredential::api_key(
        ProviderId::codex(),
        "stored-api-key-new",
        "2026-05-30T00:00:01Z",
    );
    store.save(&second).expect("replace credential atomically");

    let loaded = store
        .load(&ProviderId::codex())
        .expect("load credential")
        .expect("stored credential");
    assert_eq!(loaded.api_key.as_deref(), Some("stored-api-key-new"));
    assert_eq!(loaded.secret_values(), vec!["stored-api-key-new"]);

    #[cfg(unix)]
    assert_eq!(
        credential_file_mode(&store.credential_path(&ProviderId::codex())).expect("mode"),
        0o600
    );
}

#[tokio::test]
async fn credential_resolution_precedence_prefers_stored_then_env_then_inline() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    let manager = ProviderCredentialManager::new(
        store.clone(),
        ProviderId::codex(),
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
            ProviderId::codex(),
            "stored-api-key",
            "2026-05-30T00:00:00Z",
        ))
        .expect("save api credential");
    let resolved = manager.resolve().await.expect("stored api credential");
    assert_eq!(resolved.source, ResolvedCredentialSource::StoredApiKey);
    assert_eq!(resolved.token, "stored-api-key");

    store
        .save(&StoredCredential::oauth(
            ProviderId::codex(),
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
        ProviderId::github_copilot(),
        "stored-copilot-access",
        "stored-copilot-refresh",
        None,
        "2026-05-30T00:00:00Z",
    );
    credential.enterprise_url = Some("ghe.example.com".to_string());
    store.save(&credential).expect("save copilot credential");

    let manager =
        ProviderCredentialManager::new(store, ProviderId::github_copilot(), Vec::new(), "", |_| {
            None
        });

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
            ProviderId::codex(),
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
        ProviderCredentialManager::new(store.clone(), ProviderId::codex(), Vec::new(), "", |_| {
            None
        })
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
        .load(&ProviderId::codex())
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
            ProviderId::github_copilot(),
            "access-secret-value",
            "refresh-secret-value",
            Some("2099-01-01T00:00:00Z".to_string()),
            "2026-05-30T00:00:00Z",
        ))
        .expect("save credential");

    let manifest = serde_json::to_string(&store.manifest_entries([ProviderId::github_copilot()]))
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

#[test]
fn provider_id_parse_rejects_empty() {
    // arrange
    let input_empty = "";
    let input_spaces = "   ";

    // act
    let result_empty = ProviderId::parse(input_empty);
    let result_spaces = ProviderId::parse(input_spaces);

    // assert
    assert!(result_empty.is_none());
    assert!(result_spaces.is_none());
}

#[test]
fn provider_id_parse_rejects_path_traversal() {
    // arrange
    let input1 = "../etc/passwd";
    let input2 = "..\\..\\windows";

    // act
    let result1 = ProviderId::parse(input1);
    let result2 = ProviderId::parse(input2);

    // assert
    assert!(result1.is_none());
    assert!(result2.is_none());
}

#[test]
fn provider_id_parse_rejects_null_bytes() {
    // arrange
    let input = "codex\x00evil";

    // act
    let result = ProviderId::parse(input);

    // assert
    assert!(result.is_none());
}

#[test]
fn provider_id_parse_rejects_newlines() {
    // arrange
    let input1 = "codex\nevil";
    let input2 = "codex\revil";

    // act
    let result1 = ProviderId::parse(input1);
    let result2 = ProviderId::parse(input2);

    // assert
    assert!(result1.is_none());
    assert!(result2.is_none());
}

#[test]
fn provider_id_parse_rejects_terminal_control_characters() {
    // arrange
    let input_esc = "codex\u{1b}]52;c;SGFja2Vk\u{7}";
    let input_del = "codex\u{7f}evil";
    let input_leading_newline = "\ncodex";
    let input_trailing_newline = "codex\n";
    let input_leading_tab = "\tcodex";

    // act
    let result_esc = ProviderId::parse(input_esc);
    let result_del = ProviderId::parse(input_del);
    let result_leading_newline = ProviderId::parse(input_leading_newline);
    let result_trailing_newline = ProviderId::parse(input_trailing_newline);
    let result_leading_tab = ProviderId::parse(input_leading_tab);

    // assert
    assert!(result_esc.is_none());
    assert!(result_del.is_none());
    assert!(result_leading_newline.is_none());
    assert!(result_trailing_newline.is_none());
    assert!(result_leading_tab.is_none());
}

#[test]
fn provider_id_parse_rejects_slashes() {
    // arrange
    let input1 = "foo/bar";
    let input2 = "foo\\bar";

    // act
    let result1 = ProviderId::parse(input1);
    let result2 = ProviderId::parse(input2);

    // assert
    assert!(result1.is_none());
    assert!(result2.is_none());
}

#[test]
fn provider_id_parse_rejects_dotdot() {
    // arrange
    let input1 = "..";
    let input2 = "foo..bar";

    // act
    let result1 = ProviderId::parse(input1);
    let result2 = ProviderId::parse(input2);

    // assert
    assert!(result1.is_none());
    assert!(result2.is_none());
}

#[test]
fn credential_path_stays_in_credentials_dir() {
    // arrange
    let temp = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());

    // act
    let path = store.credential_path(&provider_id("test"));
    let credentials_dir = temp.path().join(CREDENTIALS_DIR_NAME);

    // assert
    assert!(
        path.starts_with(&credentials_dir),
        "credential path {path:?} must be inside {credentials_dir:?}"
    );
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("test.json")
    );
}

#[test]
fn credential_store_load_corrupted_json_fails_gracefully() {
    // arrange
    let temp = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    let path = store.credential_path(&ProviderId::codex());
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(&path, "{{{{not valid json").expect("write corrupted json");

    // act
    let result = store.load(&ProviderId::codex());

    // assert
    assert!(
        result.is_err(),
        "corrupted json must return error not panic"
    );
    match result {
        Err(CredentialStoreError::Parse { .. }) => {}
        other => panic!("expected Parse error, got {other:?}"),
    }
}

#[test]
fn credential_store_load_truncated_json_fails() {
    // arrange
    let temp = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    let path = store.credential_path(&ProviderId::codex());
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(&path, "{\"version\":1,\"provider\":\"cod").expect("write truncated json");

    // act
    let result = store.load(&ProviderId::codex());

    // assert
    assert!(result.is_err(), "truncated json must return error");
}

#[test]
fn credential_store_load_wrong_version_fails() {
    // arrange
    let temp = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    let path = store.credential_path(&ProviderId::codex());
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(
        &path,
        r#"{"version":99,"provider":"codex","kind":"api_key","apiKey":"k","updatedAt":"t"}"#,
    )
    .expect("write wrong version credential");

    // act
    let result = store.load(&ProviderId::codex());

    // assert
    assert!(result.is_err(), "wrong version must return error");
    match result {
        Err(CredentialStoreError::InvalidCredential { .. }) => {}
        other => panic!("expected InvalidCredential error, got {other:?}"),
    }
}

#[test]
fn credential_store_load_wrong_provider_fails() {
    // arrange
    let temp = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    let path = store.credential_path(&ProviderId::codex());
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
    std::fs::write(
        &path,
        r#"{"version":1,"provider":"anthropic","kind":"api_key","apiKey":"k","updatedAt":"t"}"#,
    )
    .expect("write wrong provider credential");

    // act
    let result = store.load(&ProviderId::codex());

    // assert
    assert!(
        result.is_err(),
        "loading codex.json with provider=anthropic must return error"
    );
    match result {
        Err(CredentialStoreError::InvalidCredential { .. }) => {}
        other => panic!("expected InvalidCredential error, got {other:?}"),
    }
}

#[test]
fn credential_store_save_and_load_arbitrary_provider() {
    // arrange
    let temp = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    let provider = provider_id("anthropic");
    let credential = StoredCredential::api_key(
        provider.clone(),
        "sk-anthropic-test",
        "2026-06-26T00:00:00Z",
    );

    // act
    store
        .save(&credential)
        .expect("save arbitrary provider credential");
    let loaded = store
        .load(&provider)
        .expect("load arbitrary provider credential")
        .expect("credential exists");

    // assert
    assert_eq!(loaded.provider, provider);
    assert_eq!(loaded.api_key.as_deref(), Some("sk-anthropic-test"));
}

#[test]
fn credential_store_delete_arbitrary_provider() {
    // arrange
    let temp = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    let provider = provider_id("custom");
    let credential =
        StoredCredential::api_key(provider.clone(), "sk-custom-test", "2026-06-26T00:00:00Z");
    store
        .save(&credential)
        .expect("save custom provider credential");

    // act
    let deleted = store
        .delete(&provider)
        .expect("delete custom provider credential");
    let loaded = store.load(&provider).expect("load after delete");

    // assert
    assert!(deleted, "delete should return true for existing credential");
    assert!(loaded.is_none(), "credential should be gone after delete");
}

#[test]
fn credential_store_file_permissions_are_0600() {
    // arrange
    let temp = tempfile::tempdir().expect("tempdir");
    let store = CredentialStore::new(temp.path());
    let provider = provider_id("perm-test");
    let credential =
        StoredCredential::api_key(provider.clone(), "sk-perm-test", "2026-06-26T00:00:00Z");
    store
        .save(&credential)
        .expect("save credential for permission check");
    let path = store.credential_path(&provider);

    // act
    #[cfg(unix)]
    let mode = credential_file_mode(&path).expect("read file mode");
    #[cfg(not(unix))]
    let _ = path;

    // assert
    #[cfg(unix)]
    assert_eq!(mode, 0o600, "credential file must have 0600 permissions");
    #[cfg(not(unix))]
    {
        // no-op on non-unix
    }
}
