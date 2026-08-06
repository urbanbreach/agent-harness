use super::cancel::{visuals, CancelStage, QueueVisuals};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueLifecycle {
    Idle,
    Streaming,
    Tool,
    Waiting,
    Cancelling,
    Completed,
    Failed,
}

impl QueueLifecycle {
    pub const fn has_work(self) -> bool {
        matches!(
            self,
            Self::Streaming | Self::Tool | Self::Waiting | Self::Cancelling
        )
    }

    pub const fn is_busy(self) -> bool {
        self.has_work()
    }
}

impl fmt::Display for QueueLifecycle {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Idle => "idle",
            Self::Streaming => "streaming",
            Self::Tool => "tool",
            Self::Waiting => "waiting",
            Self::Cancelling => "cancelling",
            Self::Completed => "completed",
            Self::Failed => "failed",
        };
        formatter.write_str(label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedItem {
    pub id: String,
    pub text: String,
    pub is_interjection: bool,
}

impl QueuedItem {
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            is_interjection: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueState {
    pub lifecycle: QueueLifecycle,
    pub queued: Vec<QueuedItem>,
    pub draft: String,
    pub cancel_stage: Option<CancelStage>,
}

impl Default for QueueState {
    fn default() -> Self {
        Self::new(QueueLifecycle::Idle)
    }
}

impl QueueState {
    pub fn new(lifecycle: QueueLifecycle) -> Self {
        Self {
            lifecycle,
            queued: Vec::new(),
            draft: String::new(),
            cancel_stage: None,
        }
    }

    pub fn with_draft(mut self, draft: impl Into<String>) -> Self {
        self.draft = draft.into();
        self
    }

    pub fn with_queued(mut self, queued: Vec<QueuedItem>) -> Self {
        self.queued = queued;
        self
    }

    pub fn queued_ids(&self) -> Vec<&str> {
        self.queued.iter().map(|item| item.id.as_str()).collect()
    }

    pub fn visuals(&self) -> QueueVisuals {
        visuals(self.lifecycle, self.cancel_stage)
    }
}
