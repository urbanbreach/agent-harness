use tokio::sync::mpsc;

use crate::{
    CompletionUsage, ProviderErrorCategory, ProviderStreamEvent, ProviderStreamFinishedMetadata,
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
    let mut completion_seen = false;

    let done_context = loop {
        let event = match next_sse_event(&mut body, &mut sse_buffer).await {
            Ok(Some(event)) => event,
            Ok(None) if completion_seen => break "responses.done_after_stream_end",
            Ok(None) => {
                let message = "openai_compatible responses stream ended before a terminal outcome";
                warn_stream_processing_failure("responses.premature_eof", message);
                let _ = tx.send(malformed_stream_error(message)).await;
                return;
            }
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
            break "responses.done";
        }

        let parsed = match parse_responses_event(data) {
            Ok(parsed) => parsed,
            Err((message, category)) => {
                warn_stream_processing_failure("responses.invalid_event", message);
                let _ = tx
                    .send(ProviderStreamEvent::categorized_error(message, category))
                    .await;
                return;
            }
        };

        let handled = match parsed.event_type.as_str() {
            "response.reasoning_summary_text.delta" => {
                let delta = format_reasoning_delta(
                    parsed,
                    &mut reasoning_summary_key,
                    &mut reasoning_trailing_newlines,
                );
                send_optional_delta(&tx, delta, ProviderStreamEvent::ReasoningDelta).await
            }
            "response.output_text.delta" => {
                send_optional_delta(&tx, parsed.delta, ProviderStreamEvent::TextDelta).await
            }
            "response.output_item.added" => {
                handle_responses_tool_item_added(&tx, &mut tool_calls, parsed).await
            }
            "response.function_call_arguments.delta" => {
                handle_responses_arguments_delta(&tx, &mut tool_calls, parsed).await
            }
            "response.output_item.done" => {
                handle_responses_tool_item_done(&tx, &mut tool_calls, parsed).await
            }
            "response.completed" | "response.done" => {
                apply_response_completion(parsed, &mut usage, &mut finished_metadata);
                completion_seen = true;
                true
            }
            _ => true,
        };
        if !handled {
            return;
        }
    };

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
        done_context,
    )
    .await;
}

fn parse_responses_event(
    data: &str,
) -> Result<OpenAiResponsesEvent, (&'static str, ProviderErrorCategory)> {
    let event: OpenAiResponsesEvent = serde_json::from_str(data).map_err(|_| {
        (
            "openai_compatible returned invalid SSE JSON chunk",
            ProviderErrorCategory::MalformedStream,
        )
    })?;
    let status = event
        .response
        .as_ref()
        .and_then(|response| response.status.as_deref());
    if matches!(
        event.event_type.as_str(),
        "error" | "response.error" | "response.failed" | "response.incomplete"
    ) || matches!(status, Some("failed" | "cancelled" | "incomplete"))
    {
        return Err((
            "openai_compatible responses stream failed or was incomplete",
            ProviderErrorCategory::Other,
        ));
    }
    if matches!(
        event.event_type.as_str(),
        "response.completed" | "response.done"
    ) && status.is_some_and(|status| status != "completed")
    {
        return Err((
            "openai_compatible responses completion has inconsistent status",
            ProviderErrorCategory::MalformedStream,
        ));
    }
    Ok(event)
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
