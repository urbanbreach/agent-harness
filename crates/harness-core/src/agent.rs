use std::future::Future;
use std::sync::Arc;

use async_trait::async_trait;
use harness_providers::{
    CompletionMessage, CompletionRequest, MessageRole, Provider, ProviderEventStream,
    ProviderStreamEvent,
};
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentProfile {
    pub name: String,
    pub category: String,
    pub model_ref: String,
    pub system_prompt: String,
    pub toolset: Vec<String>,
}

impl AgentProfile {
    pub fn fallback(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            category: name.clone(),
            model_ref: "default:default".to_string(),
            system_prompt: String::new(),
            toolset: Vec::new(),
            name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRequest {
    pub agent_id: String,
    pub prompt: String,
    pub model_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentModelRef {
    pub provider_id: String,
    pub model_id: String,
}

impl AgentModelRef {
    pub fn parse(model_ref: &str) -> Self {
        let mut parts = model_ref.splitn(2, ':');
        let provider_id = parts
            .next()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("default")
            .to_string();
        let model_id = parts
            .next()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("default")
            .to_string();

        Self {
            provider_id,
            model_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRequestStarted {
    pub request_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub prompt_summary: String,
    pub request_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRequestFinished {
    pub request_id: String,
    pub finish_reason: String,
    pub output_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRuntimeEvent {
    ProviderRequestStarted(ProviderRequestStarted),
    ProviderStreamDelta { request_id: String, delta: String },
    ProviderRequestFinished(ProviderRequestFinished),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentTurnOutcome {
    Succeeded { output: String },
    Failed { reason: String },
}

pub fn default_provider() -> Arc<dyn Provider> {
    Arc::new(NullProvider)
}

pub async fn run_single_turn_streaming<F, Fut>(
    provider: Arc<dyn Provider>,
    profile: &AgentProfile,
    request_id: String,
    request: AgentRequest,
    mut emit: F,
) -> AgentTurnOutcome
where
    F: FnMut(AgentRuntimeEvent) -> Fut,
    Fut: Future<Output = ()>,
{
    let model = AgentModelRef::parse(&request.model_ref);
    let completion_request = CompletionRequest {
        model_id: model.model_id.clone(),
        messages: vec![
            CompletionMessage {
                role: MessageRole::System,
                content: profile.system_prompt.clone(),
            },
            CompletionMessage {
                role: MessageRole::User,
                content: request.prompt.clone(),
            },
        ],
        temperature: Some(0.0),
        max_tokens: None,
        stream: true,
    };

    emit(AgentRuntimeEvent::ProviderRequestStarted(
        ProviderRequestStarted {
            request_id: request_id.clone(),
            provider_id: model.provider_id,
            model_id: model.model_id,
            prompt_summary: truncate_summary(&request.prompt, 256),
            request_digest: digest12_completion_request(&completion_request),
        },
    ))
    .await;

    let mut stream = provider.stream_completion(completion_request).await;
    let mut output = String::new();

    while let Some(event) = stream.next().await {
        match event {
            ProviderStreamEvent::Start => {}
            ProviderStreamEvent::TextDelta(delta) => {
                output.push_str(&delta);
                emit(AgentRuntimeEvent::ProviderStreamDelta {
                    request_id: request_id.clone(),
                    delta,
                })
                .await;
            }
            ProviderStreamEvent::Done { .. } => {
                emit(AgentRuntimeEvent::ProviderRequestFinished(
                    ProviderRequestFinished {
                        request_id: request_id.clone(),
                        finish_reason: "done".to_string(),
                        output_digest: Some(digest12(output.as_bytes())),
                    },
                ))
                .await;

                return AgentTurnOutcome::Succeeded { output };
            }
            ProviderStreamEvent::Error { message } => {
                emit(AgentRuntimeEvent::ProviderRequestFinished(
                    ProviderRequestFinished {
                        request_id: request_id.clone(),
                        finish_reason: "error".to_string(),
                        output_digest: None,
                    },
                ))
                .await;

                return AgentTurnOutcome::Failed { reason: message };
            }
        }
    }

    emit(AgentRuntimeEvent::ProviderRequestFinished(
        ProviderRequestFinished {
            request_id,
            finish_reason: "stream_ended".to_string(),
            output_digest: Some(digest12(output.as_bytes())),
        },
    ))
    .await;

    AgentTurnOutcome::Succeeded { output }
}

fn truncate_summary(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }

    let mut summary: String = text.chars().take(max_chars).collect();
    summary.push('…');
    summary
}

fn digest12_completion_request(request: &CompletionRequest) -> String {
    let bytes = serde_json::to_vec(request).unwrap_or_else(|_| b"null".to_vec());
    digest12(&bytes)
}

fn digest12(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().chars().take(12).collect()
}

struct NullProvider;

#[async_trait]
impl Provider for NullProvider {
    async fn stream_completion(&self, _req: CompletionRequest) -> ProviderEventStream {
        Box::pin(tokio_stream::iter(vec![ProviderStreamEvent::Error {
            message: "no provider configured".to_string(),
        }]))
    }
}
