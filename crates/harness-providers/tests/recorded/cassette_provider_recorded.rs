use harness_providers::UnwrapOrAbort;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use harness_providers::cassette::{
    assert_cassette_is_safe, CassetteInteraction, CassetteMode, ProviderCassette, RecordedProvider,
};
use harness_providers::{
    CompletionMessage, CompletionRequest, MessageRole, Provider, ProviderEventStream,
    ProviderStreamEvent,
};
use tempfile::tempdir;
use tokio_stream::{self as stream, StreamExt};

#[tokio::test]
async fn replay_matches_requests_by_sequential_cursor() {
    let provider = RecordedProvider::with_ci(
        CountingProvider::default(),
        fixture_path("sequential.json"),
        CassetteMode::Replay,
        false,
    )
    .unwrap_or_abort();

    let first = provider
        .stream_completion(request("first"))
        .await
        .collect::<Vec<_>>()
        .await;
    assert_eq!(
        first,
        vec![ProviderStreamEvent::TextDelta("one".to_string())]
    );

    let second = provider
        .stream_completion(request("second"))
        .await
        .collect::<Vec<_>>()
        .await;
    assert_eq!(
        second,
        vec![ProviderStreamEvent::TextDelta("two".to_string())]
    );
}

#[tokio::test]
async fn replay_reports_clear_mismatch_without_calling_inner_provider() {
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = RecordedProvider::with_ci(
        CountingProvider::new(Arc::clone(&calls)),
        fixture_path("sequential.json"),
        CassetteMode::Replay,
        false,
    )
    .unwrap_or_abort();

    let events = provider
        .stream_completion(request("wrong"))
        .await
        .collect::<Vec<_>>()
        .await;

    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(
        matches!(&events[..], [ProviderStreamEvent::Error { message, .. }] if message.contains("cassette request mismatch at interaction 0"))
    );
}

#[tokio::test]
async fn ci_forces_replay_and_missing_cassette_fails_closed() {
    let temp = tempdir().unwrap_or_abort();
    let missing = temp.path().join("missing.json");

    let err = match RecordedProvider::with_ci(
        CountingProvider::default(),
        &missing,
        CassetteMode::Auto,
        true,
    ) {
        Ok(_) => panic!("CI must force replay instead of recording"),
        Err(err) => err,
    };

    assert!(err.to_string().contains("missing cassette"));
    assert!(!missing.exists());
}

#[tokio::test]
async fn record_mode_writes_safe_cassette_and_replays_recorded_events() {
    let temp = tempdir().unwrap_or_abort();
    let path = temp.path().join("recorded.json");
    let provider = RecordedProvider::with_ci(
        CountingProvider::default(),
        &path,
        CassetteMode::Record,
        false,
    )
    .unwrap_or_abort();

    let events = provider
        .stream_completion(request("record me"))
        .await
        .collect::<Vec<_>>()
        .await;
    assert_eq!(
        events,
        vec![ProviderStreamEvent::TextDelta("call-1".to_string())]
    );

    let cassette = ProviderCassette::read_from(&path).unwrap_or_abort();
    assert_cassette_is_safe(&cassette).unwrap_or_abort();
    assert_eq!(cassette.interactions.len(), 1);
    assert_eq!(cassette.interactions[0].request, request("record me"));
    assert_eq!(cassette.interactions[0].events, events);
}

#[tokio::test]
async fn unsafe_secret_refuses_to_write_recording() {
    let temp = tempdir().unwrap_or_abort();
    let path = temp.path().join("unsafe.json");
    let provider = RecordedProvider::with_ci(
        CountingProvider::default(),
        &path,
        CassetteMode::Record,
        false,
    )
    .unwrap_or_abort();

    let events = provider
        .stream_completion(request("please leak sk-testsecret123"))
        .await
        .collect::<Vec<_>>()
        .await;

    assert!(
        matches!(&events[..], [ProviderStreamEvent::Error { message, .. }] if message.contains("unsafe cassette secret detected"))
    );
    assert!(!path.exists(), "unsafe cassette must not be written");
}

#[derive(Debug, Clone)]
struct CountingProvider {
    calls: Arc<AtomicUsize>,
}

impl Default for CountingProvider {
    fn default() -> Self {
        Self::new(Arc::new(AtomicUsize::new(0)))
    }
}

impl CountingProvider {
    fn new(calls: Arc<AtomicUsize>) -> Self {
        Self { calls }
    }
}

#[async_trait]
impl Provider for CountingProvider {
    async fn stream_completion(&self, _req: CompletionRequest) -> ProviderEventStream {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        Box::pin(stream::iter(vec![ProviderStreamEvent::TextDelta(format!(
            "call-{call}"
        ))]))
    }
}

fn fixture_path(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cassettes")
        .join(name)
        .to_string_lossy()
        .into_owned()
}

fn request(content: &str) -> CompletionRequest {
    CompletionRequest {
        provider_id: Some("default".to_string()),
        model_id: "test-model".to_string(),
        messages: vec![CompletionMessage {
            role: MessageRole::User,
            content: content.to_string(),
            name: None,
            tool_call_id: None,
            assistant_tool_calls: None,
        }],
        temperature: Some(0.0),
        max_tokens: None,
        variant: None,
        reasoning_effort: None,
        text_verbosity: None,
        reasoning_summary: None,
        thinking: None,
        tools: None,
        tool_choice: None,
        context: Default::default(),
        stream: true,
    }
}

fn interaction(content: &str, delta: &str) -> CassetteInteraction {
    CassetteInteraction::new(
        request(content),
        vec![ProviderStreamEvent::TextDelta(delta.to_string())],
    )
}
