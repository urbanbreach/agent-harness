//! Recovery after the provider rejected the request as too long.
use super::*;

/// How one overflow attempt ended.
pub(super) struct Attempt {
    /// The request should be sent again.
    pub retry: bool,
    /// The pass ran and rewrote history, even if the context still does not fit.
    pub rewritten: bool,
}

impl Worker {
    /// Returns true when the request should be sent again.
    pub(in crate::coord) async fn overflow_maintenance(
        &mut self,
        context: &mut Context,
    ) -> Result<bool, CoordinatorError> {
        Ok(self.overflow_attempt(context, true, false).await?.retry)
    }

    /// A larger-context model first when `promote` allows it, then a recovery pass with
    /// retry intent. `exclude_media` bars image-producing methods. Retries only when the
    /// outcome fits the prompt budget.
    pub(super) async fn overflow_attempt(
        &mut self,
        context: &mut Context,
        promote: bool,
        exclude_media: bool,
    ) -> Result<Attempt, CoordinatorError> {
        if promote && self.promote(PromoteWhy::Overflow).await? {
            return Ok(Attempt {
                retry: true,
                rewritten: false,
            });
        }
        let none = Attempt {
            retry: false,
            rewritten: false,
        };
        if !self.effective_enabled() {
            return Ok(none);
        }
        let order = self.compaction.method_order.clone();
        let mut request = Request::automatic(Reason::Overflow, &order, self.pending_turn(context));
        request.exclude_media = exclude_media;
        request.auto_continue = false;
        request.suppress_continuation = true;
        let Some(outcome) = self
            .recovery_pass(context, request, "Context overflow recovery failed")
            .await?
        else {
            return Ok(none);
        };
        Ok(Attempt {
            retry: (outcome.rewritten || outcome.promoted) && outcome.fits,
            rewritten: outcome.rewritten,
        })
    }
}
