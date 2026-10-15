//! What a maintenance pass is asked to do and what it reports back.
use super::super::commit::Committed;
use harness_compaction::CompactionMethod;

/// Why a maintenance pass runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord) enum Reason {
    Manual,
    Threshold,
    Overflow,
    Incomplete,
    Idle,
}

impl Reason {
    pub(in crate::coord) const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Threshold => "threshold",
            Self::Overflow => "overflow",
            Self::Incomplete => "incomplete",
            Self::Idle => "idle",
        }
    }
    /// Recovery passes retry the failed request by default.
    pub(super) const fn will_retry(self) -> bool {
        matches!(self, Self::Overflow | Self::Incomplete)
    }
    pub(in crate::coord) const fn automatic(self) -> bool {
        !matches!(self, Self::Manual)
    }
}

/// Where in a turn a pass runs. Providers see it in the compaction descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord) enum TurnPhase {
    /// Before the next prompt or at the end of a turn.
    Pre,
    /// Inside a continuing tool loop or while recovering a failed request.
    Mid,
    /// Outside any turn (idle, speculation).
    Standalone,
}

impl TurnPhase {
    pub(in crate::coord) const fn as_str(self) -> &'static str {
        match self {
            Self::Pre => "pre_turn",
            Self::Mid => "mid_turn",
            Self::Standalone => "standalone_turn",
        }
    }
}

/// One maintenance invocation.
#[derive(Debug, Clone)]
pub(in crate::coord) struct Request {
    pub reason: Reason,
    /// Recorded trigger label (`threshold`, `overflow`, or a caller-supplied manual reason).
    pub label: String,
    pub order: Vec<CompactionMethod>,
    /// Public focus the user typed; hooks see it.
    pub focus: Option<String>,
    /// Host guidance the summarizer follows (it overrides `focus`); hooks never see it.
    pub internal_guidance: Option<String>,
    /// Entries from this index on have not reached the provider yet and are always kept.
    pub pending_from: Option<usize>,
    /// The order was forced by an explicit `/compact` mode.
    pub forced: bool,
    /// Media-limit evidence: image-producing methods may not run.
    pub exclude_media: bool,
    /// The handoff method runs only when explicitly requested by `/handoff` or automatically.
    pub handoff_requested: bool,
    /// The invocation's own retry intent; `None` follows the reason (overflow and incomplete
    /// retry). It is the `will_retry` the lifecycle end event reports.
    pub retry_intent: Option<bool>,
    /// The caller allows a synthetic continuation (the `auto_continue` setting also applies).
    pub auto_continue: bool,
    /// The caller owns the next call: no continuation of any kind is scheduled.
    pub suppress_continuation: bool,
    /// Occupancy that triggered the pass, when the caller measured it.
    pub trigger_tokens: Option<u32>,
    /// Tokens of messages not yet in the prepared history.
    pub pending_tokens: u32,
    /// Stored context size before preparation changed its representation.
    pub prepared_tokens: Option<u32>,
    pub phase: TurnPhase,
    /// The assistant ended with a text-only answer, so nothing needs to resume.
    pub terminal_text_answer: bool,
    /// Post-commit fan-out may finish after the pass returns (mid-turn boundaries).
    pub detach_post_commit: bool,
    /// First index of `order` the walk starts from.
    pub method_offset: usize,
    /// An earlier shake already fell through; rescue does not repeat the elide step.
    pub fallback_from_shake: bool,
    /// The model asked for a new context window.
    pub explicit_new_context: bool,
}

impl Request {
    pub(in crate::coord) fn automatic(
        reason: Reason,
        order: &[CompactionMethod],
        pending_from: Option<usize>,
    ) -> Self {
        Self {
            reason,
            label: reason.as_str().to_owned(),
            order: harness_compaction::method::dedupe_order(order),
            focus: None,
            internal_guidance: None,
            pending_from,
            forced: false,
            exclude_media: false,
            handoff_requested: false,
            retry_intent: None,
            auto_continue: true,
            suppress_continuation: false,
            trigger_tokens: None,
            pending_tokens: 0,
            prepared_tokens: None,
            phase: match reason {
                Reason::Threshold => TurnPhase::Pre,
                Reason::Idle => TurnPhase::Standalone,
                Reason::Manual | Reason::Overflow | Reason::Incomplete => TurnPhase::Mid,
            },
            terminal_text_answer: false,
            detach_post_commit: false,
            method_offset: 0,
            fallback_from_shake: false,
            explicit_new_context: false,
        }
    }

    /// The invocation's retry intent: explicit when the caller set one, else by reason.
    pub(in crate::coord) fn will_retry(&self) -> bool {
        self.retry_intent
            .unwrap_or_else(|| self.reason.will_retry())
    }

    /// The pass keeps the agent working afterwards and so is judged by headroom, not fit,
    /// unless it retries a failed request.
    pub(in crate::coord) const fn checks_progress(&self) -> bool {
        self.reason.automatic() && !matches!(self.reason, Reason::Idle)
    }
}

/// What a pass did.
#[derive(Debug, Default)]
pub(in crate::coord) struct Outcome {
    pub committed: Option<Committed>,
    /// Some history was rewritten (a commit, shake or rescue).
    pub rewritten: bool,
    /// Automatic continuation should pause: the pass could not create headroom.
    pub blocked: bool,
    /// After the pass the context fits the model's prompt budget.
    pub fits: bool,
    /// The pass switched to a larger-context model instead of compacting.
    pub promoted: bool,
    /// The pass ended without running a method (no model, nothing eligible).
    pub skipped: bool,
    /// A continuation (queued work or a synthetic resume) was scheduled after the pass.
    pub continuation_scheduled: bool,
    /// The scheduled continuation is a synthetic resume the caller must append and continue.
    pub synthetic_resume: bool,
    /// Why nothing happened, for manual callers.
    pub noop: Option<String>,
    /// Shake summary line for `/shake` and automatic shake notices.
    pub shake: Option<(String, u32)>,
    /// Where a handoff document was saved, when the pass wrote one.
    pub saved_path: Option<String>,
}
