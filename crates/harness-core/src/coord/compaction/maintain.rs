//! Maintenance passes: measurement, method selection, progress guard and dead-end rescue.
use super::super::{context::Context, turn::Worker, *};
use super::commit::{Committed, Draft};
use super::generate::{Transport, WalkMode};
use super::progress::Progress;
use super::reduce::RescueGoal;
use super::{CompactionError, CompactionErrorKind};
use crate::config::{CompactionThreshold, HookLifecycleEvent as Hook};
use crate::event::CompactionPhase;
use harness_compaction::{
    files, policy,
    policy::{ThresholdOverride, ThresholdSettings},
    prepare::{self, Preparation, PrepareInput},
    prompts, tokens, CompactionMethod, ItemKind,
};
use serde_json::json;

mod request;
pub(in crate::coord) use request::{Outcome, Reason, Request, TurnPhase};
/// Context measurements for threshold decisions.
#[derive(Debug, Clone)]
pub(in crate::coord) struct Measure {
    /// Context window used for threshold decisions: the catalog window, else the request
    /// budget's conservative fallback; zero when neither exists.
    pub window: u32,
    /// The catalog states this model's window. Rescue fit, payload byte text and promotion
    /// only trust a window that is known; admission alone may fall back.
    pub window_known: bool,
    pub threshold: u32,
    /// max(provider, local).
    pub tokens: u32,
    /// Provider-anchored estimate: the newest qualifying usage plus the local tail after it.
    pub provider: u32,
    /// Local estimate of the next request, system prompt and tools included.
    pub local: u32,
    /// System prompt, tools and framing.
    pub fixed: u32,
    /// The request budget's input limit; `u32::MAX` when unknown.
    pub input_cap: u32,
    pub settings: ThresholdSettings,
}

impl Measure {
    pub(in crate::coord) fn over_threshold(&self) -> bool {
        policy::should_compact(self.tokens, self.window, self.threshold)
    }
    pub(super) fn prompt_budget(&self) -> u32 {
        policy::prompt_budget(self.window, self.settings.reserve_tokens).min(self.input_cap)
    }
    pub(in crate::coord) fn fits(&self) -> bool {
        self.window == 0 || self.tokens <= self.prompt_budget()
    }
    pub(super) fn has_headroom(&self) -> bool {
        self.window == 0 || self.tokens <= policy::recovery_target(self.threshold)
    }
}

enum MethodResult {
    Committed(Committed),
    /// A mechanical reduction ran; `enough` means it reached the recovery target.
    Reduced {
        enough: bool,
    },
    /// The method cannot run here; the message says why (for warnings).
    Unavailable(Option<String>),
    /// There is no older history to compact.
    NothingToCompact,
}

fn threshold_override(value: CompactionThreshold, agent: bool) -> ThresholdOverride {
    match value {
        CompactionThreshold::Percent(percent) => ThresholdOverride::Percent(percent.get()),
        CompactionThreshold::Tokens { tokens } => ThresholdOverride::Fixed(tokens.get()),
        CompactionThreshold::Base { base } if agent => ThresholdOverride::Fixed(base.get()),
        CompactionThreshold::Base { base } => ThresholdOverride::BaseWindow(base.get()),
    }
}

/// The pause warning after a pass that cannot create headroom. `frames` counts archive
/// frames still held in context; they are named first because providers often bill vision
/// media separately from text tokens.
pub(in crate::coord) fn dead_end_message(prepared: bool, frames: usize) -> String {
    let remedies = if prepared {
        "clear large tool output"
    } else {
        "shrink it (e.g. clear large tool output)"
    };
    let remedies = if frames > 0 {
        format!(
            "reduce archived image frames ({frames} held) — providers often bill vision media separately from tokens; {remedies}"
        )
    } else {
        remedies.to_owned()
    };
    format!(
        "Compaction freed too little context to make progress — pausing automatic maintenance to avoid a compaction loop. The most recent turn alone is too large to reduce further; {remedies} or switch to a larger-context model."
    )
}
/// Archive frames attached to the active summary.
fn held_frames(context: &Context) -> usize {
    context
        .entries
        .get(context.body_start())
        .filter(|entry| {
            entry
                .compaction
                .as_ref()
                .is_some_and(|state| state.archive.is_some())
        })
        .map_or(0, |entry| entry.attachments.len())
}
/// Estimated tokens of everything after the active summary.
fn tail_tokens(context: &Context, counter: &harness_compaction::tokenizer::Counter) -> u32 {
    let items = context.items_with(counter);
    let start = usize::from(items.first().is_some_and(|i| i.kind == ItemKind::Summary));
    items[start..]
        .iter()
        .fold(0u32, |n, item| n.saturating_add(item.tokens))
}

fn noop_reason(context: &Context, handoff: bool) -> String {
    let body = context.body_start();
    let messages = context.entries.len().saturating_sub(body);
    match (handoff, messages) {
        (true, 0 | 1) => "Nothing to hand off (no messages yet)".into(),
        (true, _) => "Nothing to hand off (already compacted)".into(),
        (false, _)
            if context
                .entries
                .get(body)
                .is_some_and(|e| e.compaction.is_some())
                && messages == 1 =>
        {
            "Already compacted".into()
        }
        (false, _) => "Nothing to compact (session too small)".into(),
    }
}

mod continuation;
pub(in crate::coord::compaction) mod guard;
mod idle_guard;
pub(in crate::coord::compaction) mod lifecycle;
mod measurement;
pub(in crate::coord::compaction) mod methods;
mod overlay;
mod parked;
mod promote;
mod status;
mod triggers;
mod walk;
pub(in crate::coord) use continuation::{
    delivered_content, drop_orphan_tool_use, terminal_text_answer, Continuation,
    ContinuationSource, RECOVERY_DELAY,
};
pub(in crate::coord) use promote::PromoteWhy;
pub(in crate::coord) use triggers::{LengthAction, Maintained, Site};

impl Worker {
    /// Runs one maintenance pass: lifecycle events, live progress, failure hooks, and the
    /// continuation that follows. Pass failures other than cancellation reach the caller.
    pub(in crate::coord) async fn run_pass(
        &mut self,
        context: &mut Context,
        request: Request,
    ) -> Result<Outcome, CoordinatorError> {
        let agent = self
            .actor
            .agent_id
            .clone()
            .ok_or_else(|| CoordinatorError::Invalid("compaction needs an agent".into()))?;
        // A notes rollover with nothing to roll over ends before any event exists.
        if let Some(outcome) = self.notes_unprepared(context, &request)? {
            return Ok(outcome);
        }
        let progress = Progress::start(
            &self.handle,
            &self.actor,
            &self.turn.id,
            &agent,
            &request.label,
            &self.cancellation,
        )
        .await?;
        self.lifecycle_start(&progress, &request).await?;
        let result = self.maintain_inner(context, &request, &progress).await;
        let cancelled = progress.cancel.is_cancelled();
        let (phase, message) = match &result {
            Ok(outcome) if outcome.committed.is_some() || outcome.shake.is_some() => {
                (CompactionPhase::Done, None)
            }
            Ok(outcome) if outcome.promoted => (
                CompactionPhase::Skipped,
                Some("Switched to a larger-context model".to_owned()),
            ),
            Ok(outcome) => (CompactionPhase::Skipped, outcome.noop.clone()),
            Err(_) if cancelled => (CompactionPhase::Cancelled, None),
            Err(error) => (CompactionPhase::Failed, Some(error.to_string())),
        };
        let mut hook = Ok(());
        if let Err(error) = &result {
            let (task, owner, failure) =
                (self.turn.id.clone(), self.actor.clone(), error.to_string());
            // The end event must go out even when the hook call cannot.
            hook = self
                .handle
                .call(move |s| {
                    s.hook(
                        Hook::CompactionFailed,
                        &owner,
                        Some(&task),
                        json!({"outcome":"failed","failure_reason":failure}),
                    )
                    .map(|_| ())
                })
                .await;
        }
        let aborted =
            cancelled || matches!(result, Err(CoordinatorError::CompactionCancelled { .. }));
        self.lifecycle_end(
            &progress,
            &request,
            lifecycle::End {
                outcome: result.as_ref().ok(),
                aborted,
                error: result
                    .as_ref()
                    .err()
                    .filter(|_| !aborted)
                    .map(ToString::to_string),
            },
        )
        .await?;
        progress.finish(phase, message).await?;
        hook?;
        match result {
            Ok(mut outcome) => {
                self.continue_after(&request, &mut outcome).await?;
                Ok(outcome)
            }
            // Only typed cancellation is relabeled; an unrelated failure keeps its identity
            // even when the pass was cancelled meanwhile.
            Err(CoordinatorError::Cancelled(_)) if cancelled => {
                Err(CoordinatorError::CompactionCancelled {
                    agent_id: agent,
                    reason: super::CancelReason::Manual,
                })
            }
            Err(error) => Err(error),
        }
    }

    /// Decides what follows a finished pass. Recovery passes report whether the request
    /// fits again (the caller retries); a threshold pass in a running turn goes through the
    /// shared continuation scheduler.
    async fn continue_after(
        &mut self,
        request: &Request,
        outcome: &mut Outcome,
    ) -> Result<(), CoordinatorError> {
        if !request.checks_progress() {
            return Ok(());
        }
        let acted = outcome.committed.is_some() || outcome.rewritten || outcome.promoted;
        if request.will_retry() {
            outcome.continuation_scheduled = acted && outcome.fits && !outcome.blocked;
            return Ok(());
        }
        if !(acted || outcome.blocked) {
            return Ok(());
        }
        let source = ContinuationSource::AfterPass {
            suppress: request.suppress_continuation,
            auto_continue: request.auto_continue
                && self.compaction.auto_continue
                && !outcome.blocked,
            terminal_text_answer: request.terminal_text_answer,
            blocked: outcome.blocked,
        };
        let generation = self.guidance.input_generation;
        match self.schedule_continuation(source, generation).await? {
            Continuation::Stop => {}
            Continuation::Blocked => outcome.blocked = true,
            Continuation::Queue | Continuation::Retry => outcome.continuation_scheduled = true,
            Continuation::Resume => {
                outcome.continuation_scheduled = true;
                outcome.synthetic_resume = true;
            }
        }
        Ok(())
    }
}
