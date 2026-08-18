use serde::Deserialize;

use super::non_empty_string;
use crate::{CompletionUsage, ProviderStreamFinishedMetadata};

#[derive(Debug, Deserialize)]
pub(super) struct OpenAiResponsesEvent {
    #[serde(rename = "type")]
    pub(super) event_type: String,
    #[serde(default)]
    pub(super) delta: Option<String>,
    #[serde(default)]
    pub(super) item_id: Option<String>,
    #[serde(default)]
    pub(super) summary_index: Option<usize>,
    #[serde(default)]
    pub(super) item: Option<OpenAiResponsesOutputItem>,
    #[serde(default)]
    pub(super) response: Option<OpenAiResponsesResponsePayload>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAiResponsesOutputItem {
    #[serde(rename = "type")]
    pub(super) item_type: String,
    #[serde(default)]
    pub(super) id: Option<String>,
    #[serde(default)]
    pub(super) call_id: Option<String>,
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAiResponsesResponsePayload {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default, alias = "session_id")]
    provider_session_id: Option<String>,
    #[serde(default, alias = "cache_id")]
    provider_cache_id: Option<String>,
    #[serde(default)]
    pub(super) usage: Option<OpenAiResponsesUsage>,
}

impl OpenAiResponsesResponsePayload {
    pub(super) fn merge_finished_metadata(&self, metadata: &mut ProviderStreamFinishedMetadata) {
        if let Some(id) = self.id.as_deref().and_then(non_empty_string) {
            metadata.provider_response_id = Some(id.to_string());
        }
        if let Some(status) = self.status.as_deref().and_then(non_empty_string) {
            metadata.provider_stop_reason = Some(status.to_string());
        }
        if let Some(session_id) = self
            .provider_session_id
            .as_deref()
            .and_then(non_empty_string)
        {
            metadata.provider_session_id = Some(session_id.to_string());
        }
        if let Some(cache_id) = self.provider_cache_id.as_deref().and_then(non_empty_string) {
            metadata.provider_cache_id = Some(cache_id.to_string());
        }
        if let Some(usage) = &self.usage {
            usage.merge_finished_metadata(metadata);
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct OpenAiResponsesUsage {
    #[serde(default)]
    prompt_tokens: Option<u32>,
    #[serde(default)]
    completion_tokens: Option<u32>,
    #[serde(default)]
    input_tokens: Option<u32>,
    #[serde(default)]
    output_tokens: Option<u32>,
    #[serde(default)]
    total_tokens: Option<u32>,
    #[serde(default)]
    prompt_tokens_details: Option<OpenAiTokenDetails>,
    #[serde(default)]
    input_tokens_details: Option<OpenAiTokenDetails>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u32>,
    #[serde(default)]
    cache_write_input_tokens: Option<u32>,
}

impl OpenAiResponsesUsage {
    pub(super) fn completion_usage(&self) -> CompletionUsage {
        let prompt_tokens = self.prompt_tokens.or(self.input_tokens).unwrap_or(0);
        let completion_tokens = self.completion_tokens.or(self.output_tokens).unwrap_or(0);
        let total_tokens = self
            .total_tokens
            .filter(|&total| total > 0)
            .unwrap_or(prompt_tokens.saturating_add(completion_tokens));

        CompletionUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        }
    }

    pub(super) fn merge_finished_metadata(&self, metadata: &mut ProviderStreamFinishedMetadata) {
        metadata.cache_read_tokens = self
            .input_tokens_details
            .as_ref()
            .and_then(|details| details.cached_tokens)
            .or_else(|| {
                self.prompt_tokens_details
                    .as_ref()
                    .and_then(|details| details.cached_tokens)
            })
            .or(metadata.cache_read_tokens);
        metadata.cache_write_tokens = self
            .cache_write_input_tokens
            .or(self.cache_creation_input_tokens)
            .or_else(|| {
                self.input_tokens_details.as_ref().and_then(|details| {
                    details
                        .cache_write_tokens
                        .or(details.cache_creation_tokens)
                        .or(details.cache_creation_input_tokens)
                })
            })
            .or_else(|| {
                self.prompt_tokens_details.as_ref().and_then(|details| {
                    details
                        .cache_write_tokens
                        .or(details.cache_creation_tokens)
                        .or(details.cache_creation_input_tokens)
                })
            })
            .or(metadata.cache_write_tokens);
    }
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiTokenDetails {
    #[serde(default)]
    cached_tokens: Option<u32>,
    #[serde(default)]
    cache_write_tokens: Option<u32>,
    #[serde(default)]
    cache_creation_tokens: Option<u32>,
    #[serde(default)]
    cache_creation_input_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAiChatCompletionsChunk {
    #[serde(default)]
    pub(super) id: Option<String>,
    #[serde(default)]
    pub(super) choices: Vec<OpenAiChatChoiceChunk>,
    #[serde(default)]
    pub(super) usage: Option<OpenAiResponsesUsage>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAiChatChoiceChunk {
    #[serde(default)]
    pub(super) delta: OpenAiChatDeltaChunk,
    #[serde(default)]
    pub(super) finish_reason: Option<String>,
    #[serde(default)]
    pub(super) usage: Option<OpenAiResponsesUsage>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct OpenAiChatDeltaChunk {
    #[serde(default)]
    pub(super) content: Option<String>,
    #[serde(default, alias = "reasoning_content")]
    pub(super) reasoning_text: Option<String>,
    #[serde(default)]
    pub(super) tool_calls: Vec<OpenAiChatToolCallDeltaChunk>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OpenAiChatToolCallDeltaChunk {
    pub(super) index: usize,
    #[serde(default)]
    pub(super) id: Option<String>,
    #[serde(default)]
    pub(super) function: Option<OpenAiChatToolFunctionDeltaChunk>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct OpenAiChatToolFunctionDeltaChunk {
    #[serde(default)]
    pub(super) name: Option<String>,
    #[serde(default)]
    pub(super) arguments: Option<String>,
}
