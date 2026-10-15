//! Recovery after a response stopped at its output limit.
use super::*;
use harness_compaction::{prompts, tokens};

/// What to do with a response that stopped at its output limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord) enum LengthAction {
    /// Keep the truncated answer.
    Keep,
    /// Drop it and send the request again.
    Retry,
}

/// Consecutive `length` stops without delivered content before giving up.
const LENGTH_RECOVERY_CAP: u32 = 3;

/// The window is exhausted when it is unknown or the adjusted usage is strictly above the
/// threshold.
fn window_exhausted(window: u32, tokens: u32, threshold: u32) -> bool {
    window == 0 || tokens > threshold
}

impl Worker {
    async fn keep_truncated(&self, output_tokens: u32) -> Result<LengthAction, CoordinatorError> {
        let model = crate::agent::AgentModelRef::parse(&self.turn.model);
        self.warn(format!(
            "Response hit the {}/{} output limit ({output_tokens} tokens) and was truncated.",
            model.provider_id, model.model_id
        ))
        .await?;
        Ok(LengthAction::Keep)
    }

    /// Retries after the recovery delay.
    async fn retry_after_pause(&self) -> Result<LengthAction, CoordinatorError> {
        self.recovery_pause().await?;
        Ok(LengthAction::Retry)
    }

    /// Decides what to do with a response that hit its output limit. The response came from
    /// the active model, so the same-model rule holds by construction.
    pub(in crate::coord) async fn length_recovery(
        &mut self,
        context: &mut Context,
        text: &str,
        delivered_calls: bool,
        output_tokens: u32,
        usage: Option<&harness_providers::CompletionUsage>,
    ) -> Result<LengthAction, CoordinatorError> {
        let delivered = delivered_content(text, delivered_calls, false);
        let measure = self.measure(context)?;
        // The truncated response's own usage is the freshest account of the context.
        let reported = usage.map_or(0, tokens::context_tokens);
        let exhausted = window_exhausted(
            measure.window,
            measure.tokens.max(reported),
            measure.threshold,
        );
        if delivered {
            self.guidance.length_recoveries = 0;
            if !exhausted {
                return self.keep_truncated(output_tokens).await;
            }
        }
        // Without automatic maintenance there is nothing to shrink and no retry loop.
        if !self.auto_compaction_available() {
            return self.keep_truncated(output_tokens).await;
        }
        let model = crate::agent::AgentModelRef::parse(&self.turn.model);
        if !delivered {
            if self.guidance.length_recoveries >= LENGTH_RECOVERY_CAP {
                let attempts = self.guidance.length_recoveries + 1;
                self.guidance.length_recoveries = 0;
                return Err(CoordinatorError::Invalid(format!(
                    "Length-stop recovery gave up after {attempts} consecutive `length` responses from {}/{} with no text or tool call. Try switching models or raising the model's max output tokens.",
                    model.provider_id, model.model_id
                )));
            }
            self.guidance.length_recoveries += 1;
        }
        if self.promote(PromoteWhy::Incomplete).await? {
            self.guidance.length_recoveries = 0;
            return self.retry_after_pause().await;
        }
        if exhausted {
            let order = self.compaction.method_order.clone();
            let mut request = Request::automatic(Reason::Incomplete, &order, None);
            request.auto_continue = false;
            request.suppress_continuation = true;
            let outcome = self
                .recovery_pass(context, request, "Incomplete response recovery failed")
                .await?;
            if outcome.is_some_and(|outcome| {
                (outcome.rewritten || outcome.promoted) && outcome.fits && !outcome.blocked
            }) {
                return self.retry_after_pause().await;
            }
            if delivered {
                return self.keep_truncated(output_tokens).await;
            }
        }
        if output_tokens > 0 {
            let body = prompts::length_stop_reminder(
                output_tokens,
                self.guidance.length_recoveries,
                LENGTH_RECOVERY_CAP,
            );
            self.remind(RuntimeReminderKind::LengthStop, body, None, context)
                .await?;
        }
        self.retry_after_pause().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhaustion_is_an_unknown_window_or_usage_strictly_above_the_threshold() {
        for (window, tokens, threshold, exhausted) in [
            (0, 10, u32::MAX, true),
            (1000, 800, 800, false),
            (1000, 801, 800, true),
            (1000, 10, 800, false),
        ] {
            assert_eq!(
                window_exhausted(window, tokens, threshold),
                exhausted,
                "{window} {tokens} {threshold}"
            );
        }
    }
}
