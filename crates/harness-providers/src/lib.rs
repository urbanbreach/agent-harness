use std::pin::Pin;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompletionRequest {
    pub model_id: ModelId,
    pub messages: Vec<CompletionMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
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
    Done { usage: CompletionUsage },
    Error { message: String },
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn stream_completion(&self, req: CompletionRequest) -> ProviderEventStream;
}
