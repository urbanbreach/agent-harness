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
        if let Some((frame, remaining)) = split_sse_frame(buffer)? {
            *buffer = remaining;
            if let Some(event) = parse_sse_frame(&frame) {
                return Ok(Some(event));
            }
            continue;
        }

        let Some(chunk) = body.next().await else {
            if buffer.is_empty() {
                return Ok(None);
            }
            let frame = String::from_utf8(std::mem::take(buffer)).map_err(|err| {
                format!("openai_compatible SSE stream returned non-UTF-8 bytes: {err}")
            })?;
            return Ok(parse_sse_frame(&frame));
        };
        buffer.extend_from_slice(&chunk?);
    }
}

fn split_sse_frame(buffer: &[u8]) -> Result<Option<(String, Vec<u8>)>, String> {
    for delimiter in [
        b"\r\n\r\n".as_slice(),
        b"\n\n".as_slice(),
        b"\r\r".as_slice(),
    ] {
        if let Some(index) = buffer
            .windows(delimiter.len())
            .position(|window| window == delimiter)
        {
            let frame = String::from_utf8(buffer[..index].to_vec()).map_err(|err| {
                format!("openai_compatible SSE stream returned non-UTF-8 bytes: {err}")
            })?;
            let remaining = buffer[index + delimiter.len()..].to_vec();
            return Ok(Some((frame, remaining)));
        }
    }
    Ok(None)
}

fn parse_sse_frame(frame: &str) -> Option<SseEvent> {
    let data = frame
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|value| value.strip_prefix(' ').unwrap_or(value))
        .collect::<Vec<_>>()
        .join("\n");
    (!data.is_empty()).then_some(SseEvent { data })
}
