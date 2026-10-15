//! When maintenance runs: threshold checks around a request, and recovery after a provider
//! rejection or an output-limit stop.
use super::*;

mod body_timeout;
mod length;
mod overflow;
mod payload;
mod threshold;

pub(in crate::coord) use length::LengthAction;

/// Where a threshold check runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord) enum Site {
    /// Before a provider request: the pre-prompt check (the turn has no assistant entry
    /// yet) or, inside a tool loop, the mid-turn check.
    PreRequest,
    /// After the turn's answer was stored.
    PostTurn {
        /// The answer ended the turn with text only, so nothing needs to resume.
        terminal_text_answer: bool,
    },
}

/// What a threshold check did, for the caller that owns the next provider call.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord) struct Maintained {
    /// History was rewritten or the model was promoted: rebuild the request.
    pub changed: bool,
    /// A continuation was scheduled after the pass.
    pub continuation_scheduled: bool,
    /// The scheduled continuation is a synthetic resume the caller appends and loops on.
    pub synthetic_resume: bool,
    /// The pass paused automatic work on purpose.
    pub blocked: bool,
}

impl Worker {
    /// Index of the first entry of the current turn when that turn has not reached the
    /// provider yet (no assistant response so far).
    fn pending_turn(&self, context: &Context) -> Option<usize> {
        let start = context
            .entries
            .iter()
            .position(|e| e.turn.as_deref() == Some(self.turn.id.as_str()))?;
        (!context.entries[start..]
            .iter()
            .any(|e| e.message.role == harness_providers::MessageRole::Assistant))
        .then_some(start)
    }

    /// Runs one automatic pass. Cancellation and failures end it without an outcome;
    /// a failure other than cancellation is reported as `failure: {error}`.
    async fn recovery_pass(
        &mut self,
        context: &mut Context,
        request: Request,
        failure: &str,
    ) -> Result<Option<Outcome>, CoordinatorError> {
        // Session-end recovery (overflow, output limit) lets a background summary or handoff
        // still being written finish first, so the pass claims it (and a handoff document is
        // saved) instead of racing it. A threshold pass above the grace cap does not wait.
        // Work that arrived meanwhile owns the next call and ends the recovery.
        if request.reason.will_retry() {
            let generation = self.guidance.input_generation;
            self.await_speculation_settled().await?;
            if self.cancellation.is_cancelled() {
                return Err(CoordinatorError::Cancelled(self.turn.id.clone()));
            }
            if self.guidance.input_generation != generation {
                return Ok(None);
            }
        }
        match self.run_pass(context, request).await {
            Ok(outcome) => Ok(Some(outcome)),
            Err(CoordinatorError::CompactionCancelled { .. }) => Ok(None),
            Err(error) if !self.cancellation.is_cancelled() => {
                self.warn(format!("{failure}: {error}")).await?;
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}
