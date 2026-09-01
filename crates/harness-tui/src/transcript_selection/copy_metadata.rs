#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    User,
    Assistant,
    Tool,
    System,
    Error,
}

impl BlockKind {
    const fn label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
            Self::System => "system",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyMetadata {
    pub turn_id: String,
    pub block_kind: BlockKind,
    pub timestamp: String,
}

impl CopyMetadata {
    pub fn new(
        turn_id: impl Into<String>,
        block_kind: BlockKind,
        timestamp: impl Into<String>,
    ) -> Self {
        Self {
            turn_id: turn_id.into(),
            block_kind,
            timestamp: timestamp.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyMetadataPolicy {
    pub include_turn_id: bool,
    pub include_block_kind: bool,
    pub include_timestamp: bool,
}

pub fn copy_with_metadata(
    text: &str,
    metadata: &CopyMetadata,
    policy: CopyMetadataPolicy,
) -> String {
    let mut fields = Vec::new();
    if policy.include_turn_id {
        fields.push(metadata.turn_id.clone());
    }
    if policy.include_block_kind {
        fields.push(metadata.block_kind.label().to_string());
    }
    if policy.include_timestamp {
        fields.push(metadata.timestamp.clone());
    }
    if fields.is_empty() {
        text.to_string()
    } else {
        format!("[{}]\n{text}", fields.join("] ["))
    }
}
