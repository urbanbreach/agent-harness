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
    let mut scan_offset = 0;
    loop {
        if let Some((relative_end, delimiter_len)) = sse_frame_boundary(&buffer[scan_offset..]) {
            let frame_end = scan_offset + relative_end;
            let frame = std::str::from_utf8(&buffer[..frame_end]).map_err(|err| {
                format!("openai_compatible SSE stream returned non-UTF-8 bytes: {err}")
            })?;
            let event = parse_sse_frame(frame);
            buffer.drain(..frame_end + delimiter_len);
            scan_offset = 0;
            if event.is_some() {
                return Ok(event);
            }
            continue;
        }

        // Recheck the suffix that could begin a split four-byte delimiter.
        scan_offset = buffer.len().saturating_sub(3);
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
        for (input, second) in [
            ("data: first\n\ndata: second\r\n\r\n", "second"),
            (": comment\r\n\r\ndata: first\r\rdata: second\n\n", "second"),
            ("\n\ndata: first\r\n\r\ndata: second", "second"),
            ("data: first\n\ndata: sécond\n\n", "sécond"),
        ] {
            for chunk_size in 1..=input.len() {
                // arrange: exercise delimiter and UTF-8 splits, including empty frames.
                let chunks: Vec<Result<Vec<u8>, String>> = input
                    .as_bytes()
                    .chunks(chunk_size)
                    .map(|chunk| Ok(chunk.to_vec()))
                    .collect();
                let mut body: OpenAiResponseBody = Box::pin(tokio_stream::iter(chunks));
                let mut buffer = Vec::new();

                // act
                let first_event = next_sse_event(&mut body, &mut buffer)
                    .await
                    .expect("first frame should parse");
                let second_event = next_sse_event(&mut body, &mut buffer)
                    .await
                    .expect("second frame should parse");
                let end = next_sse_event(&mut body, &mut buffer)
                    .await
                    .expect("EOF should parse");

                // assert
                assert_eq!(first_event.map(|event| event.data), Some("first".into()));
                assert_eq!(second_event.map(|event| event.data), Some(second.into()));
                assert_eq!(end, None, "chunk size {chunk_size}");
            }
        }
    }

    #[tokio::test]
    async fn next_sse_event_reuses_the_input_buffer_allocation() {
        // arrange
        let mut body: OpenAiResponseBody =
            Box::pin(tokio_stream::empty::<Result<Vec<u8>, String>>());
        let mut buffer = Vec::with_capacity(4_096);
        buffer.extend_from_slice(b"data: first\n\ndata: second\n\n");
        let initial_capacity = buffer.capacity();

        // act
        let _event = next_sse_event(&mut body, &mut buffer)
            .await
            .expect("SSE parse should succeed")
            .expect("first frame should produce an event");

        // assert
        assert_eq!(buffer.capacity(), initial_capacity);
    }
}
