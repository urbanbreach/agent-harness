use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCompactionEvent {
    pub agent_id: String,
    pub summary: String,
    pub first_kept_event_seq: u64,
    pub first_kept_request_id: Option<String>,
    pub first_kept_entry_id: Option<crate::ids::EntryId>,
    pub tokens_before: u32,
    pub tokens_after: Option<u32>,
    #[serde(alias = "summary_generation_usage")]
    pub summary_usage: Option<harness_providers::CompletionUsage>,
    pub summary_provider_id: Option<String>,
    pub summary_model_id: Option<String>,
    #[serde(default)]
    pub read_files: Vec<String>,
    #[serde(default)]
    pub modified_files: Vec<String>,
    pub task_intent: Option<String>,
    pub current_intent: Option<super::UiIntentReceivedEvent>,
    pub trigger_reason: String,
    #[serde(default)]
    pub from_hook: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchSummaryEvent {
    pub agent_id: String,
    pub summary: String,
    pub from_event_seq: u64,
    #[serde(default)]
    pub read_files: Vec<String>,
    #[serde(default)]
    pub modified_files: Vec<String>,
    #[serde(default)]
    pub from_hook: bool,
}

// Older journals and the unchanged TUI still decode these records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionRequestedEvent {
    pub checkpoint_id: String,
    pub agent_id: String,
    pub trigger_reason: String,
    pub through_seq: u64,
    pub through_request_id: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub tokens_before: Option<u32>,
    pub tokens_before_estimate: Option<u32>,
    pub estimate_source: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionWrittenEvent {
    pub checkpoint_id: String,
    pub agent_id: String,
    pub artifact_path: String,
    pub artifact_digest: Option<String>,
    pub artifact_bytes: u64,
    pub trigger_reason: String,
    pub through_seq: u64,
    pub through_request_id: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub tokens_before: Option<u32>,
    pub tokens_before_estimate: Option<u32>,
    pub tokens_after_estimate: Option<u32>,
    pub summary_tokens_estimate: Option<u32>,
    pub compacted_turns: Option<u32>,
    pub reduction_tokens_estimate: Option<u32>,
    pub reduction_percent_estimate: Option<u32>,
    pub estimate_source: Option<String>,
    pub summary_source: Option<crate::agent::ProviderCompactionSummarySource>,
    pub preserved_turns: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionAppliedEvent {
    pub checkpoint_id: String,
    pub agent_id: String,
    pub through_seq: u64,
    pub through_request_id: Option<String>,
    pub tokens_before_estimate: Option<u32>,
    pub tokens_after_estimate: Option<u32>,
    pub summary_tokens_estimate: Option<u32>,
    pub compacted_turns: Option<u32>,
    pub preserved_turns: Option<u32>,
    pub reduction_tokens_estimate: Option<u32>,
    pub reduction_percent_estimate: Option<u32>,
    pub estimate_source: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionFailedEvent {
    pub agent_id: String,
    pub trigger_reason: String,
    pub reason: String,
    pub checkpoint_id: Option<String>,
    pub through_seq: Option<u64>,
    pub through_request_id: Option<String>,
}
