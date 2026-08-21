use tokio_stream::StreamExt;

use super::OpenAiResponseBody;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SseEvent {
    pub(super) data: String,
}

pub(super) async fn collect_body_text(mut body: OpenAiResponseBody) -> Result<String, String> {
    let mut bytes = Vec::new();
    while let Some(chunk) = body.next().await {
        bytes.extend_from_slice(&chunk?);
    }
    String::from_utf8(bytes)
        .map_err(|err| format!("openai_compatible response body was not valid UTF-8: {err}"))
}

pub(super) async fn next_sse_event(
    body: &mut OpenAiResponseBody,
    buffer: &mut Vec<u8>,
) -> Result<Option<SseEvent>, String> {
    loop {
        if let Some((frame_end, delimiter_len)) = sse_frame_boundary(buffer) {
            let frame = std::str::from_utf8(&buffer[..frame_end]).map_err(|err| {
                format!("openai_compatible SSE stream returned non-UTF-8 bytes: {err}")
            })?;
            let event = parse_sse_frame(frame);
            buffer.drain(..frame_end + delimiter_len);
            if event.is_some() {
                return Ok(event);
            }
            continue;
        }

        let Some(chunk) = body.next().await else {
            if buffer.is_empty() {
                return Ok(None);
            }
            let frame = std::str::from_utf8(buffer).map_err(|err| {
                format!("openai_compatible SSE stream returned non-UTF-8 bytes: {err}")
            })?;
            let event = parse_sse_frame(frame);
            buffer.clear();
            return Ok(event);
        };
        buffer.extend_from_slice(&chunk?);
    }
}

fn sse_frame_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    for index in 0..buffer.len() {
        match buffer[index..] {
            [b'\r', b'\n', b'\r', b'\n', ..] => return Some((index, 4)),
            [b'\n', b'\n', ..] | [b'\r', b'\r', ..] => return Some((index, 2)),
            _ => {}
        }
    }
    None
}

fn parse_sse_frame(frame: &str) -> Option<SseEvent> {
    let mut data = String::with_capacity(frame.len());
    let mut saw_data = false;
    for line in frame.lines() {
        let Some(value) = line.strip_prefix("data:") else {
            continue;
        };
        if saw_data {
            data.push('\n');
        }
        data.push_str(value.strip_prefix(' ').unwrap_or(value));
        saw_data = true;
    }
    (!data.is_empty()).then_some(SseEvent { data })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn next_sse_event_uses_the_earliest_mixed_delimiter() {
        let mut body: OpenAiResponseBody =
            Box::pin(tokio_stream::empty::<Result<Vec<u8>, String>>());
        let mut buffer = b"data: first\n\ndata: second\r\n\r\n".to_vec();

        let event = next_sse_event(&mut body, &mut buffer)
            .await
            .expect("SSE parse should succeed")
            .expect("first frame should produce an event");

        assert_eq!(event.data, "first");
    }

    #[tokio::test]
    async fn next_sse_event_reuses_the_input_buffer_allocation() {
        let mut body: OpenAiResponseBody =
            Box::pin(tokio_stream::empty::<Result<Vec<u8>, String>>());
        let mut buffer = Vec::with_capacity(4_096);
        buffer.extend_from_slice(b"data: first\n\ndata: second\n\n");
        let initial_capacity = buffer.capacity();

        let _event = next_sse_event(&mut body, &mut buffer)
            .await
            .expect("SSE parse should succeed")
            .expect("first frame should produce an event");

        assert_eq!(buffer.capacity(), initial_capacity);
    }
}
