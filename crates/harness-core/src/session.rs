//! Settled session content and provider selection recorded in the journal.
use serde::{Deserialize, Serialize};
mod entry;
pub mod legacy;
mod model;
mod projection;
pub use entry::*;
pub use legacy::{
    canonical_provider_fragment_for_event, canonical_provider_fragment_payload,
    CanonicalLegacyCompaction, CanonicalLegacyCompactionStatus, CanonicalProviderFragment,
    CanonicalProviderFragmentKind,
};
pub use model::CanonicalSession;
pub use projection::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AssistantPart {
    Text { text: String },
    Reasoning { text: String },
    ToolCall(AssistantToolCall),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantToolCall {
    pub tool_call_id: crate::ids::ToolCallId,
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
    pub request_id: crate::ids::ProviderRequestId,
    pub response_id: Option<String>,
    pub stop_reason: Option<String>,
    pub usage: Option<harness_providers::CompletionUsage>,
    pub runtime_selection: Option<Box<CanonicalRuntimeSelection>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalRuntimeSelection {
    pub profile: Option<String>,
    pub provider_id: String,
    pub model_id: String,
    pub variant: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub reasoning_summary: Option<String>,
    pub thinking: Option<serde_json::Value>,
    pub resolved_limits: crate::config::ResolvedModelLimits,
    pub profile_tool_shape_digest: String,
}
