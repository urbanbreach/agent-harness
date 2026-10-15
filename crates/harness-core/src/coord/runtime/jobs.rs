//! Work the coordinator runs outside its actor and the completions it receives back.
use super::*;

pub(in crate::coord) struct Job {
    pub join_id: Option<Id>,
    pub actor: EventActor,
    pub kind: JobKind,
    pub parent: Option<String>,
    pub cancellation: CancellationToken,
    pub reason: Option<String>,
    pub hooks: Vec<HookExecutionMetadata>,
}
pub(in crate::coord) enum JobKind {
    BranchSummary,
    SubagentPreparation {
        agent: String,
    },
    Command,
    Turn {
        agent: String,
    },
    Tool {
        tool_id: String,
        reply: Option<Reply<ToolResult>>,
        capability: crate::tool::ToolCapability,
        paths: Vec<PathBuf>,
    },
}
pub(in crate::coord) enum Completion {
    BranchSummary {
        id: String,
        result: Result<BranchSummaryEvent, CoordinatorError>,
    },
    SubagentPrepared {
        id: String,
        result: Result<crate::coord::public_subagents::PreparedSubagent, CoordinatorError>,
    },
    Command {
        id: String,
        result: Result<crate::coord::commands::CommandExit, CoordinatorError>,
    },
    Turn {
        id: String,
        messages: Context,
        result: Result<String, CoordinatorError>,
    },
    Tool {
        id: String,
        result: Result<ToolResult, CoordinatorError>,
        instructions: Vec<crate::coord::instructions::Instruction>,
    },
}

impl Runtime {
    /// A worker task panicked or was aborted: settle its job with a failure, and stop the
    /// run when a turn's context was lost with it.
    pub(super) fn worker_failed(&mut self, error: tokio::task::JoinError) {
        let Some((id, job)) = self
            .running
            .iter()
            .find(|(_, j)| j.join_id == Some(error.id()))
        else {
            return;
        };
        let lost_context = matches!(job.kind, JobKind::Turn { .. });
        let message = "worker stopped unexpectedly";
        let failure = CoordinatorError::Invalid(message.into());
        let completion = match job.kind {
            JobKind::BranchSummary => Completion::BranchSummary {
                id: id.clone(),
                result: Err(failure),
            },
            JobKind::Turn { .. } => Completion::Turn {
                id: id.clone(),
                messages: Context::default(),
                result: Err(failure),
            },
            JobKind::Tool { .. } => Completion::Tool {
                id: id.clone(),
                result: Err(failure),
                instructions: Vec::new(),
            },
            JobKind::Command => Completion::Command {
                id: id.clone(),
                result: Err(failure),
            },
            JobKind::SubagentPreparation { .. } => Completion::SubagentPrepared {
                id: id.clone(),
                result: Err(failure),
            },
        };
        if lost_context {
            self.fault = Some(message.into());
        }
        let _ = self.finished(completion);
        if lost_context && self.stopping.is_none() {
            let (reply, _) = oneshot::channel();
            let _ = self.stop(Some(message.into()), reply);
        }
    }
}
