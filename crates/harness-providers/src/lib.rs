//! Provider abstraction layer for deterministic mocks and OpenAI-compatible
//! streaming backends.
//!
//! Keep transport/request normalization here so the coordinator and agent loop
//! can remain provider-agnostic.

use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio_stream::{self, Stream};

pub mod cassette;
pub mod mock;
pub mod openai;
pub mod schema_compat;

pub type ProviderId = String;
pub type ModelId = String;
pub type ProviderEventStream = Pin<Box<dyn Stream<Item = ProviderStreamEvent> + Send>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assistant_tool_calls: Option<Vec<AssistantToolCall>>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub parameters: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    Auto,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum CacheRetention {
    None,
    #[default]
    Short,
    Long,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderRequestInitiator {
    #[default]
    Agent,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRequestContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "ProviderRequestInitiator::is_default")]
    pub initiator: ProviderRequestInitiator,
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_media: bool,
    #[serde(default, skip_serializing_if = "CacheRetention::is_default")]
    pub cache_retention: CacheRetention,
}

impl Default for ProviderRequestContext {
    fn default() -> Self {
        Self {
            session_id: None,
            request_id: None,
            initiator: ProviderRequestInitiator::Agent,
            has_media: false,
            cache_retention: CacheRetention::Short,
        }
    }
}

impl ProviderRequestContext {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

impl ProviderRequestInitiator {
    fn is_default(value: &Self) -> bool {
        *value == Self::Agent
    }
}

impl CacheRetention {
    fn is_default(value: &Self) -> bool {
        *value == Self::Short
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompletionRequest {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub provider_id: Option<ProviderId>,
    pub model_id: ModelId,
    pub messages: Vec<CompletionMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_verbosity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(default, skip_serializing_if = "ProviderRequestContext::is_default")]
    pub context: ProviderRequestContext,
    pub stream: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderStreamStartMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_cache_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderStreamThinkingMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderStreamFinishedMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_response_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_cache_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ProviderStreamThinkingMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorCategory {
    MissingCredentials,
    InvalidCredentials,
    RateLimited,
    ContextWindowExceeded,
    UnsupportedToolCall,
    MalformedStream,
    TransportFailure,
    Other,
}

impl ProviderErrorCategory {
    pub const ALL: [Self; 8] = [
        Self::MissingCredentials,
        Self::InvalidCredentials,
        Self::RateLimited,
        Self::ContextWindowExceeded,
        Self::UnsupportedToolCall,
        Self::MalformedStream,
        Self::TransportFailure,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingCredentials => "missing_credentials",
            Self::InvalidCredentials => "invalid_credentials",
            Self::RateLimited => "rate_limited",
            Self::ContextWindowExceeded => "context_window_exceeded",
            Self::UnsupportedToolCall => "unsupported_tool_call",
            Self::MalformedStream => "malformed_stream",
            Self::TransportFailure => "transport_failure",
            Self::Other => "other",
        }
    }

    pub fn remediation(self) -> &'static str {
        match self {
            Self::MissingCredentials => {
                "Configure the provider API key or apiKeyEnv value, then retry."
            }
            Self::InvalidCredentials => {
                "Check that the provider credential is valid for the selected provider and model."
            }
            Self::RateLimited => {
                "Wait for the provider rate limit to reset or switch to a less constrained model/provider."
            }
            Self::ContextWindowExceeded => {
                "Reduce prompt context, enable compaction, or choose a model with a larger context window."
            }
            Self::UnsupportedToolCall => {
                "Inspect the tool schema and provider support matrix, then retry with a supported tool shape."
            }
            Self::MalformedStream => {
                "Retry the request; if it repeats, capture a support bundle because the provider stream was malformed."
            }
            Self::TransportFailure => {
                "Check provider base URL/network reachability and retry the request."
            }
            Self::Other => {
                "Inspect the provider message and support bundle for the provider-specific failure detail."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCredentialKind {
    StoredOauth,
    StoredApiKey,
    EnvApiKey,
    InlineApiKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderBearerToken {
    pub token: String,
    pub kind: ProviderCredentialKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enterprise_url: Option<String>,
}

#[derive(Debug, Clone, Error)]
#[error("{message}")]
pub struct ProviderCredentialError {
    pub category: ProviderErrorCategory,
    pub message: String,
}

impl ProviderCredentialError {
    pub fn new(category: ProviderErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }
}

#[async_trait]
pub trait ProviderCredentialSource: Send + Sync {
    async fn bearer_token(&self) -> Result<ProviderBearerToken, ProviderCredentialError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderStreamEvent {
    Start,
    Started {
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<ProviderStreamStartMetadata>,
    },
    TextDelta(String),
    ReasoningDelta(String),
    ToolCallDelta {
        tool_call_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        function_name: Option<String>,
        arguments_delta: String,
    },
    ToolCallComplete {
        tool_call_id: String,
        function_name: String,
        arguments_json: String,
    },
    Done {
        usage: CompletionUsage,
    },
    DoneWithMetadata {
        usage: CompletionUsage,
        #[serde(skip_serializing_if = "Option::is_none")]
        metadata: Option<ProviderStreamFinishedMetadata>,
    },
    Error {
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        category: Option<ProviderErrorCategory>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remediation: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after_ms: Option<u64>,
    },
}

impl ProviderStreamEvent {
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
            category: None,
            remediation: None,
            retry_after_ms: None,
        }
    }

    pub fn categorized_error(message: impl Into<String>, category: ProviderErrorCategory) -> Self {
        Self::categorized_error_with_retry_after_ms(message, category, None)
    }

    pub fn categorized_error_with_retry_after_ms(
        message: impl Into<String>,
        category: ProviderErrorCategory,
        retry_after_ms: Option<u64>,
    ) -> Self {
        let message = message.into();
        Self::Error {
            message: format!("{}: {message}", category.as_str()),
            category: Some(category),
            remediation: Some(category.remediation().to_string()),
            retry_after_ms,
        }
    }
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream;
}

pub struct ProviderRouter {
    providers: BTreeMap<ProviderId, Arc<dyn Provider>>,
}

impl ProviderRouter {
    pub fn new(providers: BTreeMap<ProviderId, Arc<dyn Provider>>) -> Self {
        Self { providers }
    }

    fn resolve_provider_id<'a>(
        &'a self,
        requested_provider_id: Option<&'a str>,
    ) -> Result<&'a str, String> {
        if let Some(provider_id) = requested_provider_id {
            return if self.providers.contains_key(provider_id) {
                Ok(provider_id)
            } else {
                Err(format!(
                    "unknown provider `{provider_id}` in completion request; configured providers: {}",
                    configured_provider_list(&self.providers)
                ))
            };
        }

        if self.providers.contains_key("default") {
            Ok("default")
        } else if self.providers.len() == 1 {
            Ok(self
                .providers
                .keys()
                .next()
                .expect("single-provider map should have a key"))
        } else {
            Err(format!(
                "completion request omitted provider_id and no default provider is configured; configured providers: {}",
                configured_provider_list(&self.providers)
            ))
        }
    }
}

#[async_trait]
impl Provider for ProviderRouter {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let provider_id = match self.resolve_provider_id(req.provider_id.as_deref()) {
            Ok(provider_id) => provider_id,
            Err(message) => {
                return Box::pin(tokio_stream::iter(vec![ProviderStreamEvent::error(
                    message,
                )]));
            }
        };

        self.providers
            .get(provider_id)
            .expect("resolved provider id should exist")
            .stream_completion(req)
            .await
    }
}

fn configured_provider_list(providers: &BTreeMap<ProviderId, Arc<dyn Provider>>) -> String {
    providers
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        AssistantToolCall, CompletionMessage, CompletionRequest, CompletionUsage, MessageRole,
        ProviderStreamEvent, ToolChoice, ToolDef,
    };

    #[test]
    fn completion_request_roundtrip_with_tools_is_stable() {
        let request = CompletionRequest {
            provider_id: None,
            model_id: "gpt-5.4-mini".to_string(),
            messages: vec![
                CompletionMessage {
                    role: MessageRole::Assistant,
                    content: "calling tool".to_string(),
                    name: Some("assistant-tool-router".to_string()),
                    tool_call_id: None,
                    assistant_tool_calls: Some(vec![AssistantToolCall {
                        tool_call_id: "call_1".to_string(),
                        function_name: "filesystem_read".to_string(),
                        arguments_json: "{\"filePath\":\"/tmp/demo.txt\"}".to_string(),
                    }]),
                },
                CompletionMessage {
                    role: MessageRole::Tool,
                    content: "{\"ok\":true}".to_string(),
                    name: Some("filesystem_read".to_string()),
                    tool_call_id: Some("call_1".to_string()),
                    assistant_tool_calls: None,
                },
            ],
            temperature: Some(0.0),
            max_tokens: Some(256),
            variant: Some("low".to_string()),
            reasoning_effort: Some("low".to_string()),
            text_verbosity: Some("low".to_string()),
            reasoning_summary: Some("auto".to_string()),
            thinking: None,
            tools: Some(vec![ToolDef {
                tool_id: "fs.read".to_string(),
                function_name: "filesystem_read".to_string(),
                description: Some("Read file content by absolute path".to_string()),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "filePath": {"type": "string"}
                    },
                    "required": ["filePath"],
                    "additionalProperties": false
                }),
            }]),
            tool_choice: Some(ToolChoice::Auto),
            context: Default::default(),
            stream: true,
        };

        let encoded = serde_json::to_string(&request).expect("serialize completion request");
        let decoded: CompletionRequest =
            serde_json::from_str(&encoded).expect("deserialize completion request");

        assert_eq!(decoded, request);
    }

    #[test]
    fn completion_request_omits_optional_tool_fields_when_absent() {
        let request = CompletionRequest {
            provider_id: None,
            model_id: "gpt-4o-mini".to_string(),
            messages: vec![CompletionMessage {
                role: MessageRole::User,
                content: "hello".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            }],
            temperature: None,
            max_tokens: None,
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            thinking: None,
            tools: None,
            tool_choice: None,
            context: Default::default(),
            stream: true,
        };

        let value = serde_json::to_value(&request).expect("serialize minimal request");
        let payload = value
            .as_object()
            .expect("completion request should serialize as object");
        assert!(!payload.contains_key("tools"));
        assert!(!payload.contains_key("tool_choice"));

        let message = payload["messages"]
            .as_array()
            .and_then(|messages| messages.first())
            .and_then(|message| message.as_object())
            .expect("first message should be object");
        assert!(!message.contains_key("name"));
        assert!(!message.contains_key("tool_call_id"));
        assert!(!message.contains_key("assistant_tool_calls"));
    }

    #[test]
    fn provider_stream_event_ordering_with_tool_calls_is_stable() {
        let events = vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::ToolCallDelta {
                tool_call_id: "call_1".to_string(),
                function_name: Some("filesystem_read".to_string()),
                arguments_delta: "{\"filePath\":".to_string(),
            },
            ProviderStreamEvent::ToolCallDelta {
                tool_call_id: "call_1".to_string(),
                function_name: None,
                arguments_delta: "\"/tmp/demo.txt\"}".to_string(),
            },
            ProviderStreamEvent::ToolCallComplete {
                tool_call_id: "call_1".to_string(),
                function_name: "filesystem_read".to_string(),
                arguments_json: "{\"filePath\":\"/tmp/demo.txt\"}".to_string(),
            },
            ProviderStreamEvent::TextDelta("done".to_string()),
            ProviderStreamEvent::Done {
                usage: CompletionUsage {
                    prompt_tokens: 10,
                    completion_tokens: 5,
                    total_tokens: 15,
                },
            },
        ];

        let encoded = serde_json::to_string(&events).expect("serialize stream events");
        let decoded: Vec<ProviderStreamEvent> =
            serde_json::from_str(&encoded).expect("deserialize stream events");

        assert_eq!(decoded, events);
    }
}
