use tokio::sync::mpsc;

use crate::{
    CompletionUsage, ProviderErrorCategory, ProviderStreamEvent, ProviderStreamFinishedMetadata,
    ProviderStreamStartMetadata,
};

use super::super::sse::next_sse_event;
use super::super::stream_event::{
    malformed_stream_error, non_empty_finished_metadata,
    provider_stream_finished_metadata_from_start, transport_failure_error,
};
use super::super::stream_payload::{ChatContent, ChatContentPart, OpenAiChatCompletionsChunk};
use super::super::tool_call::{
    consume_tool_call_deltas, emit_tool_call_completions, ChatToolCallState,
};
use super::super::transport::OpenAiHttpResponse;
use super::{
    non_empty_string, send_optional_delta, send_stream_event, warn_stream_processing_failure,
};

pub(super) async fn consume_chat_sse_stream(
    response: OpenAiHttpResponse,
    tx: mpsc::Sender<ProviderStreamEvent>,
    start_metadata: Option<ProviderStreamStartMetadata>,
) {
    if !send_stream_event(
        &tx,
        ProviderStreamEvent::Started {
            metadata: start_metadata.clone(),
        },
        "chat.start",
    )
    .await
    {
        return;
    }

    let mut usage: Option<CompletionUsage> = None;
    let mut finished_metadata = provider_stream_finished_metadata_from_start(start_metadata);
    let mut finish_seen = false;
    let mut tool_call_state = ChatToolCallState::default();
    let mut body = response.body;
    let mut sse_buffer = Vec::new();

    let done_context = loop {
        let event = match next_sse_event(&mut body, &mut sse_buffer).await {
            Ok(Some(event)) => event,
            Ok(None) if finish_seen => break "chat.done_after_stream_end",
            Ok(None) => {
                let message = "openai_compatible chat stream ended before a terminal outcome";
                warn_stream_processing_failure("chat.premature_eof", message);
                let _ = tx.send(malformed_stream_error(message)).await;
                return;
            }
            Err(_) => {
                warn_stream_processing_failure(
                    "chat.transport",
                    "openai_compatible SSE stream transport error",
                );
                let _ = tx
                    .send(transport_failure_error(
                        "openai_compatible SSE stream transport error",
                    ))
                    .await;
                return;
            }
        };

        let data = event.data.trim();
        if data == "[DONE]" {
            break "chat.done";
        }

        let chunk: OpenAiChatCompletionsChunk = match serde_json::from_str(data) {
            Ok(chunk) => chunk,
            Err(_) => {
                warn_stream_processing_failure(
                    "chat.invalid_json",
                    "openai_compatible returned invalid SSE JSON chunk",
                );
                let _ = tx
                    .send(malformed_stream_error(
                        "openai_compatible returned invalid SSE JSON chunk",
                    ))
                    .await;
                return;
            }
        };

        if chunk.error.is_some() {
            let message = "openai_compatible chat stream returned an error";
            warn_stream_processing_failure("chat.error_event", message);
            let _ = tx
                .send(ProviderStreamEvent::categorized_error(
                    message,
                    ProviderErrorCategory::Other,
                ))
                .await;
            return;
        }

        let Some(chunk_finished) = apply_chat_chunk(
            &tx,
            chunk,
            &mut usage,
            &mut finished_metadata,
            &mut tool_call_state,
        )
        .await
        else {
            return;
        };

        finish_seen |= chunk_finished;
    };

    if !emit_tool_call_completions(&tx, &mut tool_call_state).await {
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

async fn apply_chat_chunk(
    tx: &mpsc::Sender<ProviderStreamEvent>,
    chunk: OpenAiChatCompletionsChunk,
    usage: &mut Option<CompletionUsage>,
    finished_metadata: &mut ProviderStreamFinishedMetadata,
    tool_call_state: &mut ChatToolCallState,
) -> Option<bool> {
    if let Some(id) = chunk
        .id
        .as_deref()
        .filter(|id| non_empty_string(id).is_some())
    {
        finished_metadata
            .provider_response_id
            .get_or_insert_with(|| id.to_string());
    }
    if let Some(chunk_usage) = chunk.usage {
        *usage = Some(chunk_usage.completion_usage());
        chunk_usage.merge_finished_metadata(finished_metadata);
    } else {
        // Some OpenAI-compatible providers (e.g. GLM / Zhipu) emit usage
        // inside the choice object instead of the top-level chunk.
        for choice in &chunk.choices {
            if let Some(choice_usage) = &choice.usage {
                *usage = Some(choice_usage.completion_usage());
                choice_usage.merge_finished_metadata(finished_metadata);
            }
        }
    }

    let mut finish_seen = false;
    for choice in chunk.choices {
        if !send_optional_delta(
            tx,
            choice.delta.reasoning_text,
            ProviderStreamEvent::ReasoningDelta,
        )
        .await
            || !send_chat_content(tx, choice.delta.content).await
            || !consume_tool_call_deltas(
                tx,
                choice.delta.tool_calls.as_deref().unwrap_or_default(),
                tool_call_state,
            )
            .await
        {
            return None;
        }
        if let Some(finish_reason) = choice.finish_reason {
            if !matches!(
                finish_reason.as_str(),
                "stop" | "length" | "content_filter" | "tool_calls" | "function_call"
            ) {
                let message = "openai_compatible chat stream returned an unknown finish reason";
                warn_stream_processing_failure("chat.invalid_finish_reason", message);
                let _ = tx.send(malformed_stream_error(message)).await;
                return None;
            }
            finished_metadata.provider_stop_reason = Some(finish_reason);
            finish_seen = true;
        }
    }
    Some(finish_seen)
}

async fn send_chat_content(
    tx: &mpsc::Sender<ProviderStreamEvent>,
    content: Option<ChatContent>,
) -> bool {
    let parts = match content {
        None => return true,
        Some(ChatContent::Text(text)) => {
            return send_optional_delta(tx, Some(text), ProviderStreamEvent::TextDelta).await;
        }
        Some(ChatContent::Parts(parts)) => parts,
    };
    for part in parts {
        let sent = match part {
            ChatContentPart::Text { text } => {
                send_optional_delta(tx, Some(text), ProviderStreamEvent::TextDelta).await
            }
            ChatContentPart::Thinking { thinking } => {
                let text = thinking
                    .into_iter()
                    .map(|part| part.text)
                    .collect::<String>();
                send_optional_delta(tx, Some(text), ProviderStreamEvent::ReasoningDelta).await
            }
        };
        if !sent {
            return false;
        }
    }
    true
}
