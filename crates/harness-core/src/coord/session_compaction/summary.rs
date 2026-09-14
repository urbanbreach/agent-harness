use std::sync::Arc;

use harness_providers::{
    CompletionMessage, CompletionRequest, CompletionUsage, MessageRole, Provider,
};
use tokio_util::sync::CancellationToken;

use crate::digest::digest12_json;

use super::super::compaction::SUMMARIZATION_SYSTEM_PROMPT;
use super::summary_reducer::SummaryGenerationError;

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
    pub(in crate::coord) task_intent: Option<String>,
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
    pub(super) progress: Option<&'a SummaryProgress>,
    pub(super) messages: &'a [CompletionMessage],
    pub(super) tools: Option<&'a [harness_providers::ToolDef]>,
    pub(super) context_window: u32,
}

pub(in crate::coord) struct SummaryProgress {
    pub(super) job_tx: tokio::sync::mpsc::Sender<super::super::Command>,
    pub(super) agent_id: String,
    pub(super) generation: u64,
}

impl SummaryProgress {
    pub(super) fn publish(&self, preview: &str) {
        let _ = self
            .job_tx
            .try_send(super::super::Command::CompactionProgress {
                agent_id: self.agent_id.clone(),
                generation: self.generation,
                preview: preview.to_string(),
            });
    }
}

pub(super) async fn generate_summary(
    provider: &Arc<dyn Provider>,
    generation: SummaryGenerationRequest<'_>,
    cancellation: &CancellationToken,
) -> Result<GeneratedSummary, SummaryGenerationError> {
    let mut messages = generation.messages.to_vec();
    if messages
        .first()
        .is_none_or(|message| message.role != MessageRole::System)
    {
        messages.insert(
            0,
            text_message(MessageRole::System, SUMMARIZATION_SYSTEM_PROMPT),
        );
    }
    messages.push(text_message(MessageRole::User, generation.user_prompt));
    let mut request = CompletionRequest {
        provider_id: Some(generation.provider_id.to_string()),
        model_id: generation.model_id.to_string(),
        messages,
        temperature: None,
        max_tokens: Some(generation.max_tokens),
        variant: None,
        reasoning_effort: generation
            .provider_id
            .contains("openai")
            .then(|| "low".to_string()),
        text_verbosity: None,
        reasoning_summary: None,
        thinking: generation
            .provider_id
            .contains("anthropic")
            .then(|| serde_json::json!({ "type": "disabled" })),
        tools: generation.tools.map(<[_]>::to_vec),
        tool_choice: Some(harness_providers::ToolChoice::None),
        context: harness_providers::ProviderRequestContext {
            cache_retention: harness_providers::CacheRetention::None,
            ..Default::default()
        },
        stream: true,
    };
    let prompt_tokens = crate::coord::compaction::estimate_text_tokens(generation.user_prompt);
    let history_budget = generation.context_window.saturating_mul(3) / 5;
    if generation.context_window > 0 {
        shrink_summary_history(
            &mut request.messages,
            history_budget.saturating_sub(prompt_tokens).max(256),
        );
    }
    let started = tokio::time::Instant::now();
    let mut retries = 0_u32;
    let mut overflows = 0;
    let mut retried_tool_call = false;
    let publish = |text: &str| {
        if let Some(progress) = generation.progress {
            progress.publish(text);
        }
    };
    let (reduced, request_digest) = loop {
        let input_tokens = summary_input_tokens(&request.messages);
        let budget_ms = u64::from(input_tokens)
            .saturating_mul(2)
            .clamp(120_000, 1_800_000);
        let attempt = async {
            let stream = provider.stream_completion(request.clone()).await;
            super::summary_reducer::reduce_summary_stream_with_progress(
                stream,
                cancellation,
                Some(&publish),
            )
            .await
        };
        let outcome = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(SummaryGenerationError::Cancelled),
            result = tokio::time::timeout(std::time::Duration::from_millis(budget_ms), attempt) => result.unwrap_or(Err(SummaryGenerationError::DurationBudget)),
        };
        match outcome {
            Ok(reduced) => break (reduced, digest12_json(&request)),
            Err(SummaryGenerationError::Provider {
                category: Some(harness_providers::ProviderErrorCategory::ContextWindowExceeded),
                ..
            }) if overflows < 2 && started.elapsed().as_millis() < 240_000 => {
                overflows += 1;
                if !shrink_summary_history(&mut request.messages, input_tokens / 2)
                    && !drop_oldest_summary_message(&mut request.messages)
                {
                    return Err(SummaryGenerationError::IncompleteOutput(
                        "summarization context cannot shrink further".to_string(),
                    ));
                }
            }
            Err(SummaryGenerationError::Provider {
                category:
                    Some(
                        harness_providers::ProviderErrorCategory::TransportFailure
                        | harness_providers::ProviderErrorCategory::RateLimited,
                    ),
                retry_after_ms,
                ..
            }) if retries < 3 && started.elapsed().as_millis() < u128::from(budget_ms / 2) => {
                let delay = retry_after_ms.unwrap_or(1_000_u64 << retries);
                if started
                    .elapsed()
                    .as_millis()
                    .saturating_add(u128::from(delay))
                    >= u128::from(budget_ms / 2)
                {
                    return Err(SummaryGenerationError::DurationBudget);
                }
                retries += 1;
                tokio::select! {
                    biased;
                    () = cancellation.cancelled() => return Err(SummaryGenerationError::Cancelled),
                    () = tokio::time::sleep(std::time::Duration::from_millis(delay)) => {},
                }
            }
            Err(SummaryGenerationError::ToolCall) if !retried_tool_call => {
                retried_tool_call = true;
                request.tools = None;
            }
            Err(error) => return Err(error),
        }
        publish("");
    };

    let (task_intent, summary) = extract_task_intent(&reduced.text);
    if summary.trim().is_empty() {
        return Err(SummaryGenerationError::EmptyOutput);
    }
    Ok(GeneratedSummary {
        text: SummaryText(summary),
        task_intent,
        usage: reduced.usage,
        provider_id: generation.provider_id.to_string(),
        model_id: generation.model_id.to_string(),
        request_digest,
        terminal_status: SummaryTerminalStatus::Completed,
    })
}

fn text_message(role: MessageRole, content: &str) -> CompletionMessage {
    CompletionMessage {
        role,
        content: content.to_string(),
        name: None,
        tool_call_id: None,
        assistant_tool_calls: None,
    }
}

fn summary_input_tokens(messages: &[CompletionMessage]) -> u32 {
    messages.iter().fold(0_u32, |total, message| {
        let serialized = serde_json::to_string(message).unwrap_or_default();
        let extra = serialized.chars().filter(|ch| matches!(*ch, '\u{1100}'..='\u{11ff}' | '\u{2e80}'..='\u{9fff}' | '\u{ac00}'..='\u{d7af}' | '\u{f900}'..='\u{faff}' | '\u{ff00}'..='\u{ffef}' | '\u{20000}'..='\u{2fa1f}'))
            .map(char::len_utf16).sum::<usize>();
        total.saturating_add(crate::coord::compaction::estimate_text_tokens(&serialized)).saturating_add(u32::try_from(extra.div_ceil(2)).unwrap_or(u32::MAX))
    })
}

fn drop_oldest_summary_message(messages: &mut Vec<CompletionMessage>) -> bool {
    // Keep the system prompt and internal control message, and never retry an identical request.
    if messages.len() <= 3 {
        return false;
    }
    remove_summary_message(messages, 1);
    true
}

fn remove_summary_message(messages: &mut Vec<CompletionMessage>, index: usize) {
    let removed = messages.remove(index);
    if let Some(calls) = removed.assistant_tool_calls {
        messages.retain(|message| {
            !message
                .tool_call_id
                .as_ref()
                .is_some_and(|id| calls.iter().any(|call| &call.tool_call_id == id))
        });
    }
}

fn shrink_summary_history(messages: &mut Vec<CompletionMessage>, budget: u32) -> bool {
    let original_len = messages.len();
    while summary_input_tokens(messages) > budget {
        // Preserve the final user turn and the trailing compaction instruction.
        let boundary = messages
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, message)| message.role == MessageRole::User)
            .nth(1)
            .map_or(messages.len().saturating_sub(1), |(index, _)| index);
        let candidate = (1..boundary)
            .find(|&index| {
                messages[index]
                    .assistant_tool_calls
                    .as_ref()
                    .is_some_and(|calls| !calls.is_empty())
            })
            .or_else(|| (1..boundary).next());
        let Some(index) = candidate else {
            break;
        };
        remove_summary_message(messages, index);
    }
    messages.len() < original_len
}

fn extract_task_intent(text: &str) -> (Option<String>, String) {
    let mut remaining = text.to_string();
    let mut intent = None;
    while let Some(start) = remaining.find("<task-intent>") {
        let from = start + "<task-intent>".len();
        let Some(end) = remaining[from..]
            .find("</task-intent>")
            .map(|end| from + end)
        else {
            break;
        };
        let value = remaining[from..end].trim();
        if intent.is_none() {
            intent = Some(value[..value.floor_char_boundary(4096)].trim().to_string());
        }
        remaining.replace_range(start..end + "</task-intent>".len(), "");
    }
    let mut blocks = Vec::new();
    let mut rest = remaining.as_str();
    while let Some((_, after)) = rest.split_once("<summary>") {
        let Some((summary, after)) = after.split_once("</summary>") else {
            break;
        };
        blocks.push(summary.trim());
        rest = after;
    }
    let summary = if blocks.is_empty() {
        remaining.trim().to_string()
    } else {
        blocks.join("\n").trim().to_string()
    };
    (intent, summary)
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
                progress: None,
                messages: &[],
                tools: None,
                context_window: 200_000,
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
    #[tokio::test(start_paused = true)]
    async fn compaction_watchdog_bounds_provider_acquisition_and_cancellation() {
        struct StalledProvider;
        #[async_trait]
        impl Provider for StalledProvider {
            fn request_budget_semantics(
                &self,
                request: &CompletionRequest,
                pending_prompt_index: usize,
            ) -> Result<ProviderBudgetSemantics, ProviderRequestCostError> {
                generic_request_budget_semantics(request, pending_prompt_index)
            }
            async fn stream_completion(&self, _: CompletionRequest) -> ProviderEventStream {
                std::future::pending().await
            }
        }
        let provider: Arc<dyn Provider> = Arc::new(StalledProvider);
        let request = || SummaryGenerationRequest {
            provider_id: "mock",
            model_id: "model",
            user_prompt: "summarize",
            max_tokens: 1024,
            progress: None,
            messages: &[],
            tools: None,
            context_window: 128_000,
        };
        let started = tokio::time::Instant::now();
        assert_eq!(
            generate_summary(&provider, request(), &CancellationToken::new()).await,
            Err(SummaryGenerationError::DurationBudget)
        );
        assert_eq!(started.elapsed(), std::time::Duration::from_secs(120));
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let started = tokio::time::Instant::now();
        assert_eq!(
            generate_summary(&provider, request(), &cancellation).await,
            Err(SummaryGenerationError::Cancelled)
        );
        assert!(started.elapsed().is_zero());
    }

    #[tokio::test]
    async fn compaction_summary_overflow_retries_shrink_and_keep_control_prompt() {
        struct OverflowProvider(Mutex<Vec<CompletionRequest>>);
        #[async_trait]
        impl Provider for OverflowProvider {
            fn request_budget_semantics(
                &self,
                request: &CompletionRequest,
                index: usize,
            ) -> Result<ProviderBudgetSemantics, ProviderRequestCostError> {
                generic_request_budget_semantics(request, index)
            }
            async fn stream_completion(&self, request: CompletionRequest) -> ProviderEventStream {
                let mut requests = self.0.lock().expect("request capture");
                requests.push(request);
                let events = if requests.len() < 3 {
                    vec![ProviderStreamEvent::categorized_error(
                        "context overflow",
                        harness_providers::ProviderErrorCategory::ContextWindowExceeded,
                    )]
                } else {
                    vec![
                        ProviderStreamEvent::Start,
                        ProviderStreamEvent::TextDelta("bounded summary".to_string()),
                        ProviderStreamEvent::Done { usage: None },
                    ]
                };
                Box::pin(tokio_stream::iter(events))
            }
        }
        let concrete = Arc::new(OverflowProvider(Mutex::new(Vec::new())));
        let provider: Arc<dyn Provider> = Arc::clone(&concrete) as Arc<dyn Provider>;
        let messages = [
            text_message(MessageRole::User, &"older context ".repeat(300)),
            text_message(MessageRole::Assistant, "older answer"),
            text_message(MessageRole::User, "current user"),
            text_message(MessageRole::Assistant, "current answer"),
        ];
        let result = generate_summary(
            &provider,
            SummaryGenerationRequest {
                provider_id: "mock",
                model_id: "model",
                user_prompt: "internal compaction control",
                max_tokens: 1024,
                progress: None,
                messages: &messages,
                tools: None,
                context_window: 128_000,
            },
            &CancellationToken::new(),
        )
        .await
        .expect("bounded retry succeeds");
        assert_eq!(result.text.as_str(), "bounded summary");
        let requests = concrete.0.lock().expect("request capture");
        assert_eq!(requests.len(), 3);
        assert!(requests
            .windows(2)
            .all(|pair| pair[1].messages.len() < pair[0].messages.len()));
        assert!(requests.iter().all(|request| request
            .messages
            .last()
            .is_some_and(|message| message.content == "internal compaction control")
            && request.tool_choice == Some(harness_providers::ToolChoice::None)));
    }

    #[test]
    fn compaction_task_intent_extraction_is_bounded_and_keeps_all_summary_blocks() {
        let intent = "😀".repeat(1100);
        let (extracted, summary) = extract_task_intent(&format!("preamble<task-intent>{intent}</task-intent><summary>first</summary><task-intent>ignore</task-intent><summary>second</summary>trailing"));
        assert_eq!(extracted.as_deref().map(str::len), Some(4096));
        assert_eq!(summary, "first\nsecond");
        assert_eq!(
            extract_task_intent("plain summary"),
            (None, "plain summary".to_string())
        );
    }
}
