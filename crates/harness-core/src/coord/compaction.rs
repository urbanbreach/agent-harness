//! Context maintenance: compaction triggers, methods, commits and live progress.
use super::*;
use harness_compaction::{ManualMode, ShakeMode};
pub(in crate::coord) mod archive;
mod commit;
mod endpoint;
pub(in crate::coord) mod generate;
pub(in crate::coord) mod hooks;
mod maintain;
pub(in crate::coord) mod manual;
mod native;
pub(in crate::coord) mod native_store;
pub(in crate::coord) mod notes;
mod progress;
mod reduce;
mod reexpand;
mod speculate;
pub(in crate::coord) mod state;
mod types;
pub(in crate::coord) use maintain::{
    delivered_content, drop_orphan_tool_use, terminal_text_answer, Continuation,
    ContinuationSource, LengthAction, Maintained, PromoteWhy, Reason, Request, Site,
    RECOVERY_DELAY,
};
pub use manual::{HandoffRejection, ManualRejection, ResumeClaim};
pub(in crate::coord) use manual::{Manual, ManualKind, ManualReply};
pub use notes::handle::{NotesRevision, NotesSaved, NotesTicket};
pub(in crate::coord) use progress::Progress;
pub(in crate::coord) use speculate::Speculation;
pub use types::{
    CancelReason, CompactionError, CompactionErrorKind, ManualCompactionOptions,
    ManualCompactionOutcome,
};

/// Public refusal of a `/compact archive` request that carries focus or internal guidance.
const ARCHIVE_GUIDANCE_ERROR: &str = "/compact archive does not take focus instructions.";

/// What `/shake` removed, with the exact user-facing summary line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShakeOutcome {
    pub summary: String,
    pub tokens_freed: u32,
}

impl CoordinatorHandle {
    async fn queue_manual(
        &self,
        agent: String,
        kind: ManualKind,
        reason: String,
        reply: ManualReply,
        suppress: bool,
    ) -> Result<(), CoordinatorError> {
        if reason.is_empty() || reason.len() > 256 {
            return Err(CoordinatorError::Invalid(
                "invalid compaction reason".into(),
            ));
        }
        self.call(move |s| {
            s.queue_compaction(
                &agent,
                Manual {
                    kind,
                    reason,
                    reply,
                    interrupted: false,
                    suppress,
                },
            )
        })
        .await
    }
    /// Manual `/compact [mode] [focus]`. A running turn is interrupted, compacted and resumed.
    pub async fn compact_agent_context_with_options(
        &self,
        agent: impl Into<String>,
        options: ManualCompactionOptions,
    ) -> Result<ManualCompactionOutcome, CoordinatorError> {
        let instructions = options.instructions.filter(|s| !s.trim().is_empty());
        let guidance = options.internal_guidance.filter(|s| !s.trim().is_empty());
        if instructions
            .iter()
            .chain(&guidance)
            .any(|s| s.len() > 32 * 1024)
        {
            return Err(CoordinatorError::Invalid(
                "invalid compaction instructions or reason".into(),
            ));
        }
        if options.mode == Some(ManualMode::Archive)
            && (instructions.is_some() || guidance.is_some())
        {
            return Err(CoordinatorError::Invalid(ARCHIVE_GUIDANCE_ERROR.into()));
        }
        let (reply, response) = oneshot::channel();
        self.queue_manual(
            agent.into(),
            ManualKind::Compact {
                through: options.through_request_id,
                instructions,
                guidance,
                mode: options.mode,
            },
            options.reason,
            ManualReply::Compaction(reply),
            options.suppress_continuation,
        )
        .await?;
        response.await.map_err(|_| CoordinatorError::Closed)?
    }
    pub async fn compact_agent_context_with_instructions(
        &self,
        agent: impl Into<String>,
        through_request_id: Option<String>,
        trigger_reason: impl Into<String>,
        custom_instructions: Option<String>,
    ) -> Result<ManualCompactionOutcome, CoordinatorError> {
        self.compact_agent_context_with_options(
            agent,
            ManualCompactionOptions {
                through_request_id,
                reason: trigger_reason.into(),
                instructions: custom_instructions,
                mode: None,
                suppress_continuation: false,
                internal_guidance: None,
            },
        )
        .await
    }
    pub async fn compact_agent_context(
        &self,
        agent: impl Into<String>,
        through: Option<String>,
        reason: impl Into<String>,
    ) -> Result<ManualCompactionOutcome, CoordinatorError> {
        self.compact_agent_context_with_instructions(agent, through, reason, None)
            .await
    }
    /// `/handoff [focus]`: the live model writes a handoff document that replaces older history.
    pub async fn handoff_agent_context(
        &self,
        agent: impl Into<String>,
        focus: Option<String>,
    ) -> Result<ManualCompactionOutcome, CoordinatorError> {
        let focus = focus.filter(|s| !s.trim().is_empty());
        if focus.as_ref().is_some_and(|s| s.len() > 32 * 1024) {
            return Err(CoordinatorError::Invalid("invalid handoff focus".into()));
        }
        let (reply, response) = oneshot::channel();
        self.queue_manual(
            agent.into(),
            ManualKind::Handoff { focus },
            "manual".into(),
            ManualReply::Compaction(reply),
            false,
        )
        .await?;
        response.await.map_err(|_| CoordinatorError::Closed)?
    }
    /// `/shake [elide|images|thinking]`: mechanical removal without a model call.
    pub async fn shake_agent_context(
        &self,
        agent: impl Into<String>,
        mode: ShakeMode,
    ) -> Result<ShakeOutcome, CoordinatorError> {
        let (reply, response) = oneshot::channel();
        self.queue_manual(
            agent.into(),
            ManualKind::Shake { mode },
            "manual".into(),
            ManualReply::Shake(reply),
            false,
        )
        .await?;
        response.await.map_err(|_| CoordinatorError::Closed)?
    }
    /// Idle maintenance requested by an interactive client after its idle timer fired.
    pub async fn idle_compact_agent_context(
        &self,
        agent: impl Into<String>,
    ) -> Result<ManualCompactionOutcome, CoordinatorError> {
        let (reply, response) = oneshot::channel();
        self.queue_manual(
            agent.into(),
            ManualKind::Idle,
            "idle".into(),
            ManualReply::Compaction(reply),
            false,
        )
        .await?;
        response.await.map_err(|_| CoordinatorError::Closed)?
    }
    /// Cancels the agent's queued manual passes and any compaction in flight, then waits for
    /// the owning manual pass to finish its cleanup so a replacement prompt starts on a
    /// reconnected session.
    pub async fn cancel_compaction(
        &self,
        agent: impl Into<String>,
    ) -> Result<(), CoordinatorError> {
        let agent = agent.into();
        let barrier = self
            .call(move |s| {
                let state = s
                    .agents
                    .get(&agent)
                    .ok_or_else(|| CoordinatorError::UnknownAgent(agent.clone()))?;
                let queued: Vec<_> = state
                    .queue
                    .iter()
                    .filter(|t| t.manual.is_some())
                    .map(|t| t.id.clone())
                    .collect();
                for task in queued {
                    s.cancel(&task, "compaction cancelled")?;
                    s.settle_manual_task(&task);
                }
                if let Some((_, cancellation)) = s.compacting.get(&agent) {
                    cancellation.cancel();
                }
                Ok(s.manual_claims.barrier(&agent))
            })
            .await?;
        if let Some(mut barrier) = barrier {
            // A closed barrier means the pass is gone: also done waiting.
            let _ = barrier.wait_for(|done| *done).await;
        }
        Ok(())
    }
}
