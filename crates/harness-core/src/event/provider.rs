use harness_providers::{CompletionUsage, ProviderErrorCategory};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderRequestStartedMetadata {
    pub turn_id: Option<String>,
    pub provider_call_id: Option<String>,
    pub provider_session_id: Option<String>,
    pub provider_cache_id: Option<String>,
    pub retry: Option<ProviderRequestRetryMetadata>,
    pub context_budget: Option<crate::context_budget::RequestBudgetSnapshot>,
    pub runtime_selection: Option<Box<crate::session::CanonicalRuntimeSelection>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequestRetryMetadata {
    pub attempt: u32,
    pub max_attempts: u32,
    pub delay_ms: Option<u64>,
    pub category: Option<ProviderErrorCategory>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderAssistantMessageMetadata {
    pub message_id: Option<String>,
    pub text_digest: Option<String>,
    pub reasoning_digest: Option<String>,
}
pub type ProviderThinkingMetadata = harness_providers::ProviderStreamThinkingMetadata;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderRequestFinishedMetadata {
    pub turn_id: Option<String>,
    pub provider_call_id: Option<String>,
    pub provider_response_id: Option<String>,
    pub provider_session_id: Option<String>,
    pub provider_cache_id: Option<String>,
    pub provider_stop_reason: Option<String>,
    pub cache_read_tokens: Option<u32>,
    pub cache_write_tokens: Option<u32>,
    pub assistant_message: Option<ProviderAssistantMessageMetadata>,
    pub thinking: Option<ProviderThinkingMetadata>,
    pub provider_error_category: Option<ProviderErrorCategory>,
    pub provider_error_remediation: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequestStartedEvent {
    pub request_id: crate::ids::RequestId,
    pub provider_id: String,
    pub model_id: String,
    pub prompt_summary: String,
    pub request_digest: String,
    pub metadata: Option<ProviderRequestStartedMetadata>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStreamDeltaEvent {
    pub request_id: crate::ids::RequestId,
    pub delta: String,
}
pub type ProviderReasoningDeltaEvent = ProviderStreamDeltaEvent;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequestFinishedEvent {
    pub request_id: crate::ids::RequestId,
    pub finish_reason: String,
    pub output_digest: Option<String>,
    pub usage: Option<CompletionUsage>,
    pub metadata: Option<ProviderRequestFinishedMetadata>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantMessageFinishedEvent {
    pub request_id: crate::ids::RequestId,
    pub tool_call_count: usize,
    #[serde(default)]
    pub parts: Vec<crate::session::AssistantPart>,
    pub provenance: Option<crate::session::ProviderProvenance>,
    pub assistant_message: Option<ProviderAssistantMessageMetadata>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptAttachmentsSubmittedEvent {
    pub request_id: crate::ids::RequestId,
    pub attachments: Vec<crate::attachment_transport::AttachmentMetadata>,
}
