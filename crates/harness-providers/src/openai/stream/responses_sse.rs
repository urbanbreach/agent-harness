use tokio::sync::mpsc;

use crate::{
    CompletionUsage, ProviderStreamEvent, ProviderStreamFinishedMetadata,
    ProviderStreamStartMetadata,
};

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
use super::{send_optional_delta, send_stream_event, warn_stream_processing_failure};

pub(super) async fn consume_responses_sse_stream(
    response: OpenAiHttpResponse,
    tx: mpsc::Sender<ProviderStreamEvent>,
    start_metadata: Option<ProviderStreamStartMetadata>,
) {
    if !send_stream_event(
        &tx,
        ProviderStreamEvent::Started {
            metadata: start_metadata.clone(),
        },
        "responses.start",
    )
    .await
    {
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
                let message = format!("openai_compatible SSE stream transport error: {message}");
                warn_stream_processing_failure("responses.transport", &message);
                let _ = tx.send(transport_failure_error(message)).await;
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
            send_stream_event(
                &tx,
                ProviderStreamEvent::DoneWithMetadata {
                    usage,
                    metadata: non_empty_finished_metadata(finished_metadata),
                },
                "responses.done",
            )
            .await;
            return;
        }

        let parsed: OpenAiResponsesEvent = match serde_json::from_str(data) {
            Ok(parsed) => parsed,
            Err(err) => {
                let message = format!(
                    "openai_compatible returned invalid SSE JSON chunk: {err}; sample={}",
                    summarize_sse_data(data)
                );
                warn_stream_processing_failure("responses.invalid_json", &message);
                let _ = tx.send(malformed_stream_error(message)).await;
                return;
            }
        };

        match parsed.event_type.as_str() {
            "response.reasoning_summary_text.delta" => {
                let delta = format_reasoning_delta(
                    parsed,
                    &mut reasoning_summary_key,
                    &mut reasoning_trailing_newlines,
                );
                if !send_optional_delta(&tx, delta, ProviderStreamEvent::ReasoningDelta).await {
                    return;
                }
            }
            "response.output_text.delta" => {
                if !send_optional_delta(&tx, parsed.delta, ProviderStreamEvent::TextDelta).await {
                    return;
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
                apply_response_completion(parsed, &mut usage, &mut finished_metadata);
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
    send_stream_event(
        &tx,
        ProviderStreamEvent::DoneWithMetadata {
            usage,
            metadata: non_empty_finished_metadata(finished_metadata),
        },
        "responses.done_after_stream_end",
    )
    .await;
}

fn format_reasoning_delta(
    parsed: OpenAiResponsesEvent,
    summary_key: &mut Option<(Option<String>, usize)>,
    trailing_newlines: &mut usize,
) -> Option<String> {
    let mut delta = parsed.delta?;
    if let Some(summary_index) = parsed.summary_index {
        let next_key = (parsed.item_id, summary_index);
        let starts_new_summary = summary_key
            .as_ref()
            .is_some_and(|current_key| current_key != &next_key);
        *summary_key = Some(next_key);
        if starts_new_summary {
            let leading_newlines = delta
                .chars()
                .take_while(|character| *character == '\n')
                .take(2)
                .count();
            match 2usize.saturating_sub(*trailing_newlines + leading_newlines) {
                2 => delta.insert_str(0, "\n\n"),
                1 => delta.insert(0, '\n'),
                _ => {}
            }
        }
    }
    *trailing_newlines = delta.chars().fold(*trailing_newlines, |count, character| {
        if character == '\n' {
            count.saturating_add(1).min(2)
        } else {
            0
        }
    });
    Some(delta)
}

fn apply_response_completion(
    parsed: OpenAiResponsesEvent,
    usage: &mut Option<CompletionUsage>,
    finished_metadata: &mut ProviderStreamFinishedMetadata,
) {
    finished_metadata.provider_stop_reason = Some(parsed.event_type);
    let Some(response) = parsed.response else {
        return;
    };
    response.merge_finished_metadata(finished_metadata);
    if let Some(completion_usage) = response.usage.map(|usage| usage.completion_usage()) {
        *usage = Some(completion_usage);
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
