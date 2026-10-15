use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod compaction;
mod continuation;
pub use compaction::{CompactionCredential, NativeLaneSettings, RemoteModelOverride};
pub use continuation::{
    ContinuationExport, ProviderContinuation, COMPACTION_PAYLOAD_BYTE_LIMIT,
    CONTINUATION_BYTE_LIMIT,
};

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
    #[serde(skip)]
    pub continuation: Option<Box<ProviderContinuation>>,
    pub role: MessageRole,
    pub content: String,
    pub name: Option<String>,
    pub tool_call_id: Option<String>,
    pub assistant_tool_calls: Option<Vec<AssistantToolCall>>,
}

impl CompletionMessage {
    pub fn text(role: MessageRole, content: impl Into<String>) -> Self {
        Self {
            continuation: None,
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
    /// Actor workspace for local subprocess providers; never part of persisted context.
    #[serde(skip)]
    pub working_directory: Option<std::path::PathBuf>,
    pub session_id: Option<String>,
    pub request_id: Option<String>,
    pub initiator: ProviderRequestInitiator,
    pub has_media: bool,
    pub cache_retention: CacheRetention,
    /// A coordinator main turn, not an auxiliary request (compaction, evaluation).
    pub main_turn: bool,
    /// Prompt-cache identity when it differs from `session_id`; side requests keep the
    /// main session's key while routing under their own session id.
    pub cache_key: Option<String>,
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
    /// Request-only ordered text and attachment slots, replacing the message body.
    #[serde(skip)]
    pub message_content: std::collections::BTreeMap<usize, Vec<MessageContentPart>>,
    /// Request-only wall-clock time of the event behind each message, parallel to `messages`;
    /// `None` where unknown. Empty when the caller has no times. Signed-summary file metadata
    /// is scheduled against these.
    #[serde(skip)]
    pub message_times_ms: Vec<Option<u64>>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageContentPart {
    Text(String),
    /// Index into this message's attachment list.
    Attachment(usize),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompletionUsage {
    /// Every prompt token, including cache reads and writes, on every protocol.
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    /// Uncached input tokens as the provider reports them separately.
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_write_tokens: u32,
    /// Context occupancy the provider reported explicitly; an explicit zero is kept.
    pub context_tokens: Option<u32>,
    /// Reasoning tokens when the provider reports them as a separate number.
    pub reasoning_tokens: Option<u32>,
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
    #[serde(skip)]
    pub continuation: Option<Box<ProviderContinuation>>,
    pub provider_response_id: Option<String>,
    pub provider_session_id: Option<String>,
    pub provider_cache_id: Option<String>,
    pub provider_stop_reason: Option<String>,
    /// The provider exhausted its output budget before completing the response.
    #[serde(default)]
    pub output_limit_reached: bool,
    pub cache_read_tokens: Option<u32>,
    pub cache_write_tokens: Option<u32>,
    /// The part of `cache_write_tokens` cached for one hour instead of five minutes. A plain
    /// count fits the padding, keeping `ProviderStreamEvent` within its size budget.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cache_write_1h_tokens: u32,
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

fn is_zero(count: &u32) -> bool {
    *count == 0
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

#[allow(
    clippy::large_enum_variant,
    reason = "the finished-metadata variant is built once per response; boxing it would touch every stream decoder and consumer for no measurable gain"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderStreamEvent {
    Start,
    Started {
        metadata: Option<ProviderStreamStartMetadata>,
    },
    TextDelta(String),
    /// A text content block completed; the next `TextDelta` starts a new block. Emitted where
    /// the wire separates blocks (Anthropic, Responses); consumers that do not care ignore it.
    TextBlockEnd,
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
        /// HTTP status, response headers and a bounded redacted body, when the failure had them.
        envelope: Option<Box<crate::ProviderErrorEnvelope>>,
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
    Compacted {
        session_id: String,
        descriptor: Option<CompactionDescriptor>,
    },
    /// The session's history was rewound to an earlier point.
    Rewound { session_id: String },
    /// The session stopped; no further requests follow.
    Closed { session_id: String, reason: String },
}

/// Which compaction pass replaced a session's history, for providers that track it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionDescriptor {
    pub operation_id: String,
    pub trigger: String,
    pub reason: String,
    pub phase: String,
}
