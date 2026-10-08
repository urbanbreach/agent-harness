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
