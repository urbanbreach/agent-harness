use super::*;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, SystemTime},
};
use tokio::sync::Notify;

#[test]
fn credential_store_is_private_and_rejects_identity_mismatches_and_symlinks(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let store = CredentialStore::new(temp.path().join("data"));
    let provider = ProviderId::codex();
    assert!(store.load(&provider)?.is_none());
    assert!(store.stored_provider_ids()?.is_empty());
    assert!(!store.delete(&provider)?);
    assert_eq!(std::fs::read_dir(temp.path())?.count(), 0);
    let credential = StoredCredential::api_key(
        provider.clone(),
        "opaque-private-key",
        "2026-09-26T00:00:00Z",
    );
    store.save(&credential)?;
    assert_eq!(store.stored_provider_ids()?, vec![provider.clone()]);
    assert_eq!(store.load(&provider)?, Some(credential.clone()));
    assert!(!format!("{credential:?}").contains("opaque-private-key"));
    #[cfg(unix)]
    assert_eq!(
        credential_file_mode(&store.credential_path(&provider))? & 0o777,
        0o600
    );
    let path = store.credential_path(&provider);
    let original = std::fs::read(&path)?;
    let mut invalid = credential.clone();
    invalid.version = 2;
    assert!(store.save(&invalid).is_err());
    assert_eq!(std::fs::read(&path)?, original);
    invalid.version = 1;
    invalid.provider = ProviderId::github_copilot();
    std::fs::write(&path, serde_json::to_vec(&invalid)?)?;
    assert!(store.load(&provider).is_err());
    assert_ne!(
        store.manifest_entries([provider.clone()])[0].status,
        "not_stored"
    );
    #[cfg(unix)]
    {
        let external = temp.path().join("external");
        std::fs::write(&external, &original)?;
        std::fs::remove_file(&path)?;
        std::os::unix::fs::symlink(&external, &path)?;
        assert!(store.load(&provider).is_err());
        assert!(store.save(&credential).is_err());
        assert!(store.delete(&provider).is_err());
        assert!(store.stored_provider_ids().is_err());
        assert_eq!(std::fs::read(&external)?, original);
    }
    Ok(())
}

struct FixedClock;
impl CredentialClock for FixedClock {
    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000_000)
    }
}
#[derive(Default)]
struct Refresh {
    calls: AtomicUsize,
    entered: Notify,
    release: Notify,
}
#[async_trait::async_trait]
impl OAuthTokenRefresher for Refresh {
    async fn refresh(
        &self,
        _: &ProviderId,
        _: &StoredCredential,
    ) -> Result<OAuthRefreshOutcome, CredentialRefreshError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        self.release.notified().await;
        Ok(OAuthRefreshOutcome {
            access_token: "refreshed-private-token".into(),
            refresh_token: Some("rotated-private-token".into()),
            expires_at: Some("2100-01-01T00:00:00Z".into()),
            account_id: None,
            scopes: Vec::new(),
        })
    }
}
#[tokio::test]
async fn credential_refresh_coalesces_waiters_and_cannot_resurrect_logout(
) -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let store = CredentialStore::new(temp.path());
    let provider = ProviderId::codex();
    let refresh = Arc::new(Refresh::default());
    let secrets = Arc::new(crate::redact::SecretRegistry::default());
    let redactor = crate::redact::SecretRedactor::new(
        Arc::new(crate::redact::DefaultRedactor::default()),
        Arc::clone(&secrets),
    );
    let manager = Arc::new(
        ProviderCredentialManager::new(
            store.clone(),
            provider.clone(),
            vec!["TEST_KEY".into()],
            "inline-private-key",
            |_| Some("environment-private-key".into()),
        )
        .with_clock(Arc::new(FixedClock))
        .with_refresher(Arc::clone(&refresh) as Arc<dyn OAuthTokenRefresher>)
        .with_secret_registry(Arc::clone(&secrets))?,
    );
    let environment = manager.resolve().await?;
    assert_eq!(
        environment.source,
        ResolvedCredentialSource::EnvApiKey {
            env: "TEST_KEY".into()
        }
    );
    store.save(&StoredCredential::api_key(
        provider.clone(),
        "stored-private-key",
        "2026-09-26T00:00:00Z",
    ))?;
    assert_eq!(
        manager.resolve().await?.source,
        ResolvedCredentialSource::StoredApiKey
    );
    let mut expired = StoredCredential::oauth(
        provider.clone(),
        "expired-private-token",
        "old-private-refresh",
        Some("2000-01-01T00:00:00Z".into()),
        "2026-09-26T00:00:00Z",
    );
    expired.metadata.insert("retained".into(), "value".into());
    store.save(&expired)?;
    let tasks: Vec<_> = (0..2)
        .map(|_| {
            let manager = Arc::clone(&manager);
            tokio::spawn(async move { manager.resolve().await })
        })
        .collect();
    tokio::time::timeout(Duration::from_secs(3), refresh.entered.notified()).await?;
    refresh.release.notify_one();
    for task in tasks {
        let result = tokio::time::timeout(Duration::from_secs(3), task).await???;
        assert_eq!(result.token, "refreshed-private-token");
        assert!(!format!("{result:?}").contains("private-token"));
    }
    assert_eq!(refresh.calls.load(Ordering::SeqCst), 1);
    let saved = store
        .load(&provider)?
        .ok_or("missing refreshed credential")?;
    assert_eq!(
        saved.refresh_token.as_deref(),
        Some("rotated-private-token")
    );
    assert_eq!(saved.metadata, expired.metadata);
    use crate::redact::Redactor;
    for token in [
        "environment-private-key",
        "stored-private-key",
        "expired-private-token",
        "old-private-refresh",
        "refreshed-private-token",
        "rotated-private-token",
    ] {
        assert_eq!(redactor.redact_text(token), "[REDACTED]");
    }
    assert!(secrets.register(["x".repeat(4 * 1024 * 1024 + 1)]).is_err());
    assert_eq!(
        redactor.redact_text("refreshed-private-token"),
        "[REDACTED]"
    );
    store.save(&expired)?;
    let pending = {
        let manager = Arc::clone(&manager);
        tokio::spawn(async move { manager.resolve().await })
    };
    tokio::time::timeout(Duration::from_secs(3), refresh.entered.notified()).await?;
    store.delete(&provider)?;
    refresh.release.notify_one();
    assert!(tokio::time::timeout(Duration::from_secs(3), pending)
        .await??
        .is_err());
    assert!(store.load(&provider)?.is_none());
    cancel_wake_refresh(&store, manager, refresh, &expired).await?;
    Ok(())
}

async fn cancel_wake_refresh(
    store: &CredentialStore,
    manager: Arc<ProviderCredentialManager>,
    refresh: Arc<Refresh>,
    expired: &StoredCredential,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::sleep_wake_auth::*;
    store.save(expired)?;
    let decision = decide_sleep_wake_credential_refresh_for(
        SleepWakeHostEvent::Wake,
        Some(&CredentialExpirySnapshot::with_default_leeway(Some(0), 1)),
    );
    let cancel = tokio_util::sync::CancellationToken::new();
    let token = cancel.clone();
    let pending = tokio::spawn(async move {
        execute_sleep_wake_refresh_decision(&decision, &manager, &token).await
    });
    tokio::time::timeout(Duration::from_secs(3), refresh.entered.notified()).await?;
    cancel.cancel();
    assert!(tokio::time::timeout(Duration::from_secs(3), pending)
        .await??
        .is_cancelled());
    assert_eq!(store.load(&expired.provider)?.as_ref(), Some(expired));
    Ok(())
}
