//! Tool call leaf view.

/// Deterministic view state for a tool call row in the transcript.
///
/// No app-state or registry dependency — a plain `Copy` value type.
/// Captures the tool identity, lifecycle status, and whether a permission
/// gate precedes execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ToolLeafView {
    pub tool_id: &'static str,
    pub status: ToolStatusLeaf,
    pub permission_pending: bool,
    pub permission_granted: bool,
    pub has_diff: bool,
    pub has_error: bool,
    pub truncated: bool,
}

/// Lightweight tool-call lifecycle status mirroring
/// `app::ToolCallDisplayStatus` without the full projection dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolStatusLeaf {
    #[default]
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl ToolLeafView {
    pub const fn new(tool_id: &'static str, status: ToolStatusLeaf) -> Self {
        Self {
            tool_id,
            status,
            permission_pending: false,
            permission_granted: false,
            has_diff: false,
            has_error: false,
            truncated: false,
        }
    }

    /// Mark the tool as awaiting a permission decision.
    pub const fn permission_pending(mut self) -> Self {
        self.permission_pending = true;
        self
    }

    /// Mark the tool's permission as granted (permission gate passed).
    pub const fn permission_granted(mut self) -> Self {
        self.permission_granted = true;
        self.permission_pending = false;
        self
    }

    /// Mark the tool as producing a file diff.
    pub const fn with_diff(mut self) -> Self {
        self.has_diff = true;
        self
    }

    /// Mark the tool as having an error output.
    pub const fn with_error(mut self) -> Self {
        self.has_error = true;
        self
    }

    /// Mark the tool output as truncated.
    pub const fn truncated(mut self) -> Self {
        self.truncated = true;
        self
    }

    /// Returns true when permission was resolved before tool execution
    /// (the canonical ordering invariant: permission before tool).
    pub fn permission_before_tool(&self) -> bool {
        self.permission_granted && !self.permission_pending
    }
}
