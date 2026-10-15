//! Live notices and the start/end lifecycle events of a maintenance pass.
use super::*;
use crate::event::{
    CompactionEndResult, CompactionLifecycleEvent, LifecyclePhase, NoticeSeverity, NoticeSource,
};

/// The lifecycle `action` for a pass: what the UI calls the work that is starting or ended.
pub(super) fn action(request: &Request, method: Option<CompactionMethod>) -> &'static str {
    if request.explicit_new_context {
        return "new-context";
    }
    if request.reason == Reason::Idle {
        return "idle";
    }
    match method.or_else(|| request.order.first().copied()) {
        Some(CompactionMethod::Handoff) => "handoff",
        Some(CompactionMethod::Shake) => "shake",
        _ => "context-full",
    }
}

/// How a pass ended, as the lifecycle end event reports it.
pub(in crate::coord::compaction) struct End<'a> {
    pub outcome: Option<&'a Outcome>,
    pub aborted: bool,
    pub error: Option<String>,
}

impl Worker {
    /// Emits a user-visible notice from the compaction subsystem.
    pub(in crate::coord) async fn notice(
        &self,
        severity: NoticeSeverity,
        message: String,
    ) -> Result<(), CoordinatorError> {
        let (actor, task) = (self.actor.clone(), self.turn.id.clone());
        self.handle
            .call(move |s| {
                s.live(
                    actor,
                    task,
                    LiveEventV1::RuntimeNotice {
                        source: NoticeSource::Compaction,
                        severity,
                        message,
                    },
                )
            })
            .await
    }

    pub(in crate::coord) async fn warn(&self, message: String) -> Result<(), CoordinatorError> {
        self.notice(NoticeSeverity::Warning, message).await
    }

    pub(in crate::coord) async fn emit_lifecycle(
        &self,
        event: CompactionLifecycleEvent,
    ) -> Result<(), CoordinatorError> {
        let (actor, task) = (self.actor.clone(), self.turn.id.clone());
        self.handle
            .call(move |s| s.live(actor, task, LiveEventV1::CompactionLifecycle(event)))
            .await
    }

    pub(in crate::coord::compaction) async fn lifecycle_start(
        &self,
        progress: &Progress,
        request: &Request,
    ) -> Result<(), CoordinatorError> {
        self.emit_lifecycle(CompactionLifecycleEvent {
            agent_id: self.actor.agent_id.clone().unwrap_or_default(),
            operation_id: progress.operation_id.clone(),
            phase: LifecyclePhase::Start,
            reason: request.reason.as_str().to_owned(),
            action: action(request, None).to_owned(),
            will_retry: request.will_retry(),
            aborted: false,
            skipped: false,
            error: None,
            result: None,
        })
        .await
    }

    /// The end event. `will_retry` is the invocation's retry intent, not proof that a fit
    /// check scheduled the retry.
    pub(in crate::coord::compaction) async fn lifecycle_end(
        &self,
        progress: &Progress,
        request: &Request,
        end: End<'_>,
    ) -> Result<(), CoordinatorError> {
        let committed = end.outcome.and_then(|o| o.committed.as_ref());
        let result = committed.map(|c| CompactionEndResult {
            method: (!c.method_omitted).then_some(c.method),
            tokens_before: Some(c.tokens_before),
            tokens_after: Some(c.tokens_after),
            short_summary: c.short_summary.clone(),
            warning: c.warning.clone(),
            first_kept_seq: c.first_kept_seq,
        });
        let skipped = !end.aborted
            && end.error.is_none()
            && end.outcome.is_some_and(|o| {
                o.committed.is_none() && o.shake.is_none() && !o.rewritten && !o.promoted
            });
        self.emit_lifecycle(CompactionLifecycleEvent {
            agent_id: self.actor.agent_id.clone().unwrap_or_default(),
            operation_id: progress.operation_id.clone(),
            phase: LifecyclePhase::End,
            reason: request.reason.as_str().to_owned(),
            action: action(request, committed.map(|c| c.method)).to_owned(),
            will_retry: request.will_retry(),
            aborted: end.aborted,
            skipped,
            error: end.error,
            result,
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_names_what_the_pass_is_doing() {
        for (reason, order, method, new_context, expected) in [
            (
                Reason::Threshold,
                vec![CompactionMethod::Soft],
                None,
                false,
                "context-full",
            ),
            (
                Reason::Threshold,
                vec![CompactionMethod::Handoff],
                None,
                false,
                "handoff",
            ),
            (
                Reason::Overflow,
                vec![CompactionMethod::Shake],
                None,
                false,
                "shake",
            ),
            (
                Reason::Overflow,
                vec![CompactionMethod::Remote],
                None,
                false,
                "context-full",
            ),
            (
                Reason::Idle,
                vec![CompactionMethod::Handoff],
                None,
                false,
                "idle",
            ),
            (
                Reason::Threshold,
                vec![CompactionMethod::Soft],
                None,
                true,
                "new-context",
            ),
            (
                Reason::Threshold,
                vec![CompactionMethod::Handoff, CompactionMethod::Soft],
                Some(CompactionMethod::Soft),
                false,
                "context-full",
            ),
        ] {
            let mut request = Request::automatic(reason, &order, None);
            request.explicit_new_context = new_context;
            assert_eq!(action(&request, method), expected, "{reason:?} {order:?}");
        }
    }
}
