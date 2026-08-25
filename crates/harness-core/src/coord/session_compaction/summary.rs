use std::sync::Arc;

use harness_providers::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, Provider,
};
use tokio_util::sync::CancellationToken;

use crate::digest::digest12_json;

use super::super::compaction::SUMMARIZATION_SYSTEM_PROMPT;
use super::summary_reducer::{reduce_summary_stream, SummaryGenerationError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::coord) struct SummaryText(String);

impl SummaryText {
    pub(in crate::coord) fn as_str(&self) -> &str {
        &self.0
    }

    pub(in crate::coord) fn into_string(self) -> String {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::coord) enum SummaryTerminalStatus {
    Completed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::coord) struct GeneratedSummary {
    pub(in crate::coord) text: SummaryText,
    pub(in crate::coord) usage: Option<CompletionUsage>,
    pub(in crate::coord) provider_id: String,
    pub(in crate::coord) model_id: String,
    pub(in crate::coord) request_digest: String,
    pub(in crate::coord) terminal_status: SummaryTerminalStatus,
}

pub(super) struct SummaryGenerationRequest<'a> {
    pub(super) provider_id: &'a str,
    pub(super) model_id: &'a str,
    pub(super) user_prompt: &'a str,
    pub(super) max_tokens: u32,
}

pub(super) async fn generate_summary(
    provider: &Arc<dyn Provider>,
    generation: SummaryGenerationRequest<'_>,
    cancellation: &CancellationToken,
) -> Result<GeneratedSummary, SummaryGenerationError> {
    let request = CompletionRequest {
        provider_id: Some(generation.provider_id.to_string()),
        model_id: generation.model_id.to_string(),
        messages: vec![
            CompletionMessage {
                role: MessageRole::System,
                content: SUMMARIZATION_SYSTEM_PROMPT.to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
            CompletionMessage {
                role: MessageRole::User,
                content: generation.user_prompt.to_string(),
                name: None,
                tool_call_id: None,
                assistant_tool_calls: None,
            },
        ],
        temperature: None,
        max_tokens: Some(generation.max_tokens),
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
    let request_digest = digest12_json(&request);

    let stream = tokio::select! {
        biased;
        () = cancellation.cancelled() => return Err(SummaryGenerationError::Cancelled),
        stream = provider.stream_completion(request) => stream,
    };
    let reduced = reduce_summary_stream(stream, cancellation).await?;

    Ok(GeneratedSummary {
        text: SummaryText(reduced.text),
        usage: reduced.usage,
        provider_id: generation.provider_id.to_string(),
        model_id: generation.model_id.to_string(),
        request_digest,
        terminal_status: SummaryTerminalStatus::Completed,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;
    use harness_providers::{
        generic_request_budget_semantics, ProviderBudgetSemantics, ProviderEventStream,
        ProviderRequestCostError, ProviderStreamEvent,
    };

    use super::*;

    struct DeterministicSummaryProvider {
        request: Mutex<Option<CompletionRequest>>,
        usage: CompletionUsage,
    }

    #[async_trait]
    impl Provider for DeterministicSummaryProvider {
        fn request_budget_semantics(
            &self,
            request: &CompletionRequest,
            pending_prompt_index: usize,
        ) -> Result<ProviderBudgetSemantics, ProviderRequestCostError> {
            generic_request_budget_semantics(request, pending_prompt_index)
        }

        async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
            self.request
                .lock()
                .expect("summary request lock should remain available")
                .replace(request);
            Box::pin(tokio_stream::iter(vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::TextDelta("generated summary".to_string()),
                ProviderStreamEvent::Done {
                    usage: Some(self.usage.clone()),
                },
            ]))
        }
    }

    #[tokio::test]
    async fn compaction_v2_summary_generation_result_captures_terminal_digest_and_provenance() {
        // Given: deterministic provider events with exact usage and request capture.
        let usage = CompletionUsage {
            prompt_tokens: 17,
            completion_tokens: 23,
            total_tokens: 40,
        };
        let provider_impl = Arc::new(DeterministicSummaryProvider {
            request: Mutex::new(None),
            usage: usage.clone(),
        });
        let provider: Arc<dyn Provider> = Arc::clone(&provider_impl) as Arc<dyn Provider>;

        // When: the production generator reduces the completed stream.
        let generated = generate_summary(
            &provider,
            SummaryGenerationRequest {
                provider_id: "mock",
                model_id: "model-1",
                user_prompt: "deterministic input",
                max_tokens: 128,
            },
            &CancellationToken::new(),
        )
        .await
        .expect("deterministic completed summary should be generated");
        let request = provider_impl
            .request
            .lock()
            .expect("summary request lock should remain available")
            .clone()
            .expect("generator should submit one request");
        let expected_digest = digest12_json(&request);

        // Then: the generated value owns terminal state, digest, provenance, text, and usage.
        assert_eq!(
            (
                generated.text.as_str(),
                generated.usage.as_ref(),
                generated.provider_id.as_str(),
                generated.model_id.as_str(),
                generated.request_digest.as_str(),
                generated.terminal_status,
            ),
            (
                "generated summary",
                Some(&usage),
                "mock",
                "model-1",
                expected_digest.as_str(),
                SummaryTerminalStatus::Completed,
            )
        );
    }
}
