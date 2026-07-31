//! Shell lifecycle status surface — Todo 26.
//!
//! Standalone leaf module defining the ordered shell lifecycle states,
//! their status labels, and context-usage/model/effort bar data.
//! Included via `#[path]` in the parity test; not registered in `app.rs`.

use crate::app::RuntimeStateKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShellStatus {
    Idle,
    Streaming,
    PermissionBlocked,
    PermissionPending,
    ToolQueued,
    ToolRunning,
    ToolSucceeded,
    ToolFailed,
    TurnComplete,
    PostRun,
    PostRunFailure,
    ReplayReadOnly,
    Cancelled,
    Degraded,
    Disconnected,
    Failure,
    Startup,
}

impl ShellStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Ready",
            Self::Streaming => "Streaming",
            Self::PermissionBlocked => "Permission blocked",
            Self::PermissionPending => "Permission pending",
            Self::ToolQueued => "Tool queued",
            Self::ToolRunning => "Tool running",
            Self::ToolSucceeded => "Tool finished",
            Self::ToolFailed => "Tool failed",
            Self::TurnComplete => "Turn complete",
            Self::PostRun => "Run finished",
            Self::PostRunFailure => "Run failed",
            Self::ReplayReadOnly => "Replay \u{00b7} read-only",
            Self::Cancelled => "Cancelled",
            Self::Degraded => "Degraded",
            Self::Disconnected => "Disconnected",
            Self::Failure => "Failure",
            Self::Startup => "Startup",
        }
    }

    pub fn composer_disabled(self) -> bool {
        matches!(
            self,
            Self::PermissionPending
                | Self::PostRun
                | Self::PostRunFailure
                | Self::ReplayReadOnly
                | Self::Degraded
                | Self::Disconnected
        )
    }

    pub fn is_read_only(self) -> bool {
        matches!(self, Self::ReplayReadOnly)
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::PostRun | Self::PostRunFailure)
    }

    pub fn is_recoverable(self) -> bool {
        matches!(
            self,
            Self::ToolFailed
                | Self::Cancelled
                | Self::Degraded
                | Self::Disconnected
                | Self::Failure
        )
    }

    pub fn from_runtime_state(kind: RuntimeStateKind) -> Self {
        match kind {
            RuntimeStateKind::Ready => Self::Idle,
            RuntimeStateKind::Sending => Self::Streaming,
            RuntimeStateKind::Streaming => Self::Streaming,
            RuntimeStateKind::Success => Self::TurnComplete,
            RuntimeStateKind::Failure => Self::Failure,
            RuntimeStateKind::Cancelled => Self::Cancelled,
            RuntimeStateKind::PermissionBlocked => Self::PermissionBlocked,
            RuntimeStateKind::PermissionPending => Self::PermissionPending,
            RuntimeStateKind::Degraded => Self::Degraded,
            RuntimeStateKind::Disconnected => Self::Disconnected,
        }
    }

    pub const ORDERED_LIFECYCLE: [Self; 10] = [
        Self::Idle,
        Self::Streaming,
        Self::PermissionBlocked,
        Self::ToolQueued,
        Self::ToolRunning,
        Self::ToolSucceeded,
        Self::TurnComplete,
        Self::PostRun,
        Self::PostRunFailure,
        Self::ReplayReadOnly,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextUsageBar {
    pub tokens: Option<u32>,
    pub compacted_pending_refresh: bool,
    pub label: &'static str,
}

impl ContextUsageBar {
    pub fn from_tokens(tokens: u32) -> Self {
        Self {
            tokens: Some(tokens),
            compacted_pending_refresh: false,
            label: "Context",
        }
    }

    pub fn compacted_pending_refresh() -> Self {
        Self {
            tokens: None,
            compacted_pending_refresh: true,
            label: "Context \u{00b7} compacted",
        }
    }

    pub fn is_visible(&self) -> bool {
        self.tokens.is_some() || self.compacted_pending_refresh
    }

    pub fn usage_percent(&self) -> Option<u32> {
        self.tokens.map(|t| t / 128_000)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelBar {
    pub model_id: String,
    pub profile_label: String,
    pub mode_label: Option<String>,
}

impl ModelBar {
    pub fn new(model_id: &str, profile_label: &str, mode_label: Option<&str>) -> Self {
        Self {
            model_id: model_id.to_string(),
            profile_label: profile_label.to_string(),
            mode_label: mode_label.map(str::to_string),
        }
    }

    pub fn display_label(&self) -> String {
        match &self.mode_label {
            Some(mode) => format!("{} \u{00b7} {}", self.profile_label, mode),
            None => self.profile_label.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffortBar {
    Low,
    Medium,
    High,
}

impl EffortBar {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}
