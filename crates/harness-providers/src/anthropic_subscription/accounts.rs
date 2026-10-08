//! Account slots, HRW affinity and model-scoped rate-limit blocks.
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::LazyLock};
mod model_scope;
pub use model_scope::*;
mod affinity;
pub use affinity::*;
pub mod display;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccountSource {
    Login,
    Import,
    Env,
}
impl AccountSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Import => "import",
            Self::Env => "env",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModelBlock {
    pub blocked_until: i64,
}
/// A model block's expiry, keyed by model family (`fable`) or, failing that, the exact model id.
pub type ModelBlocks = BTreeMap<String, ModelBlock>;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountSlot {
    /// Immutable operational identity, including SDK session bindings.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub refresh: String,
    pub access: String,
    pub expires: i64,
    pub source: AccountSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_until: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    /// Rate limits that bind one model family on this account, not the account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_blocks: Option<ModelBlocks>,
}
impl std::fmt::Debug for AccountSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountSlot")
            .field("name", &self.name)
            .field("source", &self.source)
            .field("expires", &self.expires)
            .field("blocked_until", &self.blocked_until)
            .field("block_reason", &self.block_reason)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SlotState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_until: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_blocks: Option<ModelBlocks>,
}

/// The pooled part of the provider credential (`accounts`, `pinned`, `slotState`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountPool {
    pub accounts: Vec<AccountSlot>,
    pub pinned: Option<String>,
    pub slot_state: BTreeMap<String, SlotState>,
}

pub const SENTINEL_ACCESS: &str = "claude-sdk-oauth-managed";
pub const SENTINEL_REFRESH: &str = "claude-sdk-oauth-managed";
pub const SENTINEL_EXPIRES: i64 = 4_102_444_800_000;

/// A stored account whose material is the managed sentinel holds no token at all.
pub fn is_sentinel_slot(slot: &AccountSlot) -> bool {
    slot.access == SENTINEL_ACCESS && slot.refresh == SENTINEL_REFRESH
}

pub fn list_accounts(
    pool: &AccountPool,
    env: Option<&dyn Fn(&str) -> Option<String>>,
) -> Vec<AccountSlot> {
    let mut slots: Vec<_> = pool
        .accounts
        .iter()
        .filter(|slot| !is_sentinel_slot(slot))
        .cloned()
        .collect();
    if let Some(env) = env {
        for mut slot in env_slots(env) {
            if let Some(state) = pool.slot_state.get(&slot.name) {
                slot.blocked_until = state.blocked_until;
                slot.block_reason = state.block_reason.clone();
                slot.model_blocks = state.model_blocks.clone();
            }
            slots.push(slot);
        }
    }
    slots
}

static ACCOUNT_NAME: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9][a-zA-Z0-9_-]{0,63}$").ok());
pub fn assert_valid_account_name(name: &str) -> Result<(), String> {
    if ACCOUNT_NAME.as_ref().is_some_and(|re| re.is_match(name)) {
        Ok(())
    } else {
        Err(format!(
            "Invalid account name '{name}': use letters, digits, '-' or '_', starting with a letter or digit"
        ))
    }
}

pub fn add_account(pool: &mut AccountPool, slot: AccountSlot) -> Result<(), String> {
    assert_valid_account_name(&slot.name)?;
    if pool
        .accounts
        .iter()
        .any(|existing| existing.name == slot.name)
    {
        return Err(format!("Account '{}' already exists", slot.name));
    }
    pool.accounts.push(slot);
    Ok(())
}

/// Re-login recovery: a same-name slot is replaced in place (fresh token material and source,
/// block stamps cleared, displayName preserved). Unknown names append.
pub fn upsert_account(pool: &mut AccountPool, slot: AccountSlot) -> Result<(), String> {
    assert_valid_account_name(&slot.name)?;
    match pool
        .accounts
        .iter_mut()
        .find(|candidate| candidate.name == slot.name)
    {
        Some(existing) => {
            existing.access = slot.access;
            existing.refresh = slot.refresh;
            existing.expires = slot.expires;
            existing.source = slot.source;
            existing.blocked_until = None;
            existing.block_reason = None;
            existing.model_blocks = None;
        }
        None => pool.accounts.push(slot),
    }
    Ok(())
}

pub fn remove_account(pool: &mut AccountPool, name: &str) {
    pool.accounts.retain(|slot| slot.name != name);
    if pool.pinned.as_deref() == Some(name) {
        pool.pinned = None;
    }
}

pub fn env_slots(env: &dyn Fn(&str) -> Option<String>) -> Vec<AccountSlot> {
    let mut slots = Vec::new();
    let read = |suffix: Option<usize>| match suffix {
        None => env("CLAUDE_CODE_OAUTH_TOKEN"),
        Some(n) => env(&format!("CLAUDE_CODE_OAUTH_TOKEN_{n}")),
    };
    let names = std::iter::once((None, "env".to_owned()))
        .chain((2..=16).map(|n| (Some(n), format!("env-{n}"))));
    for (suffix, name) in names {
        if read(suffix).is_some_and(|token| !token.is_empty()) {
            slots.push(AccountSlot {
                name,
                display_name: None,
                refresh: String::new(),
                access: String::new(),
                expires: 0,
                source: AccountSource::Env,
                blocked_until: None,
                block_reason: None,
                model_blocks: None,
            });
        }
    }
    slots
}

pub fn env_slot_token(env: &dyn Fn(&str) -> Option<String>, slot_name: &str) -> Option<String> {
    match slot_name {
        "env" => env("CLAUDE_CODE_OAUTH_TOKEN"),
        _ => {
            let suffix = slot_name.strip_prefix("env-")?;
            (!suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()))
                .then(|| env(&format!("CLAUDE_CODE_OAUTH_TOKEN_{suffix}")))
                .flatten()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(name: &str) -> AccountSlot {
        AccountSlot {
            name: name.into(),
            display_name: None,
            refresh: format!("r-{name}"),
            access: format!("a-{name}"),
            expires: i64::MAX,
            source: AccountSource::Login,
            blocked_until: None,
            block_reason: None,
            model_blocks: None,
        }
    }

    #[test]
    fn selection_honors_pins_hrw_auth_and_model_blocks() -> Result<(), AllAccountsBlockedError> {
        let accounts = vec![slot("a"), slot("b"), slot("c")];
        let options = AffinityOptions {
            session_id: Some("session-1"),
            now: 1_000,
            model: Some("claude-fable-5-1"),
            ..AffinityOptions::default()
        };
        let first = select_account(&accounts, &options)?;
        assert_eq!(first.name, rendezvous_order("session-1", &accounts)[0].name);
        let pinned = AffinityOptions {
            pinned_account: Some("c"),
            ..options.clone()
        };
        assert_eq!(select_account(&accounts, &pinned)?.name, "c");
        let mut blocked = accounts.clone();
        for account in &mut blocked {
            account.model_blocks = Some(with_model_block(None, "fable", 5_000, 1_000));
        }
        let error = select_account(&blocked, &options)
            .err()
            .ok_or_else(|| AllAccountsBlockedError::new(None, false, None))?;
        assert_eq!(error.limited_model.as_deref(), Some("claude-fable-5-1"));
        assert_eq!(error.soonest_unblock_at, Some(5_000));
        let opus = AffinityOptions {
            model: Some("claude-opus-5-5"),
            ..options.clone()
        };
        assert!(select_account(&blocked, &opus).is_ok());
        for account in &mut blocked {
            account.block_reason = Some("auth_error".into());
        }
        assert!(select_account(&blocked, &opus).is_err_and(|e| e.auth_error));
        Ok(())
    }

    #[test]
    fn model_family_limits_scope_to_the_named_family() {
        assert_eq!(
            rate_limit_model_family("You've hit your Fable limit · resets 8pm").as_deref(),
            Some("fable")
        );
        assert_eq!(
            rate_limit_model_family("You've hit your weekly limit"),
            None
        );
        assert_eq!(model_block_key("opus", "claude-opus-5-5"), "opus");
        assert_eq!(
            model_block_key("opus", "claude-sonnet-5"),
            "claude-sonnet-5"
        );
    }
}
