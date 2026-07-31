//! Action leaf type.

/// Deterministic action leaf for TUI interaction shards.
///
/// No app-state or registry dependency — a plain `Copy` value type.
/// Covers shell, transcript, tool, diff, permission, and question
/// interaction surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActionLeaf {
    #[default]
    None,
    Submit,
    Cancel,
    NavigateUp,
    NavigateDown,
    /// Approve a pending permission gate.
    ApprovePermission,
    /// Deny a pending permission gate.
    DenyPermission,
    /// Answer a pending question prompt.
    AnswerQuestion,
    /// Cancel a question prompt (Esc).
    CancelQuestion,
    /// Scroll the transcript up by one page.
    ScrollUp,
    /// Scroll the transcript down by one page.
    ScrollDown,
    /// Scroll to the bottom of the transcript (latest content).
    ScrollToBottom,
    /// Expand a tool call or diff block in the transcript.
    Expand,
    /// Collapse a tool call or diff block in the transcript.
    Collapse,
    /// Recover from a failed state (retry or continue).
    Recover,
    /// Open a diff view for a tool call.
    OpenDiff,
}
