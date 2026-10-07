//! Pooled-account behavior against the fake Claude Code.
use super::*;
use crate::anthropic_subscription::{accounts::AccountPool, store::AccountStoreError};

#[derive(Default)]
struct MemoryStore(Mutex<Option<AccountPool>>);

#[async_trait::async_trait]
impl SubscriptionAccountStore for MemoryStore {
    fn read(&self) -> Result<Option<AccountPool>, AccountStoreError> {
        Ok(self.0.lock().map(|pool| pool.clone()).unwrap_or_default())
    }
    fn modify(
        &self,
        update: &mut (dyn FnMut(Option<AccountPool>) -> Result<Option<AccountPool>, String> + Send),
    ) -> Result<Option<AccountPool>, AccountStoreError> {
        let mut pool = self
            .0
            .lock()
            .map_err(|e| AccountStoreError::Store(e.to_string()))?;
        if let Some(next) = update(pool.clone()).map_err(AccountStoreError::Store)? {
            *pool = Some(next);
        }
        Ok(pool.clone())
    }
    async fn refresh_slot(
        &self,
        _: &str,
        _: &(dyn Fn(i64) -> bool + Send + Sync),
    ) -> Result<Option<AccountPool>, AccountStoreError> {
        Err(AccountStoreError::Refresh("unused".into()))
    }
}

/// A failure Claude Code reports while settling an abort is not a verdict on the account:
/// nothing rotates and no block is saved.
#[tokio::test]
async fn aborted_turns_never_fail_over() -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(MemoryStore::default());
    let fake = Fake::with_env(
        "slow",
        &[
            ("CLAUDE_CODE_OAUTH_TOKEN_2", "second"),
            ("FAKE_CLAUDE_INTERRUPT", "limit"),
        ],
        Some(Arc::clone(&store) as Arc<dyn SubscriptionAccountStore>),
    )?;
    let abort = CancellationToken::new();
    let mut stream = fake
        .provider
        .stream_completion_abortable(
            request("abort-failover-test", vec![user("slow")]),
            abort.clone(),
        )
        .await;
    while let Some(event) = stream.next().await {
        if event == Event::TextDelta("partial".into()) {
            break;
        }
    }
    abort.cancel();
    let rest: Vec<_> = stream.collect().await;
    assert!(
        matches!(rest.last(), Some(Event::Aborted { .. })),
        "{rest:?}"
    );
    assert_eq!(fake.spawns()?, 1);
    let pool = store.read()?.unwrap_or_default();
    assert!(
        pool.slot_state
            .values()
            .all(|state| state.blocked_until.is_none()),
        "{pool:?}"
    );
    Ok(())
}

/// An account whose token is rejected before any output is blocked until it signs in again,
/// and the turn moves to the next pooled account.
#[tokio::test]
async fn rejected_accounts_are_blocked_and_the_turn_moves_on(
) -> Result<(), Box<dyn std::error::Error>> {
    let store = Arc::new(MemoryStore::default());
    let marker = tempfile::tempdir()?;
    let marker = marker.path().join("rejected");
    let fake = Fake::with_env(
        "text",
        &[
            ("CLAUDE_CODE_OAUTH_TOKEN_2", "second"),
            ("FAKE_CLAUDE_REJECT_FIRST", &marker.display().to_string()),
        ],
        Some(Arc::clone(&store) as Arc<dyn SubscriptionAccountStore>),
    )?;
    let events: Vec<_> = fake
        .provider
        .stream_completion(request("auth-failover-test", vec![user("hi")]))
        .await
        .collect()
        .await;
    assert!(
        events.contains(&Event::TextDelta("ok".into())),
        "{events:?}"
    );
    // The switch is announced, and the serving account labels the finished request.
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Notice(text)
            if text.starts_with("Switched Claude account: ") && text.ends_with(" (auth error)"))),
        "{events:?}"
    );
    assert!(
        matches!(events.last(), Some(Event::DoneWithMetadata { metadata: Some(metadata), .. })
            if metadata.session_report.as_ref().is_some_and(|r| r.account.is_some())),
        "{events:?}"
    );
    let tokens: Vec<_> = fake
        .log()?
        .into_iter()
        .filter_map(|line| line.get("token").cloned())
        .collect();
    assert_eq!(tokens.len(), 2);
    assert_ne!(tokens[0], tokens[1]);
    let pool = store.read()?.unwrap_or_default();
    let blocked: Vec<_> = pool
        .slot_state
        .values()
        .filter_map(|state| state.block_reason.as_deref())
        .collect();
    assert_eq!(blocked, ["auth_error"], "{pool:?}");
    Ok(())
}
