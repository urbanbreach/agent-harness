use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    User,
    Assistant,
    Thinking,
    Tool,
    Diff,
    System,
}

impl BlockKind {
    pub const ALL: [Self; 6] = [
        Self::User,
        Self::Assistant,
        Self::Thinking,
        Self::Tool,
        Self::Diff,
        Self::System,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Thinking => "thinking",
            Self::Tool => "tool",
            Self::Diff => "diff",
            Self::System => "system",
        }
    }
}
