use super::*;
use crate::{attachment_transport::AttachmentMetadata, event::UiIntentReceivedEvent, ids::*};
use harness_providers::CompletionUsage;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionEntry {
    pub id: EntryId,
    pub parent_id: Option<EntryId>,
    pub turn_id: Option<TurnId>,
    pub run_id: RunId,
    pub payload: SessionEntryPayload,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionEntryPayload {
    UserMessage {
        text: String,
        attachments: Vec<AttachmentMetadata>,
    },
    AssistantMessage {
        parts: Vec<AssistantPart>,
        provenance: Option<Box<ProviderProvenance>>,
    },
    ToolResult {
        tool_call_id: ToolCallId,
        requesting_assistant_entry_id: EntryId,
        status: ToolResultStatus,
        output_summary: Option<String>,
        output_digest: Option<String>,
        output_json: Option<Value>,
    },
    ModelChange {
        provider_id: String,
        model_id: String,
    },
    ReasoningSettingChange {
        setting: String,
    },
    SystemContextUpdate {
        context: String,
    },
    CompactionSummary {
        summary: String,
        first_kept_entry_id: EntryId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tokens_after: Option<u32>,
        #[serde(
            default,
            alias = "summary_generation_usage",
            skip_serializing_if = "Option::is_none"
        )]
        summary_usage: Option<CompletionUsage>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary_provider_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary_model_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        preserved_state: Option<Box<CompactionPreservedState>>,
    },
    BranchSummary {
        summary: String,
    },
    CustomPersistedState {
        key: String,
        value: Value,
    },
    CustomModelVisibleContext {
        key: String,
        context: String,
    },
    SessionMetadata {
        title: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionPreservedState {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modified_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_intent: Option<UiIntentReceivedEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Copy)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultStatus {
    Succeeded,
    Failed,
}
