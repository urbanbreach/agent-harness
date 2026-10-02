use super::*;
use harness_providers::{
    CompletionRequest, ProviderErrorCategory, ProviderRequestContext, ProviderRequestInitiator,
};

impl Worker {
    pub(super) fn update_prompt(&self, messages: &mut super::super::context::Context) {
        let Some(source) = &self.prompt_source else {
            return;
        };
        let family = self
            .turn
            .target
            .as_ref()
            .map_or(crate::model_resolution::PromptFamily::Default, |target| {
                target.resolution.prompt_family
            });
        let text = source.resolve(&self.profile.name, family, &self.workspace);
        if let Some(entry) = messages
            .entries
            .first_mut()
            .filter(|e| e.message.role == MessageRole::System)
        {
            entry.message.content = text;
        } else {
            let mut system = super::super::context::Context::new(&text);
            system.entries.append(&mut messages.entries);
            messages.entries = system.entries;
        }
    }
    fn request(&self, messages: &super::super::context::Context) -> CompletionRequest {
        let model = crate::agent::AgentModelRef::parse(&self.turn.model);
        CompletionRequest {
            provider_id: Some(model.provider_id),
            model_id: model.model_id,
            messages: messages.messages(),
            attachments: messages.attachments(),
            temperature: self.profile.temperature,
            max_tokens: self
                .turn
                .target
                .as_ref()
                .and_then(|t| t.limits.max_output_tokens()),
            variant: self.turn.settings.variant.clone(),
            reasoning_effort: self.turn.settings.reasoning_effort.clone(),
            text_verbosity: self.turn.settings.text_verbosity.clone(),
            reasoning_summary: self.turn.settings.reasoning_summary.clone(),
            thinking: self.turn.settings.thinking.clone(),
            tools: (!self.tools.is_empty()).then(|| self.tools.clone()),
            context: ProviderRequestContext {
                session_id: Some(self.session.clone()),
                request_id: Some(self.turn.id.clone()),
                initiator: if self.turn.user_initiated
                    && messages
                        .entries
                        .last()
                        .is_some_and(|e| e.message.role == MessageRole::User)
                {
                    ProviderRequestInitiator::User
                } else {
                    ProviderRequestInitiator::Agent
                },
                cache_retention: self.profile.cache_retention,
                has_media: messages
                    .entries
                    .iter()
                    .flat_map(|e| &e.attachments)
                    .any(|a| a.mime.starts_with("image/")),
            },
            stream: true,
            ..Default::default()
        }
    }
    fn selection(
        &self,
        request: &CompletionRequest,
    ) -> Result<Box<crate::session::CanonicalRuntimeSelection>, CoordinatorError> {
        Ok(Box::new(crate::session::CanonicalRuntimeSelection {
            profile: Some(self.profile.name.clone()),
            provider_id: request.provider_id.clone().unwrap_or_default(),
            model_id: request.model_id.clone(),
            variant: request.variant.clone(),
            reasoning_effort: request.reasoning_effort.clone(),
            text_verbosity: request.text_verbosity.clone(),
            reasoning_summary: request.reasoning_summary.clone(),
            thinking: request.thinking.clone(),
            resolved_limits: self
                .turn
                .target
                .as_ref()
                .map(|t| t.limits.clone())
                .unwrap_or_default(),
            profile_tool_shape_digest: digest(&serde_json::to_string(&self.tools)?),
        }))
    }
    pub(in crate::coord) fn request_budget(
        &self,
        request: &CompletionRequest,
    ) -> Result<crate::RequestBudget, CoordinatorError> {
        let unknown = crate::config::ResolvedModelLimits::default();
        let limits = self.turn.target.as_ref().map_or(&unknown, |t| &t.limits);
        limits
            .validate("active model")
            .map_err(|error| CoordinatorError::Invalid(error.to_string()))?;
        let pending = if request
            .messages
            .last()
            .is_some_and(|m| m.role == MessageRole::User)
        {
            request.messages.len() - 1
        } else {
            request.messages.len()
        };
        let semantics = self
            .provider
            .request_budget_semantics(request, pending)
            .map_err(|e| CoordinatorError::Invalid(e.to_string()))?;
        crate::compute_request_budget(crate::RequestBudgetInput {
            model_limits: limits,
            request_cost: semantics.request_cost,
            requested_output_tokens: request.max_tokens,
            safety_margin_tokens: 0,
            estimated_token_triggers: self.compaction.estimated_token_triggers,
            fallback_input_tokens: self.compaction.fallback_input_tokens,
            output_cap_disposition: semantics.output_cap_disposition,
        })
        .map_err(|e| CoordinatorError::Invalid(e.to_string()))
    }

    pub(super) async fn dispatch(
        &self,
        messages: &mut super::super::context::Context,
        dispatched: &mut bool,
    ) -> Result<(String, super::super::streaming::Response), CoordinatorError> {
        let automatic = self.compaction.enabled && !self.compaction.suppress_auto_compaction;
        let (mut prepared, mut retried) = (false, false);
        let mut first_failure = None;
        let mut retry = ProviderRequestRetryMetadata {
            attempt: 0,
            max_attempts: self.retry.max_retries.saturating_add(1),
            delay_ms: Some(0),
            category: None,
        };
        loop {
            let request = self.request(messages);
            let budget = self.request_budget(&request)?;
            if automatic
                && !prepared
                && self
                    .compaction_threshold(&budget)
                    .is_some_and(|threshold| budget.occupied_input_tokens >= threshold)
            {
                prepared = true;
                if matches!(
                    self.compact(messages, None, "threshold", None, Some(&budget))
                        .await?,
                    ManualCompactionOutcome::Compacted { .. }
                ) {
                    continue;
                }
            }
            if budget.requires_compaction == Some(true) {
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
            let result = super::super::streaming::complete(
                &self.handle,
                self.provider.as_ref(),
                &self.actor,
                &self.turn.id,
                request,
                selection,
                budget,
                &self.cancellation,
                retry,
                &mut progress,
            )
            .await;
            drop(permit);
            if automatic
                && self.compaction.auto_retry_overflow
                && !retried
                && matches!(
                    &result,
                    Err(CoordinatorError::Provider {
                        category: Some(ProviderErrorCategory::ContextWindowExceeded),
                        ..
                    })
                )
            {
                retried = true;
                prepared = true;
                if matches!(
                    self.compact(messages, None, "overflow", None, Some(&budget))
                        .await?,
                    ManualCompactionOutcome::Compacted { .. }
                ) {
                    continue;
                }
            }
            let Err(error) = result else {
                return result;
            };
            if let Some((category, delay_ms)) = self.retry_delay(&error, retry.attempt, progress) {
                first_failure.get_or_insert(error);
                retry.attempt += 1;
                retry.category = Some(category);
                retry.delay_ms = Some(delay_ms);
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
                continue;
            }
            return Err(if matches!(error, CoordinatorError::Provider { .. }) {
                first_failure.unwrap_or(error)
            } else {
                error
            });
        }
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
    pub(in crate::coord) fn compaction_threshold(
        &self,
        budget: &crate::RequestBudget,
    ) -> Option<u32> {
        use crate::config::CompactionThreshold;
        let maximum = budget.maximum_input_tokens?;
        let hard = maximum
            .saturating_sub(self.compaction.reserve_tokens)
            .max(1);
        let setting = self
            .compaction
            .agent_thresholds
            .get(&self.profile.name)
            .or_else(|| self.compaction.model_thresholds.get(&self.turn.model))
            .copied()
            .or_else(|| {
                self.compaction
                    .threshold_tokens
                    .map(|tokens| CompactionThreshold::Tokens { tokens })
            })
            .or_else(|| {
                self.compaction
                    .threshold_percent
                    .map(CompactionThreshold::Percent)
            });
        let capacity = self
            .turn
            .target
            .as_ref()
            .and_then(|t| t.limits.context_window_tokens());
        let soft = match (setting, capacity) {
            (Some(CompactionThreshold::Tokens { tokens }), _) => tokens.get(),
            (Some(CompactionThreshold::Percent(percent)), Some(window)) => {
                fraction(window, percent.get())
            }
            (None, Some(window)) => fraction(
                window,
                match window {
                    0..=16_000 => 45,
                    16_001..=32_000 => 50,
                    32_001..=64_000 => 55,
                    64_001..=128_000 => 60,
                    128_001..=512_000 => 70,
                    _ => 80,
                },
            ),
            _ => hard,
        };
        Some(soft.min(hard))
    }
}
fn fraction(window: u32, percent: u8) -> u32 {
    u32::try_from((u64::from(window) * u64::from(percent)).div_ceil(100)).unwrap_or(u32::MAX)
}
