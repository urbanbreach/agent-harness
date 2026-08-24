use harness_providers::CompletionUsage;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::attachment_transport::AttachmentMetadata;
use crate::ids::{EntryId, ProviderRequestId, RunId, ToolCallId, TurnId};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionEntry {
    pub id: EntryId,
    pub parent_id: Option<EntryId>,
    pub turn_id: Option<TurnId>,
    pub run_id: RunId,
    pub payload: SessionEntryPayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionEntryPayload {
    UserMessage {
        text: String,
        attachments: Vec<AttachmentMetadata>,
    },
    AssistantMessage {
        parts: Vec<AssistantPart>,
        provenance: Option<ProviderProvenance>,
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
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AssistantPart {
    Text { text: String },
    Reasoning { text: String },
    ToolCall(AssistantToolCall),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantToolCall {
    pub tool_call_id: ToolCallId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_tool_call_id: Option<String>,
    pub tool_id: String,
    pub args_summary: String,
    pub args_digest: String,
    pub provider_call_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderProvenance {
    pub provider_id: String,
    pub model_id: String,
    pub request_id: ProviderRequestId,
    pub response_id: Option<String>,
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<CompletionUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultStatus {
    Succeeded,
    Failed,
}
