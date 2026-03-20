//! Provider abstraction layer for deterministic mocks and OpenAI-compatible
//! streaming backends.
//!
//! Keep transport/request normalization here so the coordinator and agent loop
//! can remain provider-agnostic.

use std::pin::Pin;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_stream::Stream;

pub mod mock;
pub mod openai;

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub model_id: ModelId,
    pub messages: Vec<CompletionMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    pub stream: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderStreamEvent {
    Start,
    TextDelta(String),
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
    Error {
        message: String,
    },
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream;
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
            model_id: "gpt-5.3-codex".to_string(),
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
            tools: None,
            tool_choice: None,
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
