//! Provider dispatch, retry backoff, and rejection recovery.
use super::*;

impl Worker {
    pub(in crate::coord::turn) async fn dispatch(
        &mut self,
        messages: &mut crate::coord::context::Context,
        dispatched: &mut bool,
    ) -> Result<(String, crate::coord::streaming::Response), CoordinatorError> {
        let (mut checked, mut overflow_passes, mut body_recovered) = (false, 0u32, false);
        let mut first_failure = None;
        let mut retry = ProviderRequestRetryMetadata {
            attempt: 0,
            max_attempts: self.retry.max_retries.saturating_add(1),
            delay_ms: Some(0),
            category: None,
            failure: None,
        };
        loop {
            self.sync_compaction_status(messages).await;
            self.prepare_reader_views(messages).await?;
            self.prime_inline_imaging(messages).await;
            let request = self.request(messages);
            let budget = self.request_budget(&request)?;
            // Compaction lane policy: when the backend compacts its own resident session,
            // automatic compaction and the input-budget stop stand down; manual compaction and
            // overflow recovery (the provider decides which overflows are ours) still run.
            let native_context = self.provider.manages_context(&request);
            if self.native {
                let id = self.turn.id.clone();
                let owner = self.actor.agent_id.clone().unwrap_or_default();
                let tokens = messages.native_estimated_tokens_with(&self.counter);
                let window = self
                    .turn
                    .target
                    .as_ref()
                    .and_then(|target| target.limits.context_window.tokens)
                    .map(u64::from);
                self.handle
                    .call(move |runtime| {
                        runtime.check_task(&id)?;
                        let agent = runtime
                            .agents
                            .get_mut(&owner)
                            .ok_or(CoordinatorError::UnknownAgent(owner))?;
                        agent.native_context_tokens = tokens;
                        agent.native_context_window = window;
                        Ok(())
                    })
                    .await?;
            }
            if !native_context && !checked {
                checked = true;
                if self
                    .threshold_maintenance(messages, crate::coord::compaction::Site::PreRequest)
                    .await?
                    .changed
                {
                    continue;
                }
            }
            if budget.requires_compaction == Some(true) && !native_context {
                return Err(CoordinatorError::Invalid("context exceeds its input budget; compact the conversation or shorten the prompt".into()));
            }
            let permit = tokio::select! {
                biased;
                () = self.cancellation.cancelled() => return Err(CoordinatorError::Cancelled(self.turn.id.clone())),
                permit = Arc::clone(&self.permits).acquire_owned() => permit.map_err(|_| CoordinatorError::Closed)?,
            };
            let selection = self.selection(&request)?;
            *dispatched = true;
            let mut progress = false;
            let result = crate::coord::streaming::complete(
                &self.handle,
                self.provider.as_ref(),
                &self.actor,
                &self.turn.id,
                request,
                selection,
                budget,
                &self.cancellation,
                retry,
                self.behavior.stream_guard.enabled,
                &mut progress,
            )
            .await;
            drop(permit);
            match self
                .recover_rejection(
                    &result,
                    messages,
                    &mut overflow_passes,
                    &mut checked,
                    &mut body_recovered,
                )
                .await?
            {
                Recovery::Retry => continue,
                Recovery::Stop => return result,
                Recovery::Pass => {}
            }
            let Err(error) = result else {
                return result;
            };
            if matches!(error, CoordinatorError::StreamGuard(_)) {
                return Err(error);
            }
            if let Some((category, delay_ms)) = self.retry_delay(&error, retry.attempt, progress) {
                if let CoordinatorError::Provider {
                    category, message, ..
                } = &error
                {
                    retry.failure = Some(harness_providers::ProviderRetryFailure::from_error(
                        *category, message,
                    ));
                }
                first_failure.get_or_insert(error);
                retry.attempt += 1;
                retry.category = Some(category);
                retry.delay_ms = Some(delay_ms);
                self.retry_backoff(retry, delay_ms).await?;
                continue;
            }
            return Err(if matches!(error, CoordinatorError::Provider { .. }) {
                first_failure.unwrap_or(error)
            } else {
                error
            });
        }
    }
    async fn retry_backoff(
        &self,
        retry: ProviderRequestRetryMetadata,
        delay_ms: u64,
    ) -> Result<(), CoordinatorError> {
        let (actor, turn_id) = (self.actor.clone(), self.turn.id.clone());
        self.handle
            .call(move |runtime| {
                runtime.live(
                    actor,
                    turn_id.clone(),
                    LiveEventV1::ProviderRetrying {
                        turn_id: turn_id.into(),
                        retry,
                    },
                )
            })
            .await?;
        let deadline = tokio::time::Instant::now()
            .checked_add(std::time::Duration::from_millis(delay_ms))
            .ok_or_else(|| {
                CoordinatorError::Invalid("provider retry delay exceeds clock range".into())
            })?;
        tokio::select! {
            biased;
            () = self.cancellation.cancelled() => return Err(CoordinatorError::Cancelled(self.turn.id.clone())),
            () = tokio::time::sleep_until(deadline) => {}
        }
        Ok(())
    }
    fn retry_delay(
        &self,
        error: &CoordinatorError,
        attempt: u32,
        progress: bool,
    ) -> Option<(ProviderErrorCategory, u64)> {
        let CoordinatorError::Provider {
            category: Some(category),
            retry_after_ms,
            ..
        } = error
        else {
            return None;
        };
        if progress
            || attempt >= self.retry.max_retries
            || !matches!(
                category,
                ProviderErrorCategory::RateLimited | ProviderErrorCategory::TransportFailure
            )
        {
            return None;
        }
        let exponential = self
            .retry
            .base_delay_ms
            .saturating_mul(1_u64 << attempt.min(63));
        Some((
            *category,
            retry_after_ms
                .unwrap_or(exponential)
                .min(self.retry.max_delay_ms),
        ))
    }

    /// Routes a rejected request to recovery. A request-body read timeout shrinks tool
    /// results once. Payload evidence is classified before generic overflow, so a bare 413
    /// reaches the byte guard: a size or media rejection is not a token problem and either
    /// shrinks older history by a safe method or explains the limit and stops. Overflow
    /// recovery follows for token-context evidence.
    async fn recover_rejection(
        &mut self,
        result: &Result<(String, crate::coord::streaming::Response), CoordinatorError>,
        messages: &mut crate::coord::context::Context,
        passes: &mut u32,
        checked: &mut bool,
        body_recovered: &mut bool,
    ) -> Result<Recovery, CoordinatorError> {
        use harness_compaction::payload::{is_body_read_timeout, Rejection};
        let Err(CoordinatorError::Provider {
            category,
            message,
            envelope,
            ..
        }) = result
        else {
            return Ok(Recovery::Pass);
        };
        let status = envelope
            .as_ref()
            .and_then(|envelope| envelope.status)
            .or_else(|| {
                match harness_providers::ProviderRetryFailure::from_error(*category, message) {
                    harness_providers::ProviderRetryFailure::HttpStatus(status) => Some(status),
                    _ => None,
                }
            });
        if !*body_recovered && is_body_read_timeout(status, message) {
            *body_recovered = true;
            return Ok(if self.body_read_timeout_recovery(messages).await? {
                Recovery::Retry
            } else {
                Recovery::Pass
            });
        }
        if *passes >= OVERFLOW_PASSES {
            return Ok(Recovery::Pass);
        }
        let rejection = Rejection {
            message,
            status,
            payload_flag: false,
            overflow_flag: *category == Some(ProviderErrorCategory::ContextWindowExceeded),
            usage: envelope
                .as_ref()
                .and_then(|envelope| envelope.usage.as_ref())
                .and_then(reported_usage),
        };
        if rejection.is_payload() {
            *passes += 1;
            *checked = true;
            if self.payload_recovery(messages, &rejection).await? {
                self.recovery_pause().await?;
                return Ok(Recovery::Retry);
            }
            return Ok(Recovery::Stop);
        }
        if rejection.is_overflow() && self.compaction.auto_retry_overflow {
            *passes += 1;
            *checked = true;
            if self.overflow_maintenance(messages).await? {
                self.recovery_pause().await?;
                return Ok(Recovery::Retry);
            }
        }
        Ok(Recovery::Pass)
    }
}

/// What the failed request itself reported: raw input is input plus cache reads and writes,
/// and the context value is stated only when the usage carries one. The request is the
/// current one by definition, so no staleness check applies.
fn reported_usage(
    usage: &harness_providers::CompletionUsage,
) -> Option<harness_compaction::payload::ReportedUsage> {
    use harness_compaction::tokens;
    if !tokens::has_context_usage(usage) {
        return None;
    }
    let split = u64::from(usage.input_tokens)
        + u64::from(usage.cache_read_tokens)
        + u64::from(usage.cache_write_tokens);
    Some(harness_compaction::payload::ReportedUsage {
        raw_input: if split > 0 {
            split
        } else {
            u64::from(usage.prompt_tokens)
        },
        context_tokens: Some(u64::from(tokens::context_tokens(usage))),
    })
}

/// What dispatch does after a rejected request.
enum Recovery {
    /// History changed; send the request again.
    Retry,
    /// Return the rejection as the turn's failure.
    Stop,
    /// Continue with ordinary retry and fallback handling.
    Pass,
}
/// Overflow recovery passes allowed per provider request; each must make the request fit.
const OVERFLOW_PASSES: u32 = 3;
