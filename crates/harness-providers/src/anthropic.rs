//! Anthropic Messages API transport.
//!
//! Implements request mapping from [`CompletionRequest`] to the Anthropic
//! Messages API format and SSE event parsing from Anthropic streaming
//! responses back to [`ProviderStreamEvent`].
//!
//! The Anthropic Messages API uses `x-api-key` authentication, a different
//! message format (content blocks), and SSE events (`message_start`,
//! `content_block_delta`, `message_delta`, `message_stop`).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, ProviderErrorCategory,
    ProviderStreamEvent, ProviderStreamFinishedMetadata, ToolChoice, ToolDef,
};

/// Anthropic API version header value.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Default Anthropic API base URL.
pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";

/// Build the Anthropic Messages API request body from a [`CompletionRequest`].
///
/// Maps the Harness-internal completion request to the Anthropic `/v1/messages`
/// JSON shape: extracts `system` from system messages, converts tool messages
/// to `tool_result` content blocks, and maps tool definitions to Anthropic format.
pub fn build_anthropic_request(req: &CompletionRequest) -> Value {
    let mut system: Option<String> = None;
    let mut anthropic_messages: Vec<Value> = Vec::with_capacity(req.messages.len());

    for msg in &req.messages {
        match msg.role {
            MessageRole::System => {
                system = Some(msg.content.clone());
            }
            MessageRole::User => {
                if let Some(tool_call_id) = &msg.tool_call_id {
                    anthropic_messages.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": tool_call_id,
                            "content": msg.content,
                        }]
                    }));
                } else {
                    anthropic_messages.push(json!({
                        "role": "user",
                        "content": msg.content,
                    }));
                }
            }
            MessageRole::Assistant => {
                let mut content: Vec<Value> = Vec::new();
                if !msg.content.is_empty() {
                    content.push(json!({"type": "text", "text": msg.content}));
                }
                if let Some(tool_calls) = &msg.assistant_tool_calls {
                    for tc in tool_calls {
                        content.push(json!({
                            "type": "tool_use",
                            "id": tc.tool_call_id,
                            "name": tc.function_name,
                            "input": serde_json::from_str::<Value>(&tc.arguments_json)
                                .unwrap_or(Value::Null),
                        }));
                    }
                }
                if content.is_empty() {
                    content.push(json!({"type": "text", "text": ""}));
                }
                anthropic_messages.push(json!({
                    "role": "assistant",
                    "content": content,
                }));
            }
            MessageRole::Tool => {
                if let Some(tool_call_id) = &msg.tool_call_id {
                    anthropic_messages.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": tool_call_id,
                            "content": msg.content,
                        }]
                    }));
                }
            }
        }
    }

    let mut body = json!({
        "model": req.model_id,
        "messages": anthropic_messages,
        "max_tokens": req.max_tokens.unwrap_or(4096),
    });

    if let Some(sys) = system {
        body["system"] = json!(sys);
    }
    if let Some(temp) = req.temperature {
        body["temperature"] = json!(temp);
    }
    if req.stream {
        body["stream"] = json!(true);
    }
    if let Some(tools) = &req.tools {
        body["tools"] = json!(tools
            .iter()
            .map(|t: &ToolDef| json!({
                "name": t.function_name,
                "description": t.description.as_deref().unwrap_or(""),
                "input_schema": t.parameters,
            }))
            .collect::<Vec<_>>());
        if let Some(choice) = req.tool_choice {
            match choice {
                ToolChoice::Auto => {
                    body["tool_choice"] = json!({"type": "auto"});
                }
                ToolChoice::None => {
                    body["tool_choice"] = json!({"type": "none"});
                }
            }
        }
    }

    body
}

/// Build the HTTP headers for an Anthropic API request.
pub fn build_anthropic_headers(api_key: &str) -> Vec<(&'static str, String)> {
    vec![
        ("x-api-key", api_key.to_string()),
        ("anthropic-version", ANTHROPIC_VERSION.to_string()),
        ("content-type", "application/json".to_string()),
    ]
}

/// Build the full Anthropic Messages API endpoint URL.
pub fn anthropic_messages_url(base_url: &str) -> String {
    format!("{base_url}/v1/messages")
}

/// Parsed Anthropic SSE event type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnthropicSseEvent {
    MessageStart {
        message_id: String,
        model: String,
    },
    ContentBlockStart {
        index: u32,
        block_type: AnthropicBlockType,
        block_id: Option<String>,
        name: Option<String>,
    },
    ContentBlockDelta {
        index: u32,
        delta: AnthropicDelta,
    },
    ContentBlockStop {
        index: u32,
    },
    MessageDelta {
        stop_reason: Option<String>,
    },
    MessageStop,
    Ping,
    Error {
        message: String,
    },
}

/// Type of content block in an Anthropic streaming response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnthropicBlockType {
    Text,
    ToolUse,
}

/// Delta content from an Anthropic SSE `content_block_delta` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnthropicDelta {
    TextDelta(String),
    InputJsonDelta(String),
}

/// Parse a single Anthropic SSE event line into a structured event.
///
/// Expects the `data:` payload (without the `data: ` prefix) as input.
pub fn parse_anthropic_sse_event(data: &str) -> Option<AnthropicSseEvent> {
    let value: Value = serde_json::from_str(data).ok()?;
    let event_type = value.get("type")?.as_str()?;
    match event_type {
        "message_start" => {
            let msg = value.get("message")?;
            let message_id = msg.get("id")?.as_str()?.to_string();
            let model = msg.get("model")?.as_str()?.to_string();
            Some(AnthropicSseEvent::MessageStart { message_id, model })
        }
        "content_block_start" => {
            let index = u32::try_from(value.get("index")?.as_u64()?).ok()?;
            let block = value.get("content_block")?;
            let block_type_str = block.get("type")?.as_str()?;
            let block_type = match block_type_str {
                "text" => AnthropicBlockType::Text,
                "tool_use" => AnthropicBlockType::ToolUse,
                _ => return None,
            };
            let block_id = block.get("id").and_then(|v| v.as_str()).map(String::from);
            let name = block.get("name").and_then(|v| v.as_str()).map(String::from);
            Some(AnthropicSseEvent::ContentBlockStart {
                index,
                block_type,
                block_id,
                name,
            })
        }
        "content_block_delta" => {
            let index = u32::try_from(value.get("index")?.as_u64()?).ok()?;
            let delta = value.get("delta")?;
            let delta_type = delta.get("type")?.as_str()?;
            let delta_val = match delta_type {
                "text_delta" => AnthropicDelta::TextDelta(delta.get("text")?.as_str()?.to_string()),
                "input_json_delta" => {
                    AnthropicDelta::InputJsonDelta(delta.get("partial_json")?.as_str()?.to_string())
                }
                _ => return None,
            };
            Some(AnthropicSseEvent::ContentBlockDelta {
                index,
                delta: delta_val,
            })
        }
        "content_block_stop" => {
            let index = u32::try_from(value.get("index")?.as_u64()?).ok()?;
            Some(AnthropicSseEvent::ContentBlockStop { index })
        }
        "message_delta" => {
            let delta = value.get("delta")?;
            let stop_reason = delta
                .get("stop_reason")
                .and_then(|v| v.as_str())
                .map(String::from);
            Some(AnthropicSseEvent::MessageDelta { stop_reason })
        }
        "message_stop" => Some(AnthropicSseEvent::MessageStop),
        "ping" => Some(AnthropicSseEvent::Ping),
        "error" => {
            let error = value.get("error")?;
            let message = error
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error")
                .to_string();
            Some(AnthropicSseEvent::Error { message })
        }
        _ => None,
    }
}

/// Convert an Anthropic SSE event into zero or more [`ProviderStreamEvent`]s.
///
/// Maintains internal state for tool-call assembly (block index → tool_call_id,
/// function_name, accumulated arguments JSON).
pub fn anthropic_sse_to_provider_event(
    event: &AnthropicSseEvent,
    tool_state: &mut Vec<(u32, String, String, String)>,
) -> Vec<ProviderStreamEvent> {
    match event {
        AnthropicSseEvent::MessageStart { .. } => {
            vec![ProviderStreamEvent::Started { metadata: None }]
        }
        AnthropicSseEvent::ContentBlockStart {
            index,
            block_type,
            block_id,
            name,
        } => {
            if *block_type == AnthropicBlockType::ToolUse {
                let id = block_id.clone().unwrap_or_default();
                let func = name.clone().unwrap_or_default();
                tool_state.push((*index, id.clone(), func.clone(), String::new()));
                vec![ProviderStreamEvent::ToolCallDelta {
                    tool_call_id: id,
                    function_name: Some(func),
                    arguments_delta: String::new(),
                }]
            } else {
                vec![]
            }
        }
        AnthropicSseEvent::ContentBlockDelta { index, delta } => match delta {
            AnthropicDelta::TextDelta(text) => {
                vec![ProviderStreamEvent::TextDelta(text.clone())]
            }
            AnthropicDelta::InputJsonDelta(partial) => {
                if let Some(entry) = tool_state.iter_mut().find(|(idx, _, _, _)| idx == index) {
                    entry.3.push_str(partial);
                }
                vec![]
            }
        },
        AnthropicSseEvent::ContentBlockStop { index } => {
            if let Some(pos) = tool_state.iter().position(|(idx, _, _, _)| idx == index) {
                let (_, id, func, args) = &tool_state[pos];
                if !args.is_empty() {
                    let event = ProviderStreamEvent::ToolCallComplete {
                        tool_call_id: id.clone(),
                        function_name: func.clone(),
                        arguments_json: args.clone(),
                    };
                    tool_state.remove(pos);
                    return vec![event];
                }
            }
            vec![]
        }
        AnthropicSseEvent::MessageDelta { stop_reason } => {
            let _ = stop_reason;
            vec![]
        }
        AnthropicSseEvent::MessageStop => {
            vec![ProviderStreamEvent::DoneWithMetadata {
                usage: None,
                metadata: Some(ProviderStreamFinishedMetadata {
                    provider_stop_reason: None,
                    ..Default::default()
                }),
            }]
        }
        AnthropicSseEvent::Ping => vec![],
        AnthropicSseEvent::Error { message } => {
            vec![ProviderStreamEvent::categorized_error(
                message.clone(),
                ProviderErrorCategory::TransportFailure,
            )]
        }
    }
}

/// Parse a complete Anthropic SSE stream (multiple `data:` lines) into
/// [`ProviderStreamEvent`]s.
pub fn parse_anthropic_sse_stream(raw: &str) -> Vec<ProviderStreamEvent> {
    let mut tool_state: Vec<(u32, String, String, String)> = Vec::new();
    let mut events = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("data: ") {
            continue;
        }
        let data = &trimmed[6..];
        if data == "[DONE]" {
            break;
        }
        if let Some(event) = parse_anthropic_sse_event(data) {
            events.extend(anthropic_sse_to_provider_event(&event, &mut tool_state));
        }
    }
    events
}

/// Parse a non-streaming Anthropic Messages API response into
/// [`ProviderStreamEvent`]s.
pub fn parse_anthropic_response(body: &str) -> Vec<ProviderStreamEvent> {
    let value: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(err) => {
            return vec![ProviderStreamEvent::categorized_error(
                format!("failed to parse Anthropic response: {err}"),
                ProviderErrorCategory::MalformedStream,
            )];
        }
    };

    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown Anthropic error");
        return vec![ProviderStreamEvent::categorized_error(
            message.to_string(),
            ProviderErrorCategory::TransportFailure,
        )];
    }

    let mut events = vec![ProviderStreamEvent::Started { metadata: None }];

    if let Some(content) = value.get("content").and_then(|v| v.as_array()) {
        for block in content {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match block_type {
                "text" => {
                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        events.push(ProviderStreamEvent::TextDelta(text.to_string()));
                    }
                }
                "tool_use" => {
                    let id = block.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    let name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let input = block.get("input").cloned().unwrap_or(Value::Null);
                    events.push(ProviderStreamEvent::ToolCallComplete {
                        tool_call_id: id.to_string(),
                        function_name: name.to_string(),
                        arguments_json: input.to_string(),
                    });
                }
                _ => {}
            }
        }
    }

    let usage = value.get("usage").map(|u| CompletionUsage {
        prompt_tokens: u
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| u32::try_from(v).unwrap_or(u32::MAX))
            .unwrap_or(0),
        completion_tokens: u
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| u32::try_from(v).unwrap_or(u32::MAX))
            .unwrap_or(0),
        total_tokens: u
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| u32::try_from(v).unwrap_or(u32::MAX))
            .unwrap_or(0)
            + u.get("output_tokens")
                .and_then(|v| v.as_u64())
                .map(|v| u32::try_from(v).unwrap_or(u32::MAX))
                .unwrap_or(0),
    });

    let stop_reason = value
        .get("stop_reason")
        .and_then(|v| v.as_str())
        .map(String::from);

    events.push(ProviderStreamEvent::DoneWithMetadata {
        usage,
        metadata: Some(ProviderStreamFinishedMetadata {
            provider_stop_reason: stop_reason,
            ..Default::default()
        }),
    });

    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssistantToolCall, CompletionMessage, MessageRole, ToolDef};

    // arrange
    fn basic_request() -> CompletionRequest {
        CompletionRequest {
            provider_id: None,
            model_id: "claude-sonnet-4-20250514".to_string(),
            messages: vec![CompletionMessage {
                role: MessageRole::User,
                content: "Hello".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            }],
            temperature: Some(0.7),
            max_tokens: Some(1024),
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            thinking: None,
            tools: None,
            tool_choice: None,
            context: Default::default(),
            stream: true,
        }
    }

    #[test]
    fn build_request_maps_user_message_to_anthropic_format() {
        // arrange
        let req = basic_request();

        // act
        let body = build_anthropic_request(&req);

        // assert
        assert_eq!(body["model"], "claude-sonnet-4-20250514");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "Hello");
        assert_eq!(body["max_tokens"], 1024);
        let temp = body["temperature"].as_f64().expect("temperature");
        assert!((temp - 0.7).abs() < 1e-5, "temperature: {temp}");
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn build_request_extracts_system_message() {
        // arrange
        let req = CompletionRequest {
            messages: vec![
                CompletionMessage {
                    role: MessageRole::System,
                    content: "You are helpful".to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
                CompletionMessage {
                    role: MessageRole::User,
                    content: "Hi".to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
            ],
            ..basic_request()
        };

        // act
        let body = build_anthropic_request(&req);

        // assert
        assert_eq!(body["system"], "You are helpful");
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn build_request_maps_tool_definitions() {
        // arrange
        let req = CompletionRequest {
            tools: Some(vec![ToolDef {
                tool_id: "t1".to_string(),
                function_name: "read_file".to_string(),
                description: Some("Read a file".to_string()),
                parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            }]),
            tool_choice: Some(ToolChoice::Auto),
            ..basic_request()
        };

        // act
        let body = build_anthropic_request(&req);

        // assert
        assert_eq!(body["tools"][0]["name"], "read_file");
        assert_eq!(body["tools"][0]["description"], "Read a file");
        assert!(body["tools"][0]["input_schema"]["properties"].is_object());
        assert_eq!(body["tool_choice"]["type"], "auto");
    }

    #[test]
    fn build_request_maps_assistant_tool_calls_to_content_blocks() {
        // arrange
        let req = CompletionRequest {
            messages: vec![
                CompletionMessage {
                    role: MessageRole::User,
                    content: "Read the file".to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
                CompletionMessage {
                    role: MessageRole::Assistant,
                    content: "Let me read that".to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: Some(vec![AssistantToolCall {
                        tool_call_id: "call_1".to_string(),
                        function_name: "read_file".to_string(),
                        arguments_json: r#"{"path":"/tmp/test.txt"}"#.to_string(),
                    }]),
                },
                CompletionMessage {
                    role: MessageRole::Tool,
                    content: "file contents".to_string(),
                    name: Some("read_file".to_string()),
                    tool_call_id: Some("call_1".to_string()),
                    assistant_tool_calls: None,
                },
            ],
            ..basic_request()
        };

        // act
        let body = build_anthropic_request(&req);

        // assert
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[1]["role"], "assistant");
        let content = messages[1]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "tool_use");
        assert_eq!(content[1]["name"], "read_file");
        assert_eq!(content[1]["input"]["path"], "/tmp/test.txt");
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"][0]["type"], "tool_result");
        assert_eq!(messages[2]["content"][0]["tool_use_id"], "call_1");
    }

    #[test]
    fn build_headers_include_api_key_and_version() {
        // arrange
        // act
        let headers = build_anthropic_headers("sk-test-key");

        // assert
        assert_eq!(headers[0].0, "x-api-key");
        assert_eq!(headers[0].1, "sk-test-key");
        assert_eq!(headers[1].0, "anthropic-version");
        assert_eq!(headers[1].1, ANTHROPIC_VERSION);
        assert_eq!(headers[2].0, "content-type");
    }

    #[test]
    fn build_url_appends_messages_endpoint() {
        // arrange
        // act
        let url = anthropic_messages_url(ANTHROPIC_BASE_URL);

        // assert
        assert_eq!(url, "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn parse_sse_message_start_event() {
        // arrange
        let data = r#"{"type":"message_start","message":{"id":"msg_1","model":"claude-sonnet-4-20250514"}}"#;

        // act
        let event = parse_anthropic_sse_event(data);

        // assert
        assert_eq!(
            event,
            Some(AnthropicSseEvent::MessageStart {
                message_id: "msg_1".to_string(),
                model: "claude-sonnet-4-20250514".to_string(),
            })
        );
    }

    #[test]
    fn parse_sse_text_delta_event() {
        // arrange
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#;

        // act
        let event = parse_anthropic_sse_event(data);

        // assert
        assert_eq!(
            event,
            Some(AnthropicSseEvent::ContentBlockDelta {
                index: 0,
                delta: AnthropicDelta::TextDelta("Hello".to_string()),
            })
        );
    }

    #[test]
    fn parse_sse_tool_use_start_event() {
        // arrange
        let data = r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_1","name":"read_file"}}"#;

        // act
        let event = parse_anthropic_sse_event(data);

        // assert
        assert_eq!(
            event,
            Some(AnthropicSseEvent::ContentBlockStart {
                index: 1,
                block_type: AnthropicBlockType::ToolUse,
                block_id: Some("toolu_1".to_string()),
                name: Some("read_file".to_string()),
            })
        );
    }

    #[test]
    fn parse_sse_message_stop_event() {
        // arrange
        let data = r#"{"type":"message_stop"}"#;

        // act
        let event = parse_anthropic_sse_event(data);

        // assert
        assert_eq!(event, Some(AnthropicSseEvent::MessageStop));
    }

    #[test]
    fn parse_sse_stream_produces_provider_events() {
        // arrange
        let raw = "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude\"}}\n\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\"}}\n\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello world\"}}\n\ndata: {\"type\":\"content_block_stop\",\"index\":0}\n\ndata: {\"type\":\"message_stop\"}\n\n";

        // act
        let events = parse_anthropic_sse_stream(raw);

        // assert
        assert!(events.len() >= 3);
        assert!(matches!(events[0], ProviderStreamEvent::Started { .. }));
        assert!(matches!(
            events[1],
            ProviderStreamEvent::TextDelta(ref t) if t == "Hello world"
        ));
        assert!(matches!(
            events.last(),
            Some(ProviderStreamEvent::DoneWithMetadata { .. })
        ));
    }

    #[test]
    fn parse_sse_stream_assembles_tool_call_from_deltas() {
        // arrange
        let raw = "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"read_file\"}}\n\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\"}}\n\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"/tmp/test.txt\\\"}\"}}\n\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\ndata: {\"type\":\"message_stop\"}\n\n";

        // act
        let events = parse_anthropic_sse_stream(raw);

        // assert
        let tool_complete = events.iter().find(|e| {
            matches!(e, ProviderStreamEvent::ToolCallComplete { function_name, .. } if function_name == "read_file")
        });
        assert!(
            tool_complete.is_some(),
            "expected ToolCallComplete for read_file, got: {:?}",
            events
        );
        if let Some(ProviderStreamEvent::ToolCallComplete { arguments_json, .. }) = tool_complete {
            assert!(arguments_json.contains("/tmp/test.txt"));
        }
    }

    #[test]
    fn parse_non_streaming_response_with_text() {
        // arrange
        let body = r#"{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"text","text":"Hello!"}],"model":"claude-sonnet-4-20250514","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":5}}"#;

        // act
        let events = parse_anthropic_response(body);

        // assert
        assert!(events.len() >= 3);
        assert!(matches!(events[0], ProviderStreamEvent::Started { .. }));
        assert!(matches!(
            events[1],
            ProviderStreamEvent::TextDelta(ref t) if t == "Hello!"
        ));
        if let Some(ProviderStreamEvent::DoneWithMetadata { usage, metadata }) = events.last() {
            assert!(usage.is_some());
            assert_eq!(
                metadata
                    .as_ref()
                    .and_then(|m| m.provider_stop_reason.as_deref()),
                Some("end_turn")
            );
        }
    }

    #[test]
    fn parse_non_streaming_response_with_tool_use() {
        // arrange
        let body = r#"{"id":"msg_1","type":"message","role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"read_file","input":{"path":"/tmp/test.txt"}}],"model":"claude-sonnet-4-20250514","stop_reason":"tool_use","usage":{"input_tokens":10,"output_tokens":5}}"#;

        // act
        let events = parse_anthropic_response(body);

        // assert
        let tool_complete = events.iter().find(|e| {
            matches!(e, ProviderStreamEvent::ToolCallComplete { function_name, .. } if function_name == "read_file")
        });
        assert!(tool_complete.is_some());
        if let Some(ProviderStreamEvent::ToolCallComplete { arguments_json, .. }) = tool_complete {
            assert!(arguments_json.contains("/tmp/test.txt"));
        }
    }

    #[test]
    fn parse_error_response_returns_categorized_error() {
        // arrange
        let body = r#"{"error":{"type":"overloaded_error","message":"Overloaded"}}"#;

        // act
        let events = parse_anthropic_response(body);

        // assert
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], ProviderStreamEvent::Error { .. }));
    }

    #[test]
    fn parse_sse_error_event_returns_transport_failure() {
        // arrange
        let raw = "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n";

        // act
        let events = parse_anthropic_sse_stream(raw);

        // assert
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], ProviderStreamEvent::Error { .. }));
    }
}
