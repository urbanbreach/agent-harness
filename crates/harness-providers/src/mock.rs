use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;
use tokio_stream::{self as stream, StreamExt};

use crate::{
    CompletionRequest, CompletionUsage, Provider, ProviderEventStream, ProviderStreamEvent,
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
}

impl MockProvider {
    pub fn new(scripted_events: BTreeMap<String, Vec<ProviderStreamEvent>>) -> Self {
        Self { scripted_events }
    }

    pub fn from_fixture_dir(path: impl AsRef<Path>) -> Result<Self, MockProviderError> {
        let path = path.as_ref();
        let mut entries = fs::read_dir(path)
            .map_err(|source| MockProviderError::ReadFixtureDir {
                path: display_path(path),
                source,
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| MockProviderError::ReadFixtureDir {
                path: display_path(path),
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
                    path: display_path(&entry_path),
                    source,
                }
            })?;
            let fixture: MockProviderFixture =
                serde_json::from_str(&body).map_err(|source| MockProviderError::ParseFixture {
                    path: display_path(&entry_path),
                    source,
                })?;

            let request = fixture.request.into_completion_request();
            let digest = request_digest(&request);
            scripted_events.insert(digest, fixture.events.into_iter().map(Into::into).collect());
        }

        Ok(Self { scripted_events })
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
                vec![ProviderStreamEvent::Error {
                    message: format!("mock fixture missing for request_digest={digest}"),
                }]
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
    let value = serde_json::to_value(request).unwrap_or(Value::Null);
    canonicalize_json(&value)
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

fn display_path(path: &Path) -> String {
    path.display().to_string()
}

#[derive(Debug, Deserialize)]
struct MockProviderFixture {
    request: FixtureCompletionRequest,
    events: Vec<FixtureStreamEvent>,
}

#[derive(Debug, Deserialize)]
struct FixtureCompletionRequest {
    model_id: String,
    #[serde(default, alias = "input")]
    messages: Vec<crate::CompletionMessage>,
    #[serde(default)]
    temperature: Option<f32>,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    stream: bool,
    #[serde(default)]
    tools: Vec<Value>,
}

impl FixtureCompletionRequest {
    fn into_completion_request(self) -> CompletionRequest {
        let _ = self.tools;
        CompletionRequest {
            model_id: self.model_id,
            messages: self.messages,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            stream: self.stream,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum FixtureStreamEvent {
    Start,
    TextDelta {
        text: String,
    },
    Done {
        usage: CompletionUsage,
        #[serde(default)]
        finish_reason: Option<String>,
    },
    ToolCall {
        call_id: String,
        name: String,
        arguments_json: String,
    },
    Error {
        message: String,
    },
}

impl From<FixtureStreamEvent> for ProviderStreamEvent {
    fn from(value: FixtureStreamEvent) -> Self {
        match value {
            FixtureStreamEvent::Start => Self::Start,
            FixtureStreamEvent::TextDelta { text } => Self::TextDelta(text),
            FixtureStreamEvent::Done {
                usage,
                finish_reason,
            } => {
                let _ = finish_reason;
                Self::Done { usage }
            }
            FixtureStreamEvent::ToolCall {
                call_id,
                name,
                arguments_json,
            } => Self::TextDelta(format!("[tool_call:{name}:{call_id}:{arguments_json}]")),
            FixtureStreamEvent::Error { message } => Self::Error { message },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tokio_stream::StreamExt;

    use super::{request_digest, MockProvider};
    use crate::{CompletionMessage, CompletionRequest, MessageRole, Provider, ProviderStreamEvent};

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
            model_id: "model-unknown".to_string(),
            messages: vec![CompletionMessage {
                role: MessageRole::User,
                content: "this request has no fixture".to_string(),
            }],
            temperature: Some(0.1),
            max_tokens: Some(7),
            stream: true,
        };

        let digest = request_digest(&request);
        let events: Vec<_> = provider.stream_completion(request).await.collect().await;
        assert_eq!(
            events,
            vec![ProviderStreamEvent::Error {
                message: format!("mock fixture missing for request_digest={digest}")
            }]
        );
    }

    fn load_fixture_provider() -> MockProvider {
        MockProvider::from_fixture_dir(fixture_dir()).expect("load mock provider fixtures")
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
            model_id: "model-mock-1".to_string(),
            messages: vec![
                CompletionMessage {
                    role: MessageRole::System,
                    content: "You are deterministic.".to_string(),
                },
                CompletionMessage {
                    role: MessageRole::User,
                    content: "Say hello.".to_string(),
                },
            ],
            temperature: Some(0.0),
            max_tokens: Some(32),
            stream: true,
        }
    }
}
