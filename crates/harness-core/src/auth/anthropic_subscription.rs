//! The `anthropic-subscription` credential: a managed sentinel whose real token material lives
//! in pooled account slots (senpi `accounts.ts`, `account-management.ts`, `oauth-login.ts`).
use super::anthropic::{AnthropicOAuthClient, AnthropicOAuthCredential};
use super::{
    CredentialStore, CredentialStoreError, ProviderId, StoredCredential, StoredCredentialKind,
};
use harness_providers::anthropic_subscription::{
    accounts::{
        describe_model_blocks, is_sentinel_slot, iso, list_accounts, remove_account,
        upsert_account, SENTINEL_ACCESS, SENTINEL_REFRESH,
    },
    AccountPool, AccountSlot, AccountSource, AccountStoreError, SubscriptionAccountStore,
};
use regex::Regex;
use std::sync::{Arc, LazyLock};

/// `4102444800000` (2100-01-01), the sentinel expiry senpi stores.
pub const SENTINEL_EXPIRES_AT: &str = "2100-01-01T00:00:00Z";

pub fn pool_from_credential(credential: &StoredCredential) -> AccountPool {
    AccountPool {
        accounts: credential.accounts.clone(),
        pinned: credential.pinned.clone(),
        slot_state: credential.slot_state.clone(),
    }
}

pub fn credential_from_pool(pool: AccountPool, updated_at: String) -> StoredCredential {
    let mut credential = StoredCredential::oauth(
        ProviderId::anthropic_subscription(),
        SENTINEL_ACCESS,
        SENTINEL_REFRESH,
        Some(SENTINEL_EXPIRES_AT.into()),
        updated_at,
    );
    credential.accounts = pool.accounts;
    credential.pinned = pool.pinned;
    credential.slot_state = pool.slot_state;
    credential
}

fn now_rfc3339() -> String {
    humantime::format_rfc3339(std::time::SystemTime::now()).to_string()
}

fn store_error(error: CredentialStoreError) -> AccountStoreError {
    match error {
        busy @ CredentialStoreError::Busy { .. } => AccountStoreError::Busy(busy.to_string()),
        other => AccountStoreError::Store(other.to_string()),
    }
}

/// [`SubscriptionAccountStore`] over the harness credential store.
#[derive(Clone)]
pub struct AnthropicSubscriptionAccounts {
    store: CredentialStore,
    oauth: Arc<AnthropicOAuthClient>,
}

impl AnthropicSubscriptionAccounts {
    pub fn new(store: CredentialStore, oauth: Arc<AnthropicOAuthClient>) -> Self {
        Self { store, oauth }
    }
    pub fn store(&self) -> &CredentialStore {
        &self.store
    }
}

fn oauth_pool(credential: Option<StoredCredential>) -> Option<AccountPool> {
    credential
        .filter(|c| c.kind == StoredCredentialKind::Oauth)
        .map(|c| pool_from_credential(&c))
}

#[async_trait::async_trait]
impl SubscriptionAccountStore for AnthropicSubscriptionAccounts {
    fn read(&self) -> Result<Option<AccountPool>, AccountStoreError> {
        self.store
            .load(&ProviderId::anthropic_subscription())
            .map(oauth_pool)
            .map_err(store_error)
    }

    fn modify(
        &self,
        update: &mut (dyn FnMut(Option<AccountPool>) -> Result<Option<AccountPool>, String> + Send),
    ) -> Result<Option<AccountPool>, AccountStoreError> {
        self.store
            .modify(&ProviderId::anthropic_subscription(), |current| {
                Ok(update(oauth_pool(current))?
                    .map(|pool| credential_from_pool(pool, now_rfc3339())))
            })
            .map(oauth_pool)
            .map_err(store_error)
    }

    async fn refresh_slot(
        &self,
        slot: &str,
        is_expiring: &(dyn Fn(i64) -> bool + Send + Sync),
    ) -> Result<Option<AccountPool>, AccountStoreError> {
        let store = self.store.clone();
        let lock = tokio::task::spawn_blocking(move || {
            store.lock_bounded(super::CREDENTIAL_LOCK_RETRY_BUDGET)
        })
        .await
        .map_err(|e| AccountStoreError::Store(e.to_string()))?
        .map_err(store_error)?;
        let id = ProviderId::anthropic_subscription();
        let current = oauth_pool(self.store.load(&id).map_err(store_error)?);
        let Some(mut pool) = current else {
            return Ok(None);
        };
        let Some(index) = pool
            .accounts
            .iter()
            .position(|candidate| candidate.name == slot)
        else {
            return Ok(Some(pool));
        };
        if !is_expiring(pool.accounts[index].expires) {
            return Ok(Some(pool));
        }
        let refreshed = self
            .oauth
            .refresh(&pool.accounts[index].refresh)
            .await
            .map_err(|e| AccountStoreError::Refresh(e.to_string()))?;
        let account = &mut pool.accounts[index];
        account.access = refreshed.access;
        account.refresh = refreshed.refresh;
        account.expires = refreshed.expires;
        let written = self
            .store
            .modify_locked(&id, |_| Ok(Some(credential_from_pool(pool, now_rfc3339()))))
            .map_err(store_error)?;
        drop(lock);
        Ok(oauth_pool(written))
    }
}

pub fn slot_from_login(
    credential: AnthropicOAuthCredential,
    name: &str,
    source: AccountSource,
) -> AccountSlot {
    AccountSlot {
        name: name.into(),
        display_name: None,
        refresh: credential.refresh,
        access: credential.access,
        expires: credential.expires,
        source,
        blocked_until: None,
        block_reason: None,
        model_blocks: None,
    }
}

/// A lone slot, or the pool's one auth-blocked slot, is refreshed in place by a re-login.
pub fn recovery_target_name(existing: &[AccountSlot]) -> Option<String> {
    if existing.len() == 1 {
        return Some(existing[0].name.clone());
    }
    let blocked: Vec<_> = existing
        .iter()
        .filter(|slot| slot.block_reason.as_deref() == Some("auth_error"))
        .collect();
    (blocked.len() == 1).then(|| blocked[0].name.clone())
}

/// The account-name prompt and its default (`promptAccountName`).
pub fn account_name_prompt(existing: &[AccountSlot]) -> Option<(String, String)> {
    if existing.is_empty() {
        return None;
    }
    let recovery = recovery_target_name(existing);
    let fallback = format!("account-{}", existing.len() + 1);
    let names = existing
        .iter()
        .map(|slot| slot.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let message = match &recovery {
        None => format!(
            "Name for this account (existing: {names}; press Enter to add {fallback}, or type an existing name to refresh it)"
        ),
        Some(recovery) => format!("Name for this account (existing: {names}; press Enter to refresh '{recovery}' with this login)"),
    };
    Some((message, recovery.unwrap_or(fallback)))
}

/// Commits a login into the pool under `name` (replacing a same-name slot in place).
pub fn commit_login(
    store: &CredentialStore,
    credential: AnthropicOAuthCredential,
    name: &str,
) -> Result<(), String> {
    store
        .modify(&ProviderId::anthropic_subscription(), |current| {
            let mut pool = oauth_pool(current).unwrap_or_default();
            upsert_account(
                &mut pool,
                slot_from_login(credential, name, AccountSource::Login),
            )?;
            Ok(Some(credential_from_pool(pool, now_rfc3339())))
        })
        .map(|_| ())
        .map_err(|e| e.to_string())
}

pub fn read_pool(store: &CredentialStore) -> Result<AccountPool, String> {
    Ok(oauth_pool(
        store
            .load(&ProviderId::anthropic_subscription())
            .map_err(|e| e.to_string())?,
    )
    .unwrap_or_default())
}

pub fn existing_accounts(store: &CredentialStore) -> Result<Vec<AccountSlot>, String> {
    Ok(list_accounts(&read_pool(store)?, None))
}

fn update_pool(
    store: &CredentialStore,
    update: impl FnOnce(&mut AccountPool) -> Result<(), String>,
) -> Result<(), String> {
    let mut failure = None;
    store
        .modify(&ProviderId::anthropic_subscription(), |current| {
            let mut pool = oauth_pool(current).unwrap_or_default();
            if let Err(error) = update(&mut pool) {
                failure = Some(error);
                return Ok(None);
            }
            Ok(Some(credential_from_pool(pool, now_rfc3339())))
        })
        .map_err(|e| e.to_string())?;
    failure.map_or(Ok(()), Err)
}

pub fn pin_account(
    store: &CredentialStore,
    name: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<(), String> {
    update_pool(store, |pool| {
        match name {
            None => pool.pinned = None,
            Some(name) => {
                if !list_accounts(pool, Some(env))
                    .iter()
                    .any(|a| a.name == name)
                {
                    return Err(format!("Provider account not found: {name}"));
                }
                pool.pinned = Some(name.into());
            }
        }
        Ok(())
    })
}

pub fn remove_named_account(
    store: &CredentialStore,
    name: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<(), String> {
    update_pool(store, |pool| {
        if !pool.accounts.iter().any(|a| a.name == name) {
            return Err(
                if list_accounts(pool, Some(env))
                    .iter()
                    .any(|a| a.name == name && a.source == AccountSource::Env)
                {
                    format!("Environment provider account cannot be removed: {name}")
                } else {
                    format!("Provider account not found: {name}")
                },
            );
        }
        remove_account(pool, name);
        Ok(())
    })
}

mod display;
pub use display::*;
pub use harness_providers::anthropic_subscription::accounts::display::{
    account_display_name, account_label, display_name_columns, DISPLAY_NAME_MAX_COLUMNS,
};

/// One `/claude-account list` row: label, source and block state.
pub fn slot_status(slot: &AccountSlot, now: i64) -> String {
    let mut states = Vec::new();
    if slot.block_reason.as_deref() == Some("auth_error") {
        states.push("blocked until re-login".to_owned());
    } else if let Some(until) = slot.blocked_until.filter(|until| *until > now) {
        states.push(format!("blocked until {}", iso(until)));
    }
    states.extend(describe_model_blocks(slot.model_blocks.as_ref(), now));
    if states.is_empty() {
        "available".into()
    } else {
        states.join(", ")
    }
}

pub fn is_usable_pool(pool: &AccountPool) -> bool {
    pool.accounts.iter().any(|slot| !is_sentinel_slot(slot))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pooled_logins_upsert_pin_rename_and_remove() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let store = CredentialStore::new(root.path());
        let login = |access: &str| AnthropicOAuthCredential {
            access: access.into(),
            refresh: format!("r-{access}"),
            expires: 1,
        };
        commit_login(&store, login("one"), "default")?;
        commit_login(&store, login("two"), "work")?;
        commit_login(&store, login("three"), "default")?;
        let stored = store
            .load(&ProviderId::anthropic_subscription())?
            .ok_or("missing")?;
        assert_eq!(stored.access_token.as_deref(), Some(SENTINEL_ACCESS));
        assert!(stored.secret_values().contains(&"three".to_owned()));
        let accounts = existing_accounts(&store)?;
        assert_eq!(
            accounts
                .iter()
                .map(|a| a.access.as_str())
                .collect::<Vec<_>>(),
            ["three", "two"]
        );
        let env = |_: &str| None;
        pin_account(&store, Some("work"), &env)?;
        assert!(pin_account(&store, Some("missing"), &env).is_err());
        rename_account(&store, "work", Some("  Work   Laptop "))?;
        rename_account(&store, "default", Some("Home"))?;
        assert!(rename_account(&store, "default", Some("work laptop")).is_err());
        // Fullwidth and decomposed spellings fold to the same key; the stored form is NFC.
        assert!(rename_account(&store, "default", Some("ｗｏｒｋ ｌａｐｔｏｐ")).is_err());
        rename_account(&store, "default", Some("Cafe\u{301}"))?;
        assert_eq!(
            read_pool(&store)?.accounts[0].display_name.as_deref(),
            Some("Caf\u{e9}")
        );
        assert_eq!(
            display_name_columns("\u{1f1eb}\u{1f1ee}\u{2764}\u{fe0f}"),
            3
        );
        assert!(rename_account(&store, "default", Some("bad\u{7}")).is_err());
        let pool = read_pool(&store)?;
        assert_eq!(pool.pinned.as_deref(), Some("work"));
        assert_eq!(account_label(&pool.accounts[1]), "Work Laptop (work)");
        remove_named_account(&store, "work", &env)?;
        assert_eq!(read_pool(&store)?.pinned, None);
        assert_eq!(
            account_name_prompt(&existing_accounts(&store)?)
                .map(|(_, default)| default)
                .as_deref(),
            Some("default")
        );
        Ok(())
    }
}
