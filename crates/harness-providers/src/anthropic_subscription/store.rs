//! The persisted account pool, implemented over the harness credential store.
use super::accounts::AccountPool;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AccountStoreError {
    /// Another writer held the store past the lock budget (senpi `CredentialStoreBusyError`).
    #[error("{0}")]
    Busy(String),
    /// The token endpoint refused or failed the refresh; the text is classified.
    #[error("{0}")]
    Refresh(String),
    #[error("{0}")]
    Store(String),
}

/// `CredentialStore` for the `anthropic-subscription` entry.
#[async_trait::async_trait]
pub trait SubscriptionAccountStore: Send + Sync {
    /// The stored pool; `None` when no credential exists.
    fn read(&self) -> Result<Option<AccountPool>, AccountStoreError>;
    /// Locked read-modify-write; `Ok(None)` from `update` leaves the store unchanged.
    fn modify(
        &self,
        update: &mut (dyn FnMut(Option<AccountPool>) -> Result<Option<AccountPool>, String> + Send),
    ) -> Result<Option<AccountPool>, AccountStoreError>;
    /// Refreshes `slot` under the store lock while `is_expiring(expires)` still holds there,
    /// so one single-use refresh token is never redeemed twice.
    async fn refresh_slot(
        &self,
        slot: &str,
        is_expiring: &(dyn Fn(i64) -> bool + Send + Sync),
    ) -> Result<Option<AccountPool>, AccountStoreError>;
}
