use crate::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;
use tokio_stream::{self as stream, StreamExt};

use crate::{
    CompletionRequest, CompletionUsage, Provider, ProviderEventStream, ProviderRequestContext,
    ProviderStreamEvent, ProviderStreamFinishedMetadata, ProviderStreamStartMetadata, ToolChoice,
    ToolDef,
};

#[derive(Debug, Error)]
pub enum MockProviderError {
    #[error("failed to read fixture directory {path}: {source}")]
    ReadFixtureDir {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read fixture file {path}: {source}")]
    ReadFixtureFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse fixture JSON {path}: {source}")]
    ParseFixture {
        path: String,
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Debug, Default, Clone)]
pub struct MockProvider {
    scripted_events: BTreeMap<String, Vec<ProviderStreamEvent>>,
    fixture_path: Option<String>,
}

impl MockProvider {
    pub fn new(scripted_events: BTreeMap<String, Vec<ProviderStreamEvent>>) -> Self {
        Self {
            scripted_events,
            fixture_path: None,
        }
    }

    pub fn from_fixture_dir(path: impl AsRef<Path>) -> Result<Self, MockProviderError> {
        let path = path.as_ref();
        let path_string = path.display().to_string();
        let mut entries = fs::read_dir(path)
            .map_err(|source| MockProviderError::ReadFixtureDir {
                path: path_string.clone(),
                source,
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| MockProviderError::ReadFixtureDir {
                path: path_string.clone(),
                source,
            })?;

        entries.sort_by_key(|entry| entry.path());

        let mut scripted_events = BTreeMap::new();
        for entry in entries {
            let entry_path = entry.path();
            if entry_path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }

            let body = fs::read_to_string(&entry_path).map_err(|source| {
                MockProviderError::ReadFixtureFile {
                    path: entry_path.display().to_string(),
                    source,
                }
            })?;
            let fixture: MockProviderFixture =
                serde_json::from_str(&body).map_err(|source| MockProviderError::ParseFixture {
                    path: entry_path.display().to_string(),
                    source,
                })?;

            let request: CompletionRequest = fixture.request.into();
            let digest = request_digest(&request);
            scripted_events.insert(digest, fixture.events.into_iter().map(Into::into).collect());
        }

        Ok(Self {
            scripted_events,
            fixture_path: Some(path_string),
        })
    }

    fn format_missing_fixture_message(&self, digest: &str) -> String {
        let fixture_hint = match &self.fixture_path {
            Some(fixture_path) => format!(
                "; fixture_path={fixture_path}; add a fixture JSON whose normalized request hashes to this digest"
            ),
            None => {
                "; add a fixture JSON whose normalized request hashes to this digest".to_string()
            }
        };
        format!(
            "mock fixture missing for request_digest={digest}{fixture_hint}\n\
             Suggested commands:\n\
             \x20 1. Run the deterministic golden path scenario: `harness run --scenario golden_path --deterministic`\n\
             \x20 2. Record a fixture for your prompt: `harness run --mock \"your prompt\" --record-fixture`\n\
             \x20 3. Or set MOCK_FIXTURE_RECORD=<output-dir> to capture the request shape on the next run"
        )
    }
}

#[async_trait]
impl Provider for MockProvider {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream {
        let digest = request_digest(&req);
        let events = self
            .scripted_events
            .get(&digest)
            .cloned()
            .unwrap_or_else(|| {
                let message = self.format_missing_fixture_message(&digest);
                vec![ProviderStreamEvent::error(message)]
            });

        Box::pin(stream::iter(events).map(|event| event))
    }
}

pub fn request_digest(request: &CompletionRequest) -> String {
    let normalized = normalize_request(request);
    let normalized_bytes = serde_json::to_vec(&normalized).unwrap_or_else(|_| b"null".to_vec());
    blake3::hash(&normalized_bytes).to_hex().to_string()
}

fn normalize_request(request: &CompletionRequest) -> Value {
    let mut value = serde_json::to_value(request).unwrap_or(Value::Null);
    normalize_volatile_provider_context_for_digest(&mut value);
    canonicalize_json(&value)
}

fn normalize_volatile_provider_context_for_digest(value: &mut Value) {
    let Value::Object(request) = value else {
        return;
    };
    let Some(Value::Object(context)) = request.get_mut("context") else {
        return;
    };

    // Session and request ids are provider-routing metadata that change from run
    // to run. Mock fixtures key off the prompt-visible request shape while still
    // preserving semantic context fields such as cache retention, media, and
    // initiator when they are explicitly non-default.
    context.remove("session_id");
    context.remove("request_id");
    if context.is_empty() {
        request.remove("context");
    }
}

fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut ordered = serde_json::Map::new();
            for (key, value) in map {
                ordered.insert(key.clone(), canonicalize_json(value));
            }
            Value::Object(ordered)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_json).collect()),
        _ => value.clone(),
    }
}

#[derive(Debug, Deserialize)]
struct MockProviderFixture {
    request: FixtureCompletionRequest,
    events: Vec<FixtureStreamEvent>,
}

#[derive(Debug, Deserialize)]
struct FixtureCompletionRequest {
    model_id: String,
    messages: Vec<crate::CompletionMessage>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    variant: Option<String>,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    text_verbosity: Option<String>,
    #[serde(default)]
    reasoning_summary: Option<String>,
    #[serde(default)]
    thinking: Option<serde_json::Value>,
    #[serde(default)]
    tools: Option<Vec<ToolDef>>,
    #[serde(default)]
    tool_choice: Option<ToolChoice>,
    #[serde(default)]
    context: ProviderRequestContext,
    stream: bool,
}

impl From<FixtureCompletionRequest> for CompletionRequest {
    fn from(value: FixtureCompletionRequest) -> Self {
        Self {
            provider_id: None,
            model_id: value.model_id,
            messages: value.messages,
            temperature: value.temperature,
            max_tokens: value.max_tokens,
            variant: value.variant,
            reasoning_effort: value.reasoning_effort,
            text_verbosity: value.text_verbosity,
            reasoning_summary: value.reasoning_summary,
            thinking: value.thinking,
            tools: value.tools,
            tool_choice: value.tool_choice,
            context: value.context,
            stream: value.stream,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum FixtureStreamEvent {
    Start,
    Started {
        #[serde(default)]
        metadata: Option<ProviderStreamStartMetadata>,
    },
    ReasoningDelta {
        text: String,
    },
    TextDelta {
        text: String,
    },
    ToolCallDelta {
        tool_call_id: String,
        #[serde(default)]
        function_name: Option<String>,
        arguments_delta: String,
    },
    ToolCall {
        tool_call_id: String,
        function_name: String,
        arguments_json: String,
    },
    Done {
        usage: CompletionUsage,
    },
    DoneWithMetadata {
        usage: CompletionUsage,
        #[serde(default)]
        metadata: Option<ProviderStreamFinishedMetadata>,
    },
    Error {
        message: String,
    },
}

impl From<FixtureStreamEvent> for ProviderStreamEvent {
    fn from(value: FixtureStreamEvent) -> Self {
        match value {
            FixtureStreamEvent::Start => Self::Start,
            FixtureStreamEvent::Started { metadata } => Self::Started { metadata },
            FixtureStreamEvent::ReasoningDelta { text } => Self::ReasoningDelta(text),
            FixtureStreamEvent::TextDelta { text } => Self::TextDelta(text),
            FixtureStreamEvent::ToolCallDelta {
                tool_call_id,
                function_name,
                arguments_delta,
            } => Self::ToolCallDelta {
                tool_call_id,
                function_name,
                arguments_delta,
            },
            FixtureStreamEvent::ToolCall {
                tool_call_id,
                function_name,
                arguments_json,
            } => Self::ToolCallComplete {
                tool_call_id,
                function_name,
                arguments_json,
            },
            FixtureStreamEvent::Done { usage } => Self::Done { usage },
            FixtureStreamEvent::DoneWithMetadata { usage, metadata } => {
                Self::DoneWithMetadata { usage, metadata }
            }
            FixtureStreamEvent::Error { message } => Self::error(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::UnwrapOrAbort;
    use std::path::PathBuf;

    use tokio_stream::StreamExt;

    use super::{request_digest, MockProvider};
    use crate::{
        CompletionMessage, CompletionRequest, MessageRole, Provider, ProviderRequestContext,
        ProviderStreamEvent,
    };

    #[tokio::test]
    async fn deterministic_streaming_order_from_fixtures() {
        let provider = load_fixture_provider();
        let request = fixture_known_request();

        let mut stream = provider.stream_completion(request).await;
        let mut events = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event);
        }

        assert_eq!(
            events,
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::TextDelta("Hello".to_string()),
                ProviderStreamEvent::TextDelta(" from".to_string()),
                ProviderStreamEvent::TextDelta(" mock provider.".to_string()),
                ProviderStreamEvent::Done {
                    usage: crate::CompletionUsage {
                        prompt_tokens: 8,
                        completion_tokens: 4,
                        total_tokens: 12,
                    }
                }
            ]
        );
    }

    #[tokio::test]
    async fn unknown_digest_returns_deterministic_error() {
        let provider = load_fixture_provider();
        let request = CompletionRequest {
            provider_id: None,
            model_id: "model-unknown".to_string(),
            messages: vec![CompletionMessage {
                role: MessageRole::User,
                content: "this request has no fixture".to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            }],
            temperature: Some(0.1),
            max_tokens: Some(7),
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

        let digest = request_digest(&request);
        let events: Vec<_> = provider.stream_completion(request).await.collect().await;
        assert_eq!(events.len(), 1);
        let ProviderStreamEvent::Error {
            message, category, ..
        } = &events[0]
        else {
            panic!("expected error event, got {:?}", events[0]);
        };
        assert_eq!(*category, None);
        assert!(
            message.contains(&format!("request_digest={digest}")),
            "error should name missing digest: {message}"
        );
        assert!(
            message.contains("fixture_path="),
            "error should include configured fixture path: {message}"
        );
        assert!(
            message.contains("add a fixture JSON"),
            "error should be actionable: {message}"
        );
        assert!(
            message.contains("harness run --scenario golden_path --deterministic"),
            "error should suggest the golden path scenario: {message}"
        );
        assert!(
            message.contains("harness run --mock"),
            "error should suggest mock fixture recording: {message}"
        );
    }

    #[test]
    fn request_digest_ignores_volatile_provider_context_ids() {
        let base = fixture_known_request();
        let mut contextual = base.clone();
        contextual.context = ProviderRequestContext {
            session_id: Some("agent-session-one".to_string()),
            request_id: Some("req-volatile-one".to_string()),
            ..ProviderRequestContext::default()
        };

        assert_eq!(request_digest(&base), request_digest(&contextual));
    }

    #[tokio::test]
    async fn tool_call_fixture_emits_structured_tool_call_events() {
        let provider = load_fixture_provider();
        let request = fixture_tool_call_request();

        let events: Vec<_> = provider.stream_completion(request).await.collect().await;
        assert_eq!(
            events,
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::ToolCallDelta {
                    tool_call_id: "call_fs_read_1".to_string(),
                    function_name: Some("filesystem_read".to_string()),
                    arguments_delta: "{\"filePath\":".to_string(),
                },
                ProviderStreamEvent::ToolCallDelta {
                    tool_call_id: "call_fs_read_1".to_string(),
                    function_name: None,
                    arguments_delta: "\"/tmp/demo.txt\"}".to_string(),
                },
                ProviderStreamEvent::ToolCallComplete {
                    tool_call_id: "call_fs_read_1".to_string(),
                    function_name: "filesystem_read".to_string(),
                    arguments_json: "{\"filePath\":\"/tmp/demo.txt\"}".to_string(),
                },
                ProviderStreamEvent::Done {
                    usage: crate::CompletionUsage {
                        prompt_tokens: 21,
                        completion_tokens: 9,
                        total_tokens: 30,
                    }
                }
            ]
        );
    }

    fn load_fixture_provider() -> MockProvider {
        MockProvider::from_fixture_dir(fixture_dir()).unwrap_or_abort()
    }

    fn fixture_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("harness-testkit")
            .join("fixtures")
            .join("mock_provider")
    }

    fn fixture_known_request() -> CompletionRequest {
        CompletionRequest {
            provider_id: None,
            model_id: "model-mock-1".to_string(),
            messages: vec![
                CompletionMessage {
                    role: MessageRole::System,
                    content: "You are deterministic.".to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
                CompletionMessage {
                    role: MessageRole::User,
                    content: "Say hello.".to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
            ],
            temperature: Some(0.0),
            max_tokens: Some(32),
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

    fn fixture_tool_call_request() -> CompletionRequest {
        CompletionRequest {
            provider_id: None,
            model_id: "model-mock-1".to_string(),
            messages: vec![
                CompletionMessage {
                    role: MessageRole::System,
                    content: "You are deterministic.".to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
                CompletionMessage {
                    role: MessageRole::User,
                    content: "Read /tmp/demo.txt using a tool call.".to_string(),
                    name: None,
                    tool_call_id: None,
                    assistant_tool_calls: None,
                },
            ],
            temperature: Some(0.0),
            max_tokens: Some(64),
            variant: None,
            reasoning_effort: None,
            text_verbosity: None,
            reasoning_summary: None,
            thinking: None,
            tools: Some(vec![crate::ToolDef {
                tool_id: "fs.read".to_string(),
                function_name: "filesystem_read".to_string(),
                description: Some("Read file content by absolute path".to_string()),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "filePath": {"type": "string"}
                    },
                    "required": ["filePath"],
                    "additionalProperties": false
                }),
            }]),
            tool_choice: Some(crate::ToolChoice::Auto),
            context: Default::default(),
            stream: true,
        }
    }
}
