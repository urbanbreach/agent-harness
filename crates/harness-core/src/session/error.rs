use crate::ids::{EntryId, RunId, SessionId, ToolCallId};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SessionError {
    #[error("record sequence {actual} is not contiguous after {expected_previous}")]
    NonContiguousSequence { expected_previous: u64, actual: u64 },
    #[error("record belongs to session {actual}, expected {expected}")]
    MixedSession {
        expected: SessionId,
        actual: SessionId,
    },
    #[error("record sequence {sequence} is duplicated")]
    DuplicateRecord { sequence: u64 },
    #[error("entry {entry_id} is duplicated")]
    DuplicateEntry { entry_id: EntryId },
    #[error("entry {entry_id} references missing parent {parent_id}")]
    MissingParent {
        entry_id: EntryId,
        parent_id: EntryId,
    },
    #[error("entry ancestry contains a cycle at {entry_id}")]
    ParentCycle { entry_id: EntryId },
    #[error("run attempt {run_id} is duplicated")]
    DuplicateRun { run_id: RunId },
    #[error("run attempt {run_id} does not exist")]
    UnknownRun { run_id: RunId },
    #[error("terminal run attempt {run_id} cannot be mutated")]
    TerminalRunMutation { run_id: RunId },
    #[error("active leaf {entry_id} does not exist")]
    ActiveLeafMissing { entry_id: EntryId },
    #[error("terminal session {session_id} cannot be mutated")]
    TerminalSessionMutation { session_id: SessionId },
    #[error("tool call {tool_call_id} is duplicated")]
    DuplicateToolCall { tool_call_id: ToolCallId },
    #[error("tool result references orphan tool call {tool_call_id}")]
    OrphanToolResult { tool_call_id: ToolCallId },
    #[error("tool result for {tool_call_id} is off the selected ancestry")]
    ToolResultOffActivePath { tool_call_id: ToolCallId },
    #[error("tool call {tool_call_id} already has a terminal result")]
    ToolResultAlreadySettled { tool_call_id: ToolCallId },
    #[error("tool result for {tool_call_id} references assistant {assistant_entry_id}")]
    SplitToolPair {
        tool_call_id: ToolCallId,
        assistant_entry_id: EntryId,
    },
}
