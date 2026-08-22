use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    Idle,
    Drafting,
    Submitting,
    Streaming,
    Thinking,
    Tool,
    Diff,
    Permission,
    Question,
    Queued,
    Interjected,
    Cancelling,
    Recovering,
    Failed,
    Completed,
    Compacting,
}

impl LifecycleState {
    pub const ALL: [Self; 16] = [
        Self::Idle,
        Self::Drafting,
        Self::Submitting,
        Self::Streaming,
        Self::Thinking,
        Self::Tool,
        Self::Diff,
        Self::Permission,
        Self::Question,
        Self::Queued,
        Self::Interjected,
        Self::Cancelling,
        Self::Recovering,
        Self::Failed,
        Self::Completed,
        Self::Compacting,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Drafting => "drafting",
            Self::Submitting => "submitting",
            Self::Streaming => "streaming",
            Self::Thinking => "thinking",
            Self::Tool => "tool",
            Self::Diff => "diff",
            Self::Permission => "permission",
            Self::Question => "question",
            Self::Queued => "queued",
            Self::Interjected => "interjected",
            Self::Cancelling => "cancelling",
            Self::Recovering => "recovering",
            Self::Failed => "failed",
            Self::Completed => "completed",
            Self::Compacting => "compacting",
        }
    }
}
