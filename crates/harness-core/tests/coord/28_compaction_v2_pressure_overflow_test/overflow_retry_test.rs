use super::*;
use harness_core::UnwrapOrAbort;

#[tokio::test]
async fn compaction_v2_single_huge_user_overflow_does_not_split_or_resend() {
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

    // When: the overflowing context consists of one indivisible user message.
    let request_id = harness
        .turn(&"single huge overflow turn ".repeat(2_000))
        .await;
    harness.stop().await;

    // Then: recovery fails without fragmenting or resending that message.
    let events = harness.events();
    let provider_attempts = events
        .iter()
        .filter(|event| {
            matches!(event.payload, EventV1::ProviderRequestStarted(_))
                && event.correlation_id.as_deref() == Some(request_id.as_str())
        })
        .count();
    assert_eq!(
        provider_attempts, 1,
        "a single user message has no earlier atomic boundary"
    );
    assert_eq!(
        session_compaction_values(&events).len(),
        0,
        "overflow must commit once"
    );
    assert_eq!(
        provider.requests().len(),
        1,
        "no summary generation or identical retry"
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
        0,
        "failed output must not commit assistant memory"
    );
}
