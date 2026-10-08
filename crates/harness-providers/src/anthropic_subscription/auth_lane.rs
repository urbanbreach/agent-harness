//! Auth lanes and account failover.
use super::accounts::display::account_label;
use super::accounts::{
    active_model_block_until, assert_valid_account_name, clear_expired_blocks, env_slot_token,
    list_accounts, merge_model_blocks, model_block_key, select_account, with_model_block,
    AccountPool, AccountSlot, AccountSource, AffinityOptions, SlotState,
};
use super::errors::{
    classify_lane_error, classify_sdk_error, sdk_failure, Classification, LaneError, SdkErrorKind,
};
use super::limits::usage_limit_reset_ms;
use super::options::{AnthropicSubscriptionSettings, TokenInjection};
use super::prompt::hex;
use super::protocol::{ClaudeQuery, Prompt, QueryOptions};
use super::store::{AccountStoreError, SubscriptionAccountStore};
use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, LazyLock},
};
use tokio_stream::{Stream, StreamExt};
use tokio_util::sync::CancellationToken;
mod failover;
pub use failover::*;
mod config_dir;
pub use config_dir::*;

pub const EXPIRING_WITHIN_MS: i64 = 5 * 60_000;
pub const MAX_RATE_LIMIT_BLOCK_MS: i64 = 48 * 60 * 60 * 1_000;
pub const DEFAULT_RATE_LIMIT_BLOCK_MS: i64 = 60_000;
const NO_MANAGED_ACCOUNTS_ERROR: &str = "authentication_failed: No Anthropic Subscription accounts configured for the managed lane; run /login anthropic-subscription or set CLAUDE_CODE_OAUTH_TOKEN";
pub const PROVIDER_ID: &str = "anthropic-subscription";

pub type MessageStream = Pin<Box<dyn Stream<Item = Result<Value, LaneError>> + Send>>;
pub type BoxFuture<T> = Pin<Box<dyn std::future::Future<Output = T> + Send>>;

/// One authenticated attempt's messages. An attempt that failed is discarded explicitly;
/// one dropped mid-turn (the caller abandoned the request) aborts instead.
pub struct Attempt {
    pub messages: MessageStream,
    pub discard: Option<Box<dyn FnOnce() + Send>>,
}

pub struct AuthenticatedAttempt {
    pub account_name: String,
    pub accounts: Vec<AccountSlot>,
    pub auth_lane: TokenInjection,
    pub options: QueryOptions,
    /// Digest of the OAuth access token this attempt authenticates with.
    pub credential_digest: Option<String>,
}

pub type AttemptFactory =
    Arc<dyn Fn(AuthenticatedAttempt) -> BoxFuture<Result<Attempt, LaneError>> + Send + Sync>;
pub type OptionsBuilder =
    Arc<dyn Fn(TokenInjection) -> Result<QueryOptions, LaneError> + Send + Sync>;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

#[derive(Clone)]
pub struct AuthLaneInput {
    pub store: Option<Arc<dyn SubscriptionAccountStore>>,
    pub settings: AnthropicSubscriptionSettings,
    /// The host environment the subprocess inherits.
    pub environment: BTreeMap<String, String>,
    pub agent_dir: Option<PathBuf>,
    pub session_id: Option<String>,
    pub model: String,
    pub pinned_account: Option<String>,
    pub build_options: OptionsBuilder,
    pub create_attempt: AttemptFactory,
    /// The turn's abort signal; an aborted attempt is never a verdict on its account.
    pub cancel: CancellationToken,
    /// What the lane tells the user.
    pub report: Arc<std::sync::Mutex<LaneReport>>,
}

/// The serving account's label, shown only for a pool of several (senpi's footer rule), and
/// account switches to announce.
#[derive(Debug, Default)]
pub struct LaneReport {
    pub account: Option<String>,
    pub notices: Vec<String>,
}

impl LaneReport {
    pub fn take_notices(report: &std::sync::Mutex<Self>) -> Vec<String> {
        report
            .lock()
            .map(|mut report| std::mem::take(&mut report.notices))
            .unwrap_or_default()
    }
}

fn is_oauth_token_name(name: &str) -> bool {
    name == "CLAUDE_CODE_OAUTH_TOKEN"
        || name
            .strip_prefix("CLAUDE_CODE_OAUTH_TOKEN_")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Removes every credential the managed lane injects itself, and the harness's own variables.
pub fn strip_managed_auth_environment(
    parent: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    const REMOVED: [&str; 9] = [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "ANTHROPIC_CUSTOM_HEADERS",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_FOUNDRY",
        "CLAUDE_CODE_USE_GATEWAY",
        "CLAUDE_CODE_USE_VERTEX",
    ];
    parent
        .iter()
        .filter(|(name, _)| {
            !REMOVED.contains(&name.as_str())
                && !is_oauth_token_name(name)
                && !name.starts_with("HARNESS_")
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

pub fn resolve_effective_lane(
    settings: &AnthropicSubscriptionSettings,
    accounts: &[AccountSlot],
) -> TokenInjection {
    settings.token_injection.unwrap_or(if accounts.is_empty() {
        TokenInjection::Ambient
    } else {
        TokenInjection::OauthSlots
    })
}

struct ManagedPool {
    accounts: Vec<AccountSlot>,
    lane: TokenInjection,
    pinned_account: Option<String>,
}

fn store_error(error: AccountStoreError) -> LaneError {
    LaneError::Message(error.to_string())
}

fn lookup(environment: &BTreeMap<String, String>) -> impl Fn(&str) -> Option<String> + '_ {
    move |name| environment.get(name).cloned()
}

fn managed_pool(input: &AuthLaneInput) -> Result<Option<ManagedPool>, LaneError> {
    let env = lookup(&input.environment);
    let mut stored = match &input.store {
        Some(store) => store.read().map_err(store_error)?,
        None => None,
    };
    let mut accounts = list_accounts(&stored.clone().unwrap_or_default(), Some(&env));
    if stored.is_none()
        && !accounts.is_empty()
        && let Some(store) = &input.store
    {
        stored = store
            .modify(&mut |current| Ok(current.is_none().then(AccountPool::default)))
            .map_err(store_error)?;
        accounts = list_accounts(&stored.clone().unwrap_or_default(), Some(&env));
    }
    let lane = resolve_effective_lane(&input.settings, &accounts);
    if lane == TokenInjection::Ambient {
        return Ok(None);
    }
    if accounts.is_empty() {
        return Err(LaneError::Message(NO_MANAGED_ACCOUNTS_ERROR.into()));
    }
    Ok(Some(ManagedPool {
        accounts,
        lane,
        pinned_account: input
            .settings
            .pinned_account
            .clone()
            .or_else(|| stored.and_then(|pool| pool.pinned)),
    }))
}

static TRANSIENT_REFRESH_FAILURE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"\bstatus=5\d\d\b|\bTimeoutError\b").ok());

/// Throttling, server errors and timeouts on the token endpoint are not a verdict on the grant.
fn refresh_failure(detail: &str) -> LaneError {
    let classification = classify_sdk_error(detail);
    if matches!(
        classification.kind,
        SdkErrorKind::RateLimit | SdkErrorKind::Overloaded
    ) {
        return LaneError::Message(detail.into());
    }
    if classification.kind == SdkErrorKind::Other && classification.retryable
        || TRANSIENT_REFRESH_FAILURE
            .as_ref()
            .is_some_and(|re| re.is_match(detail))
    {
        return LaneError::Message(format!("server_error: {detail}"));
    }
    LaneError::Message(format!("authentication_failed: {detail}"))
}

fn stored_slot(
    input: &AuthLaneInput,
    pool: Option<AccountPool>,
    name: &str,
) -> Option<AccountSlot> {
    let env = lookup(&input.environment);
    list_accounts(&pool.unwrap_or_default(), Some(&env))
        .into_iter()
        .find(|slot| slot.name == name)
}

/// Resolves the slot's token immediately before the spawn, refreshing a near-expiry grant.
async fn prepare_slot(
    input: &AuthLaneInput,
    lane: TokenInjection,
    slot: &mut AccountSlot,
) -> Result<(BTreeMap<String, String>, String), LaneError> {
    if slot.source != AccountSource::Env && now_ms() >= slot.expires - EXPIRING_WITHIN_MS {
        let store = input.store.as_ref().ok_or_else(|| {
            LaneError::Message("authentication_failed: account store unavailable".into())
        })?;
        let refreshed = tokio::select! {
            biased;
            // A cancelled turn is not an authentication verdict: an aborted refresh must not
            // surface as authentication_failed and block the account.
            () = input.cancel.cancelled() => return Err(LaneError::Message("Operation aborted".into())),
            refreshed = store.refresh_slot(&slot.name, &|expires| now_ms() >= expires - EXPIRING_WITHIN_MS) => refreshed,
        };
        match refreshed {
            Ok(refreshed) => {
                let updated = stored_slot(input, refreshed, &slot.name).ok_or_else(|| {
                    refresh_failure("selected account disappeared during refresh")
                })?;
                *slot = updated;
            }
            Err(AccountStoreError::Busy(busy)) => {
                // Contention says nothing about the credential: adopt a sibling's rotated
                // token, or keep the stored one while it is still inside its lifetime.
                let latest = stored_slot(input, store.read().map_err(store_error)?, &slot.name);
                match latest {
                    Some(latest) if latest.refresh != slot.refresh => *slot = latest,
                    _ if now_ms() < slot.expires => {}
                    _ => return Err(LaneError::Message(busy)),
                }
            }
            Err(error) => return Err(refresh_failure(&error.to_string())),
        }
    }
    let env = lookup(&input.environment);
    let access = if slot.source == AccountSource::Env {
        env_slot_token(&env, &slot.name).unwrap_or_default()
    } else {
        slot.access.clone()
    };
    if access.is_empty() {
        return Err(LaneError::Message(
            "authentication_failed: selected OAuth token is unavailable".into(),
        ));
    }
    let mut child = strip_managed_auth_environment(&input.environment);
    let digest = hex(&Sha256::digest(access.as_bytes()));
    if lane == TokenInjection::OauthSlots {
        child.insert("CLAUDE_CODE_OAUTH_TOKEN".into(), access);
        return Ok((child, digest));
    }
    let agent_dir = input.agent_dir.as_deref().ok_or_else(|| {
        LaneError::Message("config-dir token injection needs a data directory".into())
    })?;
    let directory = write_config_dir_credential(agent_dir, slot, &access)?;
    child.insert("CLAUDE_CONFIG_DIR".into(), directory.display().to_string());
    Ok((child, digest))
}

fn visible_sdk_message(message: &Value) -> bool {
    message["type"] == "stream_event"
        && matches!(
            message["event"]["type"].as_str(),
            Some("content_block_start" | "content_block_delta" | "content_block_stop")
        )
}

/// Resolves managed OAuth immediately before each subprocess spawn and retries only
/// pre-delta failures on the next account.
pub fn query_with_auth_lane(input: AuthLaneInput) -> MessageStream {
    Box::pin(async_stream::stream! {
        let pool = match managed_pool(&input) {
            Ok(pool) => pool,
            Err(error) => { yield Err(error); return; }
        };
        let Some(pool) = pool else {
            let mut options = match (input.build_options)(TokenInjection::Ambient) {
                Ok(options) => options,
                Err(error) => { yield Err(error); return; }
            };
            options.env = input
                .environment
                .iter()
                .filter(|(name, _)| !name.starts_with("HARNESS_"))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let attempt = (input.create_attempt)(AuthenticatedAttempt {
                account_name: "ambient".into(),
                accounts: Vec::new(),
                auth_lane: TokenInjection::Ambient,
                options,
                credential_digest: None,
            }).await;
            let mut attempt = match attempt {
                Ok(attempt) => attempt,
                Err(error) => { yield Err(error); return; }
            };
            // The caller throws on a failure message; the attempt is then discarded, not retained.
            while let Some(item) = attempt.messages.next().await {
                let item = match item {
                    Ok(event) => sdk_failure(&event).map_or(Ok(event), Err),
                    Err(error) => Err(error),
                };
                let failed = item.is_err();
                yield item;
                if failed {
                    if let Some(discard) = attempt.discard.take() { discard(); }
                    return;
                }
            }
            return;
        };
        let mut failover = Box::pin(run_failover(input.clone(), pool));
        while let Some(item) = failover.next().await {
            yield item;
        }
    })
}

const INTERRUPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Keeps a one-shot query alive for its stream; abandoning it mid-turn interrupts first.
struct QueryGuard {
    query: Arc<ClaudeQuery>,
    finished: bool,
}
impl QueryGuard {
    fn finish(&mut self) {
        self.finished = true;
    }
}
impl Drop for QueryGuard {
    fn drop(&mut self) {
        if self.finished {
            self.query.close();
            return;
        }
        let query = Arc::clone(&self.query);
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    let _ = tokio::time::timeout(INTERRUPT_TIMEOUT, query.interrupt()).await;
                    query.close();
                });
            }
            Err(_) => self.query.close(),
        }
    }
}

/// The non-resident attempt: one prompt message, closed when its run ends. On abort it
/// interrupts, closes, and drains what Claude Code already sent.
pub fn one_shot_attempt(prompt: Value, cancel: CancellationToken) -> AttemptFactory {
    Arc::new(move |auth: AuthenticatedAttempt| {
        let (prompt, cancel) = (prompt.clone(), cancel.clone());
        Box::pin(async move {
            let query = Arc::new(
                ClaudeQuery::spawn(&auth.options, Prompt::Single(prompt))
                    .map_err(|e| LaneError::Message(e.0))?,
            );
            let mut guard = QueryGuard {
                query,
                finished: false,
            };
            let messages: MessageStream = Box::pin(async_stream::stream! {
                let mut aborting = false;
                loop {
                    let item = tokio::select! {
                        biased;
                        () = cancel.cancelled(), if !aborting => {
                            aborting = true;
                            let query = Arc::clone(&guard.query);
                            tokio::spawn(async move {
                                let _ = tokio::time::timeout(INTERRUPT_TIMEOUT, query.interrupt()).await;
                                query.close();
                            });
                            continue;
                        }
                        item = guard.query.next() => item,
                    };
                    let Some(item) = item else { break };
                    yield item.map_err(|e| LaneError::Message(e.0));
                }
                guard.finish();
            });
            Ok(Attempt {
                messages,
                discard: None,
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn managed_environment_drops_injected_credentials_and_harness_variables() {
        let parent = BTreeMap::from([
            ("PATH".to_owned(), "/bin".to_owned()),
            ("ANTHROPIC_API_KEY".to_owned(), "k".to_owned()),
            ("CLAUDE_CODE_OAUTH_TOKEN_3".to_owned(), "t".to_owned()),
            ("HARNESS_DATA_HOME".to_owned(), "/d".to_owned()),
            ("CLAUDE_CODE_OAUTH_TOKEN_X".to_owned(), "kept".to_owned()),
        ]);
        let child = strip_managed_auth_environment(&parent);
        assert_eq!(
            child.keys().collect::<Vec<_>>(),
            ["CLAUDE_CODE_OAUTH_TOKEN_X", "PATH"]
        );
        assert_eq!(
            refresh_failure("HTTP request failed. status=503; body=x").message(),
            "server_error: HTTP request failed. status=503; body=x"
        );
        assert!(
            refresh_failure("status=400; body={\"error\":\"invalid_grant\"}")
                .message()
                .starts_with("authentication_failed:")
        );
        assert_eq!(retry_after_ms("retry-after: 2"), Some(2000));
    }
}
