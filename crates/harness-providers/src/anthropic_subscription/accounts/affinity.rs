//! HRW account affinity (senpi `affinity.ts`).
use super::*;

// ---- affinity ----

pub const DEFAULT_AFFINITY_KEY: &str = "claude-sdk-oauth-default";

#[derive(Debug, Clone, Default)]
pub struct AffinityOptions<'a> {
    pub affinity_key: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub pinned_account: Option<&'a str>,
    pub now: i64,
    /// The requested model; an account blocked only for another model stays eligible for it.
    pub model: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct AllAccountsBlockedError {
    pub soonest_unblock_at: Option<i64>,
    /// Set when at least one slot is auth-blocked.
    pub auth_error: bool,
    pub limited_model: Option<String>,
    pub message: String,
}
impl AllAccountsBlockedError {
    pub fn new(soonest: Option<i64>, auth_error: bool, limited_model: Option<String>) -> Self {
        let until = soonest.map(iso);
        let message = match (&limited_model, &until) {
            (Some(model), Some(until)) => format!(
                "All Anthropic Subscription accounts have hit the usage limit for model {model} until {until}."
            ),
            (_, None) => "All Anthropic Subscription accounts are blocked until re-login.".into(),
            (_, Some(until)) => {
                format!("All Anthropic Subscription accounts are blocked until {until}.")
            }
        };
        Self {
            soonest_unblock_at: soonest,
            auth_error,
            limited_model,
            message,
        }
    }
}

pub fn affinity_key<'a>(options: &AffinityOptions<'a>) -> &'a str {
    options
        .affinity_key
        .or(options.session_id)
        .unwrap_or(DEFAULT_AFFINITY_KEY)
}

fn score(key: &str, account_name: &str) -> u64 {
    let digest = Sha256::digest(format!("{key}\0{account_name}").as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(bytes)
}

/// Session-stable HRW ordering preserves Claude prompt-cache locality while moving only the
/// sessions that rendezvous with a newly added or removed account.
pub fn rendezvous_order(key: &str, accounts: &[AccountSlot]) -> Vec<AccountSlot> {
    let mut scored: Vec<_> = accounts
        .iter()
        .map(|account| (score(key, &account.name), account.clone()))
        .collect();
    scored.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    scored.into_iter().map(|(_, account)| account).collect()
}

fn is_account_blocked(account: &AccountSlot, now: i64) -> bool {
    account.block_reason.as_deref() == Some("auth_error")
        || account.blocked_until.is_some_and(|until| until > now)
}

/// Whether the account can serve `model` now.
pub fn is_blocked_for(account: &AccountSlot, now: i64, model: Option<&str>) -> bool {
    is_account_blocked(account, now)
        || active_model_block_until(account.model_blocks.as_ref(), model, now).is_some()
}

/// Removes elapsed rate/capacity blocks but retains auth blocks until login refreshes the slot.
pub fn clear_expired_blocks(accounts: &[AccountSlot], now: i64) -> Vec<AccountSlot> {
    accounts
        .iter()
        .map(|account| {
            let mut available = account.clone();
            if account.block_reason.as_deref() != Some("auth_error")
                && account.blocked_until.is_some_and(|until| until <= now)
            {
                available.blocked_until = None;
                available.block_reason = None;
            }
            if available.model_blocks.is_some() {
                available.model_blocks =
                    prune_model_blocks(available.model_blocks.as_ref(), now, None);
            }
            available
        })
        .collect()
}

fn select_unblocked(
    accounts: &[AccountSlot],
    options: &AffinityOptions<'_>,
    now: i64,
) -> Option<AccountSlot> {
    let pinned = options
        .pinned_account
        .and_then(|pin| accounts.iter().find(|account| account.name == pin));
    if let Some(pinned) = pinned
        && !is_blocked_for(pinned, now, options.model)
    {
        return Some(pinned.clone());
    }
    rendezvous_order(affinity_key(options), accounts)
        .into_iter()
        .find(|account| !is_blocked_for(account, now, options.model))
}

fn unblock_at(account: &AccountSlot, now: i64, model: Option<&str>) -> Option<i64> {
    let account_until = account.blocked_until.filter(|until| *until > now);
    let model_until = active_model_block_until(account.model_blocks.as_ref(), model, now);
    match (account_until, model_until) {
        (None, model) => model,
        (Some(until), None) => Some(until),
        (Some(until), Some(model)) => Some(until.max(model)),
    }
}

/// Selects a pinned or HRW-ranked account with no provider-global selection state.
pub fn select_account(
    accounts: &[AccountSlot],
    options: &AffinityOptions<'_>,
) -> Result<AccountSlot, AllAccountsBlockedError> {
    let now = options.now;
    if let Some(selected) = select_unblocked(accounts, options, now) {
        return Ok(selected);
    }
    // A stale persisted rate-limit entry must not dead-end the pool.
    let cleared = clear_expired_blocks(accounts, now);
    if let Some(selected) = select_unblocked(&cleared, options, now) {
        return Ok(selected);
    }
    let soonest = accounts
        .iter()
        .filter_map(|account| unblock_at(account, now, options.model))
        .min();
    let limited = accounts
        .iter()
        .any(|account| {
            !is_account_blocked(account, now)
                && active_model_block_until(account.model_blocks.as_ref(), options.model, now)
                    .is_some()
        })
        .then(|| options.model.map(str::to_owned))
        .flatten();
    Err(AllAccountsBlockedError::new(
        soonest,
        accounts
            .iter()
            .any(|account| account.block_reason.as_deref() == Some("auth_error")),
        limited,
    ))
}
