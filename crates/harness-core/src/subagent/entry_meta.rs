//! Per-entry facts a semantic finalized snapshot keeps so a restored child context prices,
//! cuts and replays like the context it was captured from.
use harness_providers::CompletionUsage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalizedStop {
    #[default]
    Complete,
    Truncated,
    Aborted,
    Error,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalizedOrigin {
    #[default]
    User,
    Agent,
    Host,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalizedNoteKind {
    Reminder,
    CompactionResume,
    LengthStop,
    ChildCompletion,
    Wake,
    Peer,
    Handoff,
    Hook,
    NotesWindow,
    Other,
}

/// One ordered block of an assistant message. Reasoning is never captured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FinalizedItemPart {
    Text { text: String },
    Call { index: usize },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FinalizedToolFacts {
    pub is_error: bool,
    pub useless: bool,
    pub pruned: bool,
    pub read_complete: Option<bool>,
    pub internal_artifact: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FinalizedItemMeta {
    pub at_ms: Option<u64>,
    pub stop: FinalizedStop,
    pub origin: FinalizedOrigin,
    pub note_kind: Option<FinalizedNoteKind>,
    pub usage: Option<CompletionUsage>,
    pub pruned_at_ms: Option<u64>,
    pub elided_output_tokens: u32,
    pub parts: Vec<FinalizedItemPart>,
    pub tool: FinalizedToolFacts,
    /// The compaction this entry stands for. A native payload is referenced, never stored.
    pub compaction: Option<crate::event::SessionCompactionEvent>,
    pub branch_summary: Option<crate::event::BranchSummaryEvent>,
}
