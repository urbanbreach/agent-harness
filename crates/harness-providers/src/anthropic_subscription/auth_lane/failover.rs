//! Account failover.
use super::*;

pub(super) fn retry_after_ms(text: &str) -> Option<i64> {
    static MS: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(r"(?i)\bretry[-_ ]?after[-_ ]?ms\s*[:=]\s*(\d+(?:\.\d+)?)").ok()
    });
    static SECONDS: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"(?i)\bretry[-_ ]?after\s*[:=]\s*(\d+(?:\.\d+)?)").ok());
    let number = |re: &LazyLock<Option<Regex>>| {
        re.as_ref()?
            .captures(text)?
            .get(1)?
            .as_str()
            .parse::<f64>()
            .ok()
    };
    #[allow(clippy::cast_possible_truncation, reason = "bounded block windows")]
    number(&MS)
        .map(|ms| ms.ceil() as i64)
        .or_else(|| number(&SECONDS).map(|s| (s * 1000.0).ceil() as i64))
}

fn blocked_account(
    account: &AccountSlot,
    classification: &Classification,
    now: i64,
    attempt: u32,
    error: &LaneError,
    model: &str,
) -> AccountSlot {
    let mut blocked = account.clone();
    if classification.kind == SdkErrorKind::AuthError {
        blocked.blocked_until = None;
        blocked.block_reason = Some("auth_error".into());
        return blocked;
    }
    let fallback = MAX_RATE_LIMIT_BLOCK_MS
        .min(DEFAULT_RATE_LIMIT_BLOCK_MS.saturating_mul(1_i64 << attempt.min(40)));
    let text = error.message();
    if let Some(family) = &classification.model_family {
        // A model-scoped limit lasts until its own reset; the account keeps serving others.
        let reset = retry_after_ms(&text).or_else(|| usage_limit_reset_ms(&text, now));
        let duration = MAX_RATE_LIMIT_BLOCK_MS.min(reset.filter(|r| *r > 0).unwrap_or(fallback));
        let key = model_block_key(family, model);
        blocked.model_blocks = Some(with_model_block(
            account.model_blocks.as_ref(),
            &key,
            now + duration,
            now,
        ));
        return blocked;
    }
    let duration = MAX_RATE_LIMIT_BLOCK_MS.min(retry_after_ms(&text).unwrap_or(fallback));
    blocked.blocked_until = Some(now + duration);
    blocked.block_reason = Some(classification.kind.as_str().into());
    blocked
}

/// Persists a block onto the stored slot; returns the stored slot when an `auth_error`
/// verdict targets token material another writer already replaced.
fn persist_block(
    store: Option<&Arc<dyn SubscriptionAccountStore>>,
    account: &AccountSlot,
    now: i64,
) -> Result<Option<AccountSlot>, LaneError> {
    let Some(store) = store else {
        return Ok(None);
    };
    let mut superseded = None;
    store
        .modify(&mut |current| {
            let Some(mut pool) = current else {
                return Ok(None);
            };
            if account.source == AccountSource::Env {
                let previous = pool
                    .slot_state
                    .get(&account.name)
                    .and_then(|s| s.model_blocks.clone());
                pool.slot_state.insert(
                    account.name.clone(),
                    SlotState {
                        blocked_until: account.blocked_until,
                        block_reason: account.block_reason.clone(),
                        model_blocks: merge_model_blocks(
                            previous.as_ref(),
                            account.model_blocks.as_ref(),
                            now,
                        ),
                    },
                );
                return Ok(Some(pool));
            }
            if let Some(stored) = pool.accounts.iter().find(|s| s.name == account.name)
                && account.block_reason.as_deref() == Some("auth_error")
                && (stored.access != account.access || stored.refresh != account.refresh)
            {
                superseded = Some(stored.clone());
                return Ok(None);
            }
            for existing in &mut pool.accounts {
                if existing.name == account.name {
                    existing.blocked_until = account.blocked_until;
                    existing.block_reason = account.block_reason.clone();
                    existing.model_blocks = merge_model_blocks(
                        existing.model_blocks.as_ref(),
                        account.model_blocks.as_ref(),
                        now,
                    );
                }
            }
            Ok(Some(pool))
        })
        .map_err(store_error)?;
    Ok(superseded)
}

fn usable(account: &AccountSlot, now: i64, model: &str) -> bool {
    account.block_reason.is_none()
        && account.blocked_until.is_none_or(|until| until <= now)
        && active_model_block_until(account.model_blocks.as_ref(), Some(model), now).is_none()
}

/// At most one attempt per account; a retry is transparent only before a visible delta.
pub(super) fn run_failover(input: AuthLaneInput, pool: ManagedPool) -> MessageStream {
    Box::pin(async_stream::stream! {
        let mut accounts = clear_expired_blocks(&pool.accounts, now_ms());
        let mut last_error: Option<LaneError> = None;
        let mut retried_on_stored = HashSet::new();
        let pinned = input.pinned_account.clone().or_else(|| pool.pinned_account.clone());
        let select = |accounts: &[AccountSlot]| {
            select_account(accounts, &AffinityOptions {
                affinity_key: None,
                session_id: input.session_id.as_deref(),
                pinned_account: pinned.as_deref(),
                now: now_ms(),
                model: Some(&input.model),
            })
        };
        let mut attempt: u32 = 0;
        while (attempt as usize) < accounts.len() {
            let mut account = match select(&accounts) {
                Ok(account) => account,
                Err(error) => { yield Err(LaneError::AllAccountsBlocked(error)); return; }
            };
            let mut visible = false;
            let mut failure: Option<LaneError> = None;
            if let Ok(mut report) = input.report.lock() {
                report.account = (accounts.len() > 1).then(|| account_label(&account));
            }
            let options = (input.build_options)(pool.lane);
            let prepared = match options {
                Ok(options) => prepare_slot(&input, pool.lane, &mut account).await.map(|p| (options, p)),
                Err(error) => Err(error),
            };
            match prepared {
                Err(error) => failure = Some(error),
                Ok((mut options, (env, digest))) => {
                    options.env = env;
                    let created = (input.create_attempt)(AuthenticatedAttempt {
                        account_name: account.name.clone(),
                        accounts: pool.accounts.clone(),
                        auth_lane: pool.lane,
                        options,
                        credential_digest: Some(digest),
                    }).await;
                    match created {
                        Err(error) => failure = Some(error),
                        Ok(mut created) => {
                            while let Some(item) = created.messages.next().await {
                                match item {
                                    Err(error) => { failure = Some(error); break; }
                                    Ok(event) => {
                                        if let Some(error) = sdk_failure(&event) {
                                            failure = Some(error);
                                            break;
                                        }
                                        visible |= visible_sdk_message(&event);
                                        yield Ok(event);
                                    }
                                }
                            }
                            if failure.is_none() {
                                return;
                            }
                            if let Some(discard) = created.discard.take() { discard(); }
                        }
                    }
                }
            }
            let Some(error) = failure else { return; };
            if input.cancel.is_cancelled() {
                yield Err(error);
                return;
            }
            let classification = classify_lane_error(&error);
            let classified = LaneError::Classified {
                classification: classification.clone(),
                original: Box::new(error.clone()),
                suppress_turn_retry: visible,
            };
            last_error = Some(classified.clone());
            if !classification.retryable {
                yield Err(classified);
                return;
            }
            let now = now_ms();
            let blocked = blocked_account(&account, &classification, now, attempt, &error, &input.model);
            let superseded = match persist_block(input.store.as_ref(), &blocked, now) {
                Ok(superseded) => superseded,
                Err(error) => { yield Err(error); return; }
            };
            if let Some(stored) = superseded
                && !visible
                && usable(&stored, now_ms(), &input.model)
                && retried_on_stored.insert(account.name.clone())
            {
                // The rejected token was already replaced in the store: retry it once there.
                for slot in &mut accounts {
                    if slot.name == stored.name {
                        *slot = stored.clone();
                    }
                }
                continue;
            }
            for slot in &mut accounts {
                if slot.name == blocked.name {
                    *slot = blocked.clone();
                }
            }
            let next = if !visible && (attempt as usize) + 1 < accounts.len() {
                Some(select(&accounts))
            } else {
                None
            };
            tracing::info!(
                provider = PROVIDER_ID,
                from = %blocked.name,
                to = next.as_ref().and_then(|n| n.as_ref().ok()).map(|n| n.name.as_str()).unwrap_or(""),
                reason = classification.kind.as_str(),
                "anthropic-subscription account failover"
            );
            if let Some(Err(error)) = next {
                yield Err(LaneError::AllAccountsBlocked(error));
                return;
            }
            if let (Some(Ok(next)), Ok(mut report)) = (&next, input.report.lock()) {
                report.notices.push(format!(
                    "Switched Claude account: {} → {} ({})",
                    account_label(&blocked),
                    account_label(next),
                    classification.kind.as_str().replace('_', " ")
                ));
            }
            if visible {
                yield Err(classified);
                return;
            }
            attempt += 1;
        }
        yield Err(last_error.unwrap_or_else(|| LaneError::Message("Anthropic Subscription failover exhausted without an attempt".into())));
    })
}
