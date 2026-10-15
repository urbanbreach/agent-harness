use super::compaction::{lenient_method, CompactionMethod};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecyclePhase {
    Start,
    End,
}

/// What a finished compaction pass produced, when it produced anything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompactionEndResult {
    #[serde(deserialize_with = "lenient_method")]
    pub method: Option<CompactionMethod>,
    pub tokens_before: Option<u32>,
    pub tokens_after: Option<u32>,
    pub short_summary: Option<String>,
    pub warning: Option<String>,
    pub first_kept_seq: Option<u64>,
}

/// Start and end of one compaction pass, live only. The persisted record is the
/// `SessionCompaction` event that shares the pass's `operation_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionLifecycleEvent {
    pub agent_id: String,
    pub operation_id: String,
    pub phase: LifecyclePhase,
    pub reason: String,
    /// `context-full`, `handoff`, `shake`, `idle` or `new-context`.
    pub action: String,
    #[serde(default)]
    pub will_retry: bool,
    #[serde(default)]
    pub aborted: bool,
    #[serde(default)]
    pub skipped: bool,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub result: Option<CompactionEndResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeSource {
    Compaction,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeSeverity {
    Info,
    Warning,
    Error,
}

/// Where an agent's background summary stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeculationState {
    Idle,
    Armed,
    Running,
}

/// What the footer shows about automatic compaction, published whenever any field changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionStatusEvent {
    pub agent_id: String,
    /// Automatic compaction is enabled for the agent's active model.
    pub auto: bool,
    /// Resolved trigger in tokens for the active model; `None` when disabled or the window
    /// is unknown.
    pub threshold_tokens: Option<u64>,
    /// Threshold minus the permitted speculation lead; `None` for local-first methods,
    /// async disabled, or no threshold.
    pub speculation_tokens: Option<u64>,
    pub speculation: SpeculationState,
}
