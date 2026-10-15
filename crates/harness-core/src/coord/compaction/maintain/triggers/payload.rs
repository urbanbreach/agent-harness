//! Recovery after the provider rejected the request's size or media budget. The pure
//! decision lives in `harness_compaction::payload`; this runs it against the context.
use super::*;
use harness_compaction::{
    payload::{Capabilities, Evidence, Rejection, Verdict},
    Stop,
};
use harness_providers::MessageRole;

impl Worker {
    /// Some older assistant answer exists that a usable method that does not produce media
    /// would compact: a summary of older history, or a mechanical reduction that picks its
    /// own victims. A cut that only falls inside the failing turn does not count.
    fn reclaimable_history(
        &self,
        context: &Context,
        request: &Request,
        pending: Option<usize>,
    ) -> bool {
        let answered = |stop: Stop| matches!(stop, Stop::Complete | Stop::Truncated);
        if !context
            .entries
            .iter()
            .any(|entry| entry.message.role == MessageRole::Assistant && answered(entry.stop))
        {
            return false;
        }
        let available = |wanted: &[CompactionMethod]| {
            request.order.iter().any(|method| {
                wanted.contains(method) && self.method_available(*method, request).is_ok()
            })
        };
        if available(&[CompactionMethod::Shake]) || self.notes_usable() {
            return true;
        }
        if !available(&[
            CompactionMethod::Remote,
            CompactionMethod::Soft,
            CompactionMethod::Archive,
        ]) {
            return false;
        }
        let Some((_, preparation)) = self.prepare(context, pending) else {
            return false;
        };
        context
            .items_with(&self.counter)
            .get(preparation.history.clone())
            .is_some_and(|older| {
                older
                    .iter()
                    .any(|item| item.is_assistant() && answered(item.stop))
            })
    }

    /// Returns true when the request should be sent again. Every other outcome carries one
    /// warning that names bytes or tokens correctly; the caller stops the turn.
    pub(in crate::coord) async fn payload_recovery(
        &mut self,
        context: &mut Context,
        rejection: &Rejection<'_>,
    ) -> Result<bool, CoordinatorError> {
        let measure = self.measure(context)?;
        let window = measure.window_known.then_some(u64::from(measure.window));
        let evidence = Evidence::assess(rejection, window, u64::from(measure.local));
        let pending = self.pending_turn(context);
        let order = self.compaction.method_order.clone();
        let mut probe = Request::automatic(Reason::Overflow, &order, pending);
        probe.exclude_media = evidence.exclude_archive();
        let usable = self.effective_enabled()
            && (self.notes_usable()
                || probe
                    .order
                    .iter()
                    .any(|method| self.method_available(*method, &probe).is_ok()));
        let capabilities = Capabilities {
            usable,
            reclaimable: usable && self.reclaimable_history(context, &probe, pending),
        };
        match evidence.verdict(capabilities) {
            Verdict::NotPayload => Ok(self.overflow_attempt(context, true, false).await?.retry),
            Verdict::Terminal(dead_end) => {
                self.warn(dead_end.warning()).await?;
                Ok(false)
            }
            Verdict::Shrink {
                exclude_media,
                promote,
            } => {
                let attempt = self
                    .overflow_attempt(context, promote, exclude_media)
                    .await?;
                if !attempt.retry && !attempt.rewritten {
                    self.warn(evidence.dead_end().warning()).await?;
                }
                Ok(attempt.retry)
            }
        }
    }
}
