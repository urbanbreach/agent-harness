use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt};

use super::{anthropic_sse_to_provider_event, parse_anthropic_sse_value, AnthropicSseEvent};
use crate::{
    CompletionUsage, ProviderErrorCategory, ProviderEventStream, ProviderStreamEvent,
    ProviderStreamFinishedMetadata,
};

const MAX_SSE_FRAME_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
struct AnthropicSseStreamState {
    tool_state: Vec<(u32, String, String, String)>,
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
    provider_stop_reason: Option<String>,
    frame: Vec<u8>,
    terminal: bool,
}

impl AnthropicSseStreamState {
    // Both readers feed line fragments, so a chunk containing many frames never
    // accumulates a whole response or a whole-response event vector.
    fn push(&mut self, fragment: &[u8]) -> Vec<ProviderStreamEvent> {
        if fragment.len() > MAX_SSE_FRAME_BYTES - self.frame.len() {
            return self.fail("Anthropic SSE frame exceeded the 16 MiB limit");
        }
        self.frame.extend_from_slice(fragment);
        if !self.frame.ends_with(b"\n\n")
            && !self.frame.ends_with(b"\n\r")
            && !self.frame.ends_with(b"\r\n\r\n")
            && !self.frame.ends_with(b"\r\r")
        {
            return Vec::new();
        }
        let event = parse_frame(&self.frame);
        self.frame.clear();
        match event {
            Ok(Some(event)) => self.normalize(&event),
            Ok(None) => Vec::new(),
            Err(message) => self.fail(message),
        }
    }

    fn fail(&mut self, message: &'static str) -> Vec<ProviderStreamEvent> {
        self.terminal = true;
        vec![ProviderStreamEvent::categorized_error(
            message,
            ProviderErrorCategory::MalformedStream,
        )]
    }

    fn finish(&mut self) -> Vec<ProviderStreamEvent> {
        if self.terminal {
            Vec::new()
        } else {
            self.fail("Anthropic SSE stream ended before message_stop")
        }
    }

    fn normalize(&mut self, event: &AnthropicSseEvent) -> Vec<ProviderStreamEvent> {
        match event {
            AnthropicSseEvent::MessageStart { input_tokens, .. } => {
                self.input_tokens = *input_tokens;
                vec![ProviderStreamEvent::Started { metadata: None }]
            }
            AnthropicSseEvent::MessageDelta {
                stop_reason,
                output_tokens,
            } => {
                self.provider_stop_reason.clone_from(stop_reason);
                self.output_tokens = *output_tokens;
                vec![]
            }
            AnthropicSseEvent::MessageStop => {
                self.terminal = true;
                let usage = self.input_tokens.zip(self.output_tokens).map(
                    |(prompt_tokens, completion_tokens)| CompletionUsage {
                        prompt_tokens,
                        completion_tokens,
                        total_tokens: prompt_tokens.saturating_add(completion_tokens),
                    },
                );
                vec![ProviderStreamEvent::DoneWithMetadata {
                    usage,
                    metadata: Some(ProviderStreamFinishedMetadata {
                        provider_stop_reason: self.provider_stop_reason.clone(),
                        ..Default::default()
                    }),
                }]
            }
            AnthropicSseEvent::Error { .. } => {
                self.terminal = true;
                vec![ProviderStreamEvent::categorized_error(
                    "Anthropic SSE stream returned an error",
                    ProviderErrorCategory::TransportFailure,
                )]
            }
            _ => anthropic_sse_to_provider_event(event, &mut self.tool_state),
        }
    }
}

fn parse_frame(frame: &[u8]) -> Result<Option<AnthropicSseEvent>, &'static str> {
    let frame =
        std::str::from_utf8(frame).map_err(|_| "Anthropic SSE frame contained invalid UTF-8")?;
    let data = frame
        .split(['\r', '\n'])
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|value| value.strip_prefix(' ').unwrap_or(value))
        .collect::<Vec<_>>();
    if data.is_empty() {
        return Ok(None);
    }
    let value: serde_json::Value = serde_json::from_str(&data.join("\n"))
        .map_err(|_| "Anthropic SSE frame contained invalid JSON")?;
    let event_type = value
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or("Anthropic SSE frame omitted its event type")?;
    let event = parse_anthropic_sse_value(&value);
    // Anthropic allows future event and content types; malformed supported events
    // still fail closed instead of silently dropping output.
    let supported = match event_type {
        "content_block_start" => !value
            .pointer("/content_block/type")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|kind| !matches!(kind, "text" | "tool_use")),
        "content_block_delta" => !value
            .pointer("/delta/type")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|kind| !matches!(kind, "text_delta" | "input_json_delta")),
        "message_start" | "content_block_stop" | "message_delta" | "message_stop" | "ping"
        | "error" => true,
        _ => false,
    };
    if supported && event.is_none() {
        Err("Anthropic SSE frame contained a malformed event")
    } else {
        Ok(event)
    }
}

pub(super) fn parse_complete(raw: &str) -> Vec<ProviderStreamEvent> {
    let mut state = AnthropicSseStreamState::default();
    let mut events = Vec::new();
    for fragment in raw
        .as_bytes()
        .split_inclusive(|byte| matches!(byte, b'\r' | b'\n'))
    {
        events.extend(state.push(fragment));
        if state.terminal {
            break;
        }
    }
    events.extend(state.finish());
    events
}

pub(super) fn stream_response<S, B, E>(body: S) -> ProviderEventStream
where
    S: Stream<Item = Result<B, E>> + Send + 'static,
    B: AsRef<[u8]> + Send + Sync + 'static,
    E: Send + 'static,
{
    let (tx, rx) = mpsc::channel(64);
    tokio::spawn(async move {
        let closed_tx = tx.clone();
        tokio::select! {
            _ = closed_tx.closed() => {}
            _ = async move {
                let mut state = AnthropicSseStreamState::default();
                tokio::pin!(body);
                while let Some(chunk) = body.next().await {
                    let Ok(chunk) = chunk else {
                        let _ = tx.send(ProviderStreamEvent::categorized_error(
                            "failed to read Anthropic SSE response body",
                            ProviderErrorCategory::TransportFailure,
                        )).await;
                        return;
                    };
                    for fragment in chunk.as_ref().split_inclusive(|byte| matches!(byte, b'\r' | b'\n')) {
                        for event in state.push(fragment) {
                            if tx.send(event).await.is_err() {
                                return;
                            }
                        }
                        if state.terminal {
                            return;
                        }
                    }
                }
                for event in state.finish() {
                    let _ = tx.send(event).await;
                }
            } => {}
        }
    });
    Box::pin(ReceiverStream::new(rx))
}

#[cfg(test)]
#[path = "stream_test.rs"]
mod tests;
