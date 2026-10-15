//! The model's own request for a fresh window: acted on at the boundary of the tool batch
//! that carried it, never as a fallback to another method.
use super::super::maintain::{Reason, Request, TurnPhase};
use crate::coord::{context::Context, turn::Worker, *};

/// True when every entry of a finished batch, and the assistant message that produced it,
/// is already part of the journal. A rollover would otherwise drop entries it cannot recover.
fn persisted(context: &Context, batch_from: usize) -> bool {
    context.entries[batch_from.saturating_sub(1)..]
        .iter()
        .all(|entry| entry.seq > 0)
}

impl Worker {
    /// Rolls the window over when a `new_context` call was acknowledged in the batch that
    /// just finished. It bypasses the `enabled` and mid-turn settings but not the safety,
    /// ownership or window gates, and it never starts another compaction method. Returns
    /// true when history changed.
    pub(in crate::coord) async fn explicit_new_context(
        &mut self,
        context: &mut Context,
        batch_from: usize,
    ) -> Result<bool, CoordinatorError> {
        if !self.compaction.notes_windows {
            return Ok(false);
        }
        let (agent, turn) = (
            self.actor.agent_id.clone().unwrap_or_default(),
            self.turn.id.clone(),
        );
        // The marker is turn-local: an acknowledgement from another turn is stale.
        let requested = self
            .handle
            .call(move |s| {
                Ok(s.notes
                    .get_mut(&agent)
                    .and_then(|state| state.new_context_turn.take())
                    .is_some_and(|marked| marked == turn))
            })
            .await?;
        if !requested {
            return Ok(false);
        }
        if self.cancellation.is_cancelled() {
            return Err(CoordinatorError::Cancelled(self.turn.id.clone()));
        }
        let measure = self.measure(context)?;
        if !self.notes_usable()
            || !measure.window_known
            || measure.window == 0
            || !persisted(context, batch_from)
        {
            return Ok(false);
        }
        let request = Request {
            explicit_new_context: true,
            retry_intent: Some(false),
            auto_continue: false,
            suppress_continuation: true,
            phase: TurnPhase::Mid,
            detach_post_commit: true,
            trigger_tokens: Some(measure.tokens),
            ..Request::automatic(Reason::Threshold, &[], None)
        };
        match self.run_pass(context, request).await {
            Ok(outcome) => {
                if outcome.blocked {
                    self.park_context();
                }
                Ok(outcome.rewritten)
            }
            Err(CoordinatorError::CompactionCancelled { .. }) => Ok(false),
            Err(error) if !self.cancellation.is_cancelled() => {
                self.warn(format!("Auto-compaction failed: {error}"))
                    .await?;
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }
}
