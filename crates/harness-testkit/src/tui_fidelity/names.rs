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
