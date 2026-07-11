use tokio::sync::mpsc;

use crate::{ProviderStreamEvent, ProviderStreamStartMetadata};

use super::super::sse::next_sse_event;
use super::super::stream_event::{
    malformed_stream_error, non_empty_finished_metadata,
    provider_stream_finished_metadata_from_start, transport_failure_error,
};
use super::super::stream_payload::OpenAiChatCompletionsChunk;
use super::super::tool_call::{
    consume_tool_call_deltas, emit_tool_call_completions, ChatToolCallState,
};
use super::super::transport::OpenAiHttpResponse;
use super::{
    non_empty_string, warn_stream_processing_failure, warn_stream_send_failure, zero_usage,
};

pub(super) async fn consume_chat_sse_stream(
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
        warn_stream_send_failure("chat.start");
        return;
    }

    let mut usage = zero_usage();
    let mut finished_metadata = provider_stream_finished_metadata_from_start(start_metadata);
    let mut done_emitted = false;
    let mut tool_call_state = ChatToolCallState::default();
    let mut body = response.body;
    let mut sse_buffer = Vec::new();

    loop {
        let event = match next_sse_event(&mut body, &mut sse_buffer).await {
            Ok(Some(event)) => event,
            Ok(None) => break,
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
            if !emit_tool_call_completions(&tx, &mut tool_call_state).await {
                return;
            }

            if !done_emitted
                && tx
                    .send(ProviderStreamEvent::DoneWithMetadata {
                        usage,
                        metadata: non_empty_finished_metadata(finished_metadata),
                    })
                    .await
                    .is_err()
            {
                warn_stream_send_failure("chat.done");
            }
            return;
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
            usage = chunk_usage.completion_usage();
            chunk_usage.merge_finished_metadata(&mut finished_metadata);
        }

        let mut finish_seen = false;
        for choice in chunk.choices {
            if let Some(reasoning) = choice.delta.reasoning_text {
                if !reasoning.is_empty()
                    && tx
                        .send(ProviderStreamEvent::ReasoningDelta(reasoning))
                        .await
                        .is_err()
                {
                    return;
                }
            }

            if let Some(content) = choice.delta.content {
                if !content.is_empty()
                    && tx
                        .send(ProviderStreamEvent::TextDelta(content))
                        .await
                        .is_err()
                {
                    return;
                }
            }

            if !consume_tool_call_deltas(&tx, &choice.delta.tool_calls, &mut tool_call_state).await
            {
                return;
            }

            if matches!(choice.finish_reason.as_deref(), Some("tool_calls"))
                && !emit_tool_call_completions(&tx, &mut tool_call_state).await
            {
                return;
            }

            if let Some(finish_reason) = choice.finish_reason.as_deref() {
                finished_metadata.provider_stop_reason = Some(finish_reason.to_string());
                finish_seen = true;
            }
        }

        if finish_seen && !done_emitted {
            if !emit_tool_call_completions(&tx, &mut tool_call_state).await {
                return;
            }

            done_emitted = true;
            if tx
                .send(ProviderStreamEvent::DoneWithMetadata {
                    usage: usage.clone(),
                    metadata: non_empty_finished_metadata(finished_metadata.clone()),
                })
                .await
                .is_err()
            {
                warn_stream_send_failure("chat.done_after_finish_reason");
                return;
            }
        }
    }

    if !emit_tool_call_completions(&tx, &mut tool_call_state).await {
        return;
    }

    if !done_emitted
        && tx
            .send(ProviderStreamEvent::DoneWithMetadata {
                usage,
                metadata: non_empty_finished_metadata(finished_metadata),
            })
            .await
            .is_err()
    {
        warn_stream_send_failure("chat.done_after_stream_end");
    }
}
