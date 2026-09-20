use std::{
    pin::Pin,
    task::{Context, Poll},
};

use tokio::sync::{mpsc, oneshot};
use tokio_stream::Stream;

use super::*;

struct ControlledBody {
    chunks: mpsc::Receiver<Result<Vec<u8>, String>>,
    polled: Option<oneshot::Sender<()>>,
    dropped: Option<oneshot::Sender<()>>,
}

impl Stream for ControlledBody {
    type Item = Result<Vec<u8>, String>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(polled) = self.polled.take() {
            let _ = polled.send(());
        }
        self.chunks.poll_recv(cx)
    }
}

impl Drop for ControlledBody {
    fn drop(&mut self) {
        if let Some(dropped) = self.dropped.take() {
            let _ = dropped.send(());
        }
    }
}

struct ControlledTransport(Mutex<Option<ControlledBody>>);

#[async_trait]
impl OpenAiHttpTransport for ControlledTransport {
    async fn post_json(
        &self,
        _endpoint: String,
        _headers: HeaderMap,
        _bearer_token: String,
        _body: serde_json::Value,
    ) -> Result<OpenAiHttpResponse, String> {
        Ok(OpenAiHttpResponse::new(
            200,
            HeaderMap::new(),
            Box::pin(self.0.lock().unwrap_or_abort().take().unwrap_or_abort()),
        ))
    }
}

#[tokio::test]
async fn stream_cancellation_drops_polled_body_and_preserves_completion() {
    let mut retained_bodies = Vec::new();
    for (api_mode, transcript, expected_usage) in [
        (
            OpenAiApiMode::ChatCompletions,
            deterministic_sse_transcript(),
            CompletionUsage {
                prompt_tokens: 4,
                completion_tokens: 2,
                total_tokens: 6,
            },
        ),
        (
            OpenAiApiMode::Responses,
            responses_done_sse_transcript(),
            CompletionUsage {
                prompt_tokens: 1,
                completion_tokens: 1,
                total_tokens: 2,
            },
        ),
    ] {
        for cancel in [false, true] {
            let (chunks_tx, chunks) = mpsc::channel(1);
            let (polled_tx, polled) = oneshot::channel();
            let (dropped_tx, dropped) = oneshot::channel();
            let provider = OpenAiCompatibleProvider::with_transport(
                OpenAiCompatibleProviderConfig {
                    base_url: "http://127.0.0.1/v1".to_string(),
                    api_key: "test-secret-key".to_string(),
                    api_mode,
                    timeout_ms: 15_000,
                    headers: BTreeMap::new(),
                },
                Arc::new(ControlledTransport(Mutex::new(Some(ControlledBody {
                    chunks,
                    polled: Some(polled_tx),
                    dropped: Some(dropped_tx),
                })))),
            )
            .unwrap_or_abort();

            let stream = provider
                .stream_completion(basic_request("test-model"))
                .await;
            assert_eq!(
                timeout(Duration::from_secs(1), polled).await,
                Ok(Ok(())),
                "{api_mode:?}: reader must be polling the idle body"
            );
            if cancel {
                drop(stream);
            } else {
                chunks_tx
                    .send(Ok(transcript.as_bytes().to_vec()))
                    .await
                    .unwrap_or_abort();
                let events = timeout(Duration::from_secs(1), stream.collect::<Vec<_>>())
                    .await
                    .unwrap_or_abort();
                assert!(
                    matches!(events.last(), Some(ProviderStreamEvent::DoneWithMetadata {
                        usage: Some(usage), ..
                    }) if usage == &expected_usage),
                    "{api_mode:?}: completion and usage must survive: {events:?}"
                );
            }
            if timeout(Duration::from_secs(1), dropped).await != Ok(Ok(())) {
                retained_bodies.push((api_mode, cancel));
            }
            // Keep the HTTP body pending until after the drop assertion.
            drop(chunks_tx);
        }
    }
    assert!(
        retained_bodies.is_empty(),
        "retained bodies: {retained_bodies:?}"
    );
}
