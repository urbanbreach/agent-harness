use super::types::{AdapterKind, CheckpointName, SemanticState};

impl AdapterKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Grok => "grok",
            Self::Harness => "harness",
        }
    }
}

impl SemanticState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rest => "rest",
            Self::PromptReady => "prompt_ready",
            Self::Working => "working",
            Self::StartupReady => "startup_ready",
            Self::Streaming => "streaming",
            Self::ToolRunning => "tool_running",
            Self::ToolDone => "tool_done",
            Self::PermissionOpen => "permission_open",
            Self::QuestionOpen => "question_open",
            Self::Resized => "resized",
            Self::Settled => "settled",
        }
    }
}

impl CheckpointName {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rest => "rest",
            Self::Mid => "mid",
            Self::Settled => "settled",
        }
    }
}
