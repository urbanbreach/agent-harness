use super::*;
use harness_core::UnwrapOrAbort;

#[tokio::test]
async fn compaction_v2_unexpected_overflow_retries_once() {
    // Given: the first provider attempt overflows on one huge current text entry.
    let (harness, provider) = CompactionV2Harness::scripted(
        vec![
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::error("unexpected context overflow"),
            ],
            provider_text_events("overflow summary"),
            provider_text_events("retry answer"),
        ],
        CompactionRuntimeConfig::default(),
    )
    .await;

    // When: overflow recovery must split/compact and retry that same turn.
    let request_id = harness
        .turn(&"single huge overflow turn ".repeat(2_000))
        .await;
    harness.stop().await;

    // Then: there are two physical attempts and exactly one compaction success.
    let events = harness.events();
    let provider_attempts = events
        .iter()
        .filter(|event| {
            matches!(event.payload, EventV1::ProviderRequestStarted(_))
                && event.correlation_id.as_deref() == Some(request_id.as_str())
        })
        .count();
    assert_eq!(
        provider_attempts, 2,
        "overflow must perform exactly one retry"
    );
    assert_eq!(
        session_compaction_values(&events).len(),
        1,
        "overflow must commit once"
    );
    assert_eq!(
        provider.requests().len(),
        3,
        "attempt, summary generation, retry"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.payload, EventV1::ToolCallFinished(_)))
            .count(),
        0,
        "overflow retry must not execute tools"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event.payload, EventV1::AssistantMessageFinished(_)))
            .count(),
        1,
        "only the successful retry may commit assistant memory"
    );
}
