//! Shell area leaf view.

/// Deterministic view state for the shell surface.
///
/// Captures the shell kind and focus region for render shards without
/// pulling in `AppState` or the keybinding registry. The `shell_kind` field
/// is a static label so the type remains `Copy` and self-contained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShellLeafView {
    pub shell_kind: ShellKindLeaf,
    pub focus: FocusLeaf,
    pub streaming: bool,
    pub cancelled: bool,
    pub failed: bool,
    pub recovered: bool,
    pub completed: bool,
}

/// Lightweight shell-kind label mirroring `app::ShellKind` without the
/// full lifecycle dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShellKindLeaf {
    #[default]
    Live,
    Replay,
    Startup,
}

/// Lightweight focus label mirroring `app::Focus` without the full
/// lifecycle dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusLeaf {
    #[default]
    Composer,
    Transcript,
    Permission,
    Overlay,
}

impl ShellLeafView {
    pub const fn new(shell_kind: ShellKindLeaf, focus: FocusLeaf) -> Self {
        Self {
            shell_kind,
            focus,
            streaming: false,
            cancelled: false,
            failed: false,
            recovered: false,
            completed: false,
        }
    }

    /// Mark the shell as actively streaming a provider response.
    pub const fn streaming(mut self) -> Self {
        self.streaming = true;
        self
    }

    /// Mark the shell as cancelled mid-stream.
    pub const fn cancelled(mut self) -> Self {
        self.cancelled = true;
        self.streaming = false;
        self
    }

    /// Mark the shell as failed with a recoverable error.
    pub const fn failed(mut self) -> Self {
        self.failed = true;
        self.streaming = false;
        self
    }

    /// Mark the shell as recovered from a prior failure.
    pub const fn recovered(mut self) -> Self {
        self.recovered = true;
        self.failed = false;
        self
    }

    /// Mark the shell as completed (turn finished cleanly).
    pub const fn completed(mut self) -> Self {
        self.completed = true;
        self.streaming = false;
        self
    }

    /// Returns true when the shell is in an idle state (no active streaming,
    /// cancellation, failure, or completion in progress).
    pub fn is_idle(&self) -> bool {
        !self.streaming && !self.cancelled && !self.failed && !self.completed
    }
}
