use tokio::sync::mpsc;

use crate::{CompletionUsage, ProviderStreamEvent, ProviderStreamStartMetadata};

use super::super::sse::next_sse_event;
use super::super::stream_event::{
    malformed_stream_error, non_empty_finished_metadata,
    provider_stream_finished_metadata_from_start, transport_failure_error,
    unsupported_tool_call_error,
};
use super::super::stream_payload::OpenAiResponsesEvent;
use super::super::tool_call::{
    emit_pending_responses_tool_call_completions, handle_responses_arguments_delta,
    handle_responses_tool_item_added, handle_responses_tool_item_done, ResponsesToolCallState,
};
use super::super::transport::OpenAiHttpResponse;
use super::{warn_stream_processing_failure, warn_stream_send_failure};

pub(super) async fn consume_responses_sse_stream(
    response: OpenAiHttpResponse,
    tx: mpsc::Sender<ProviderStreamEvent>,
    start_metadata: Option<ProviderStreamStartMetadata>,
) {
    if tx
        .send(ProviderStreamEvent::Started {
            metadata: start_metadata.clone(),
        })
        .await
        .is_err()
    {
        warn_stream_send_failure("responses.start");
        return;
    }

    let mut usage: Option<CompletionUsage> = None;
    let mut finished_metadata = provider_stream_finished_metadata_from_start(start_metadata);
    let mut body = response.body;
    let mut sse_buffer = Vec::new();
    let mut tool_calls = ResponsesToolCallState::default();
    let mut reasoning_summary_key: Option<(Option<String>, usize)> = None;
    let mut reasoning_trailing_newlines = 0usize;

    loop {
        let event = match next_sse_event(&mut body, &mut sse_buffer).await {
            Ok(Some(event)) => event,
            Ok(None) => break,
            Err(message) => {
                warn_stream_processing_failure(
                    "responses.transport",
                    &format!("openai_compatible SSE stream transport error: {message}"),
                );
                let _ = tx
                    .send(transport_failure_error(format!(
                        "openai_compatible SSE stream transport error: {message}"
                    )))
                    .await;
                return;
            }
        };

        let data = event.data.trim();
        if data.is_empty() {
            continue;
        }
        if data == "[DONE]" {
            if let Err(message) =
                emit_pending_responses_tool_call_completions(&tx, &mut tool_calls).await
            {
                warn_stream_processing_failure("responses.tool_completion", &message);
                let _ = tx.send(unsupported_tool_call_error(message)).await;
                return;
            }
            if tx
                .send(ProviderStreamEvent::DoneWithMetadata {
                    usage,
                    metadata: non_empty_finished_metadata(finished_metadata),
                })
                .await
                .is_err()
            {
                warn_stream_send_failure("responses.done");
            }
            return;
        }

        let parsed: OpenAiResponsesEvent = match serde_json::from_str(data) {
            Ok(parsed) => parsed,
            Err(err) => {
                warn_stream_processing_failure(
                    "responses.invalid_json",
                    &format!(
                        "openai_compatible returned invalid SSE JSON chunk: {err}; sample={}",
                        summarize_sse_data(data)
                    ),
                );
                let _ = tx
                    .send(malformed_stream_error(format!(
                        "openai_compatible returned invalid SSE JSON chunk: {err}; sample={}",
                        summarize_sse_data(data)
                    )))
                    .await;
                return;
            }
        };

        match parsed.event_type.as_str() {
            "response.reasoning_summary_text.delta" => {
                if let Some(mut delta) = parsed.delta {
                    if let Some(summary_index) = parsed.summary_index {
                        let next_key = (parsed.item_id.clone(), summary_index);
                        let starts_new_summary = reasoning_summary_key
                            .as_ref()
                            .is_some_and(|current_key| current_key != &next_key);
                        reasoning_summary_key = Some(next_key);
                        if starts_new_summary {
                            let leading_newlines = delta
                                .chars()
                                .take_while(|character| *character == '\n')
                                .take(2)
                                .count();
                            let missing_newlines = 2usize
                                .saturating_sub(reasoning_trailing_newlines + leading_newlines);
                            match missing_newlines {
                                2 => delta.insert_str(0, "\n\n"),
                                1 => delta.insert(0, '\n'),
                                _ => {}
                            }
                        }
                    }
                    reasoning_trailing_newlines =
                        delta
                            .chars()
                            .fold(reasoning_trailing_newlines, |count, character| {
                                if character == '\n' {
                                    count.saturating_add(1).min(2)
                                } else {
                                    0
                                }
                            });
                    if !delta.is_empty()
                        && tx
                            .send(ProviderStreamEvent::ReasoningDelta(delta))
                            .await
                            .is_err()
                    {
                        return;
                    }
                }
            }
            "response.output_text.delta" => {
                if let Some(delta) = parsed.delta {
                    if !delta.is_empty()
                        && tx
                            .send(ProviderStreamEvent::TextDelta(delta))
                            .await
                            .is_err()
                    {
                        return;
                    }
                }
            }
            "response.output_item.added" => {
                if !handle_responses_tool_item_added(&tx, &mut tool_calls, parsed).await {
                    return;
                }
            }
            "response.function_call_arguments.delta" => {
                if !handle_responses_arguments_delta(&tx, &mut tool_calls, parsed).await {
                    return;
                }
            }
            "response.output_item.done" => {
                if !handle_responses_tool_item_done(&tx, &mut tool_calls, parsed).await {
                    return;
                }
            }
            "response.completed" | "response.done" | "response.incomplete" => {
                finished_metadata.provider_stop_reason = Some(parsed.event_type.clone());
                if let Some(response) = parsed.response {
                    response.merge_finished_metadata(&mut finished_metadata);
                    if let Some(completion_usage) =
                        response.usage.map(|usage| usage.completion_usage())
                    {
                        usage = Some(completion_usage);
                    }
                }
            }
            "response.error" => {
                warn_stream_processing_failure(
                    "responses.error_event",
                    "openai_compatible responses stream returned error event",
                );
                let _ = tx
                    .send(malformed_stream_error(
                        "openai_compatible responses stream returned error event",
                    ))
                    .await;
                return;
            }
            _ => {}
        }
    }

    if let Err(message) = emit_pending_responses_tool_call_completions(&tx, &mut tool_calls).await {
        warn_stream_processing_failure("responses.tool_completion", &message);
        let _ = tx.send(unsupported_tool_call_error(message)).await;
        return;
    }
    if tx
        .send(ProviderStreamEvent::DoneWithMetadata {
            usage,
            metadata: non_empty_finished_metadata(finished_metadata),
        })
        .await
        .is_err()
    {
        warn_stream_send_failure("responses.done_after_stream_end");
    }
}

fn summarize_sse_data(data: &str) -> String {
    let mut snippet = data
        .chars()
        .take(160)
        .collect::<String>()
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    if data.chars().count() > 160 {
        snippet.push('…');
    }
    snippet
}
