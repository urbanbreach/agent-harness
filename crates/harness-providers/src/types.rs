use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type ProviderId = String;
pub type ModelId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionMessage {
    pub role: MessageRole,
    pub content: String,
    pub name: Option<String>,
    pub tool_call_id: Option<String>,
    pub assistant_tool_calls: Option<Vec<AssistantToolCall>>,
}

impl CompletionMessage {
    pub fn text(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantToolCall {
    pub tool_call_id: String,
    pub function_name: String,
    pub arguments_json: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDef {
    pub tool_id: String,
    pub function_name: String,
    pub description: Option<String>,
    pub parameters: Value,
}
/// Stable names fit all supported provider protocols, including historical calls after a tool disappears.
pub fn tool_function_name(id: &str) -> String {
    if !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    {
        return id.into();
    }
    let prefix: String = id
        .chars()
        .take(47)
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{prefix}_{}", &blake3::hash(id.as_bytes()).to_hex()[..16])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    Auto,
    None,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CacheRetention {
    None,
    #[default]
    Short,
    Long,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRequestInitiator {
    #[default]
    Agent,
    User,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderRequestContext {
    pub session_id: Option<String>,
    pub request_id: Option<String>,
    pub initiator: ProviderRequestInitiator,
    pub has_media: bool,
    pub cache_retention: CacheRetention,
    /// A coordinator main turn, not an auxiliary request (compaction, evaluation).
    pub main_turn: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompletionRequest {
    pub provider_id: Option<ProviderId>,
    pub model_id: ModelId,
    pub messages: Vec<CompletionMessage>,
    /// Attachment payloads are transient; serde records only their metadata.
    pub attachments:
        std::collections::BTreeMap<usize, Vec<crate::attachment_protocol::AttachmentMetadata>>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub variant: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub reasoning_summary: Option<String>,
    pub thinking: Option<Value>,
    pub tools: Option<Vec<ToolDef>>,
    pub tool_choice: Option<ToolChoice>,
    pub context: ProviderRequestContext,
    pub stream: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStreamStartMetadata {
    pub provider_session_id: Option<String>,
    pub provider_cache_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStreamThinkingMetadata {
    pub summary: Option<String>,
    pub summary_digest: Option<String>,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderStreamFinishedMetadata {
    pub provider_response_id: Option<String>,
    pub provider_session_id: Option<String>,
    pub provider_cache_id: Option<String>,
    pub provider_stop_reason: Option<String>,
    pub cache_read_tokens: Option<u32>,
    pub cache_write_tokens: Option<u32>,
    pub assistant_message_id: Option<String>,
    pub thinking: Option<ProviderStreamThinkingMetadata>,
    /// Complete settled normalized reasoning, when the transport can supply it.
    /// None means unavailable, not an empty reasoning response or raw frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settled_reasoning: Option<Vec<String>>,
    /// False when a normalized protocol had to fill a missing usage count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_complete: Option<bool>,
    /// What a pooled, session-keeping backend reports beyond the response itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_report: Option<Box<ProviderSessionReport>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderSessionReport {
    /// The pooled account that served the request, labeled for display, when the pool has several.
    pub account: Option<String>,
    /// Compactions the backend ran on its own session during the request.
    pub native_compactions: Vec<ProviderNativeCompaction>,
}

/// A backend-run compaction of its own session (Claude Code's `compact_boundary`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderNativeCompaction {
    pub provider_session_id: String,
    pub boundary_id: String,
    pub trigger: String,
    pub pre_tokens: Option<u64>,
    pub post_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderStreamEvent {
    Start,
    Started {
        metadata: Option<ProviderStreamStartMetadata>,
    },
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallDelta {
        tool_call_id: String,
        function_name: Option<String>,
        arguments_delta: String,
    },
    ToolCallComplete {
        tool_call_id: String,
        function_name: String,
        arguments_json: String,
    },
    Done {
        usage: Option<CompletionUsage>,
    },
    DoneWithMetadata {
        usage: Option<CompletionUsage>,
        metadata: Option<ProviderStreamFinishedMetadata>,
    },
    Error {
        message: String,
        category: Option<crate::ProviderErrorCategory>,
        remediation: Option<String>,
        retry_after_ms: Option<u64>,
    },
    /// A user-visible note about the request, such as an account switch.
    Notice(String),
    /// The request was aborted and the backend settled; usage covers what it billed.
    Aborted {
        usage: Option<CompletionUsage>,
    },
}

/// Session lifecycle facts the coordinator owns, for providers that keep per-session state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderSessionEvent {
    /// A main request for the session is about to go to this provider and model.
    Routed {
        session_id: String,
        provider_id: ProviderId,
        model_id: ModelId,
    },
    /// The user picked this provider and model for the session's next turns.
    ModelSelected {
        session_id: String,
        provider_id: ProviderId,
        model_id: ModelId,
    },
    /// The user changed the reasoning level for the session's next turns.
    ReasoningSelected { session_id: String },
    /// The session's history was replaced by a compaction summary.
    Compacted { session_id: String },
    /// The session's history was rewound to an earlier point.
    Rewound { session_id: String },
    /// The session stopped; no further requests follow.
    Closed { session_id: String, reason: String },
}
