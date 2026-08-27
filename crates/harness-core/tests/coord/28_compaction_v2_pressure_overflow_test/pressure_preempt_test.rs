use super::*;
use harness_core::UnwrapOrAbort;

#[tokio::test]
async fn compaction_v2_long_session_preempts_overflow() {
    // arrange
    // act
    // assert
    // Given: two large completed turns and one pressured pending turn.
    let (harness, provider) = CompactionV2Harness::scripted(
        vec![
            provider_text_events(&"A".repeat(12_000)),
            provider_text_events(&"B".repeat(12_000)),
            provider_text_events("pressure summary"),
            provider_text_events("pressure split prefix"),
            provider_text_events("bounded answer"),
        ],
        CompactionRuntimeConfig {
            reserve_tokens: 4_096,
            fallback_input_tokens: 12_000,
            ..CompactionRuntimeConfig::default()
        },
    )
    .await;
    harness.turn("first pressure turn").await;
    harness.turn("second pressure turn").await;

    // When: the third turn requires proactive compaction.
    let request_id = harness.turn(&"C".repeat(12_000)).await;
    harness.stop().await;

    // Then: one summary commit precedes the first pressured provider dispatch.
    let events = harness.events();
    let compacted = events
        .iter()
        .find(|event| matches!(event.payload, EventV1::SessionCompaction(_)))
        .unwrap_or_abort();
    let started = events
        .iter()
        .find(|event| {
            matches!(event.payload, EventV1::ProviderRequestStarted(_))
                && event.correlation_id.as_deref() == Some(request_id.as_str())
        })
        .unwrap_or_abort();
    assert!(
        compacted.seq < started.seq,
        "summary commit must precede pressured dispatch"
    );
    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        5,
        "the pressured turn dispatch remains single-shot"
    );
    let compaction = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::SessionCompaction(payload) => Some(payload),
            _ => None,
        })
        .unwrap_or_abort();
    assert!(compaction.summary.contains("pressure summary"));
    assert!(compaction.summary.contains("pressure split prefix"));
}
