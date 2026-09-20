use std::time::Duration;

use tokio::time::timeout;

use super::*;

const TEXT: &str = "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude\",\"usage\":{\"input_tokens\":8000}}}\r\n\r\ndata:{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello 🦀\"}}\n\n";
const TOOL_START: &str = "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"read_file\"}}\n\n";
const TOOL_FINISH: &str = "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\\\"/tmp/test.txt\\\"}\"}}\r\rdata: {\"type\":\"content_block_stop\",\"index\":1}\n\n";
const STOP: &str = "data: {\"type\":\"message_delta\",\ndata: \"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":840}}\n\rdata: {\"type\":\"message_stop\"}\n\n";

#[tokio::test]
async fn anthropic_stream_delivers_split_frames_before_eof_and_keeps_tool_usage() {
    for chunk_size in [1, 2, 13, usize::MAX] {
        let (chunks_tx, chunks) = mpsc::channel::<Result<Vec<u8>, String>>(1);
        let mut events = stream_response(ReceiverStream::new(chunks));
        let producer = tokio::spawn(async move {
            for chunk in TEXT.as_bytes().chunks(chunk_size) {
                chunks_tx
                    .send(Ok(chunk.to_vec()))
                    .await
                    .expect("reader is alive");
            }
            chunks_tx
        });
        assert!(matches!(
            timeout(Duration::from_secs(2), events.next()).await,
            Ok(Some(ProviderStreamEvent::Started { .. }))
        ));
        assert_eq!(
            timeout(Duration::from_secs(2), events.next()).await,
            Ok(Some(ProviderStreamEvent::TextDelta("Hello 🦀".to_string())))
        );
        let chunks_tx = producer.await.expect("producer completed");
        let rest = format!("{TOOL_START}{TOOL_FINISH}{STOP}");
        chunks_tx
            .send(Ok(rest.into_bytes()))
            .await
            .expect("reader is alive");
        let terminal = timeout(Duration::from_secs(2), events.collect::<Vec<_>>())
            .await
            .expect("message_stop completes without waiting for EOF");
        assert!(
            matches!(
                terminal.as_slice(),
                [ProviderStreamEvent::ToolCallDelta { .. },
                 ProviderStreamEvent::ToolCallComplete { arguments_json, .. },
                 ProviderStreamEvent::DoneWithMetadata {
                    usage: Some(CompletionUsage { prompt_tokens: 8000, completion_tokens: 840, total_tokens: 8840 }),
                    metadata: Some(ProviderStreamFinishedMetadata { provider_stop_reason: Some(reason), .. }),
                 }] if arguments_json == r#"{"path":"/tmp/test.txt"}"# && reason == "tool_use"
            ),
            "{terminal:?}"
        );
        timeout(Duration::from_secs(2), chunks_tx.closed())
            .await
            .expect("completed response releases its body");
    }
}

#[tokio::test]
async fn anthropic_stream_preserves_partial_output_and_fails_once_without_pending_tools() {
    for (tail, category) in [
        (Some(Err("SENSITIVE body failure".to_string())), ProviderErrorCategory::TransportFailure),
        (None, ProviderErrorCategory::MalformedStream),
        (Some(Ok(b"data: {SENSITIVE invalid JSON}\n\n".to_vec())), ProviderErrorCategory::MalformedStream),
        (Some(Ok(b"data: \xff\n\n".to_vec())), ProviderErrorCategory::MalformedStream),
        (Some(Ok(b"data: {\"type\":\"error\",\"error\":{\"message\":\"SENSITIVE\"}}\n\ndata: {\"type\":\"message_stop\"}\n\n".to_vec())), ProviderErrorCategory::TransportFailure),
        (Some(Ok(b"data: {\"type\":\"message_stop\"}".to_vec())), ProviderErrorCategory::MalformedStream),
        (Some(Ok(vec![b'x'; MAX_SSE_FRAME_BYTES + 1])), ProviderErrorCategory::MalformedStream),
    ] {
        let (chunks_tx, chunks) = mpsc::channel::<Result<Vec<u8>, String>>(1);
        let mut events = stream_response(ReceiverStream::new(chunks));
        chunks_tx.send(Ok(format!("{TEXT}{TOOL_START}").into_bytes()))
            .await.expect("reader is alive");
        let started = timeout(Duration::from_secs(2), events.next()).await;
        assert!(matches!(started, Ok(Some(ProviderStreamEvent::Started { .. }))));
        assert_eq!(
            timeout(Duration::from_secs(2), events.next()).await,
            Ok(Some(ProviderStreamEvent::TextDelta("Hello 🦀".to_string())))
        );
        if let Some(tail) = tail {
            chunks_tx.send(tail).await.expect("reader is alive");
        }
        drop(chunks_tx);
        let failed = timeout(Duration::from_secs(2), events.collect::<Vec<_>>())
            .await.expect("failed stream terminates");
        assert!(matches!(failed.as_slice(), [
            ProviderStreamEvent::ToolCallDelta { .. },
            ProviderStreamEvent::Error { category: Some(actual), message, .. },
        ] if actual == &category && !message.contains("SENSITIVE")), "{failed:?}");
    }
}

#[tokio::test]
async fn anthropic_stream_drop_releases_an_idle_body() {
    let (chunks_tx, chunks) = mpsc::channel::<Result<Vec<u8>, String>>(1);
    let mut events = stream_response(ReceiverStream::new(chunks));
    chunks_tx
        .send(Ok(TEXT.as_bytes().to_vec()))
        .await
        .expect("reader is alive");
    for _ in 0..2 {
        assert!(timeout(Duration::from_secs(2), events.next())
            .await
            .expect("partial output is delivered")
            .is_some());
    }
    drop(events);
    timeout(Duration::from_secs(2), chunks_tx.closed())
        .await
        .expect("dropping a consumer releases its silent body");
}
