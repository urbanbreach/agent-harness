use super::*;
use harness_core::UnwrapOrAbort;

#[tokio::test]
async fn compaction_v2_failed_or_cancelled_generation_preserves_boundary() {
    // Given: one active boundary followed by a failed replacement generation.
    let (failed, _) = CompactionV2Harness::scripted(
        vec![
            provider_text_events("failed one"),
            provider_text_events("failed two"),
            provider_text_events("stable failed-case boundary"),
            provider_text_events("failed three"),
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::error("summary failed"),
            ],
        ],
        CompactionRuntimeConfig::default(),
    )
    .await;
    failed.turn("failed boundary turn one").await;
    failed.turn("failed boundary turn two").await;
    failed
        .coordinator
        .compact_agent_context(failed.agent_id.clone(), None, "manual")
        .await
        .unwrap_or_abort();
    failed.turn("failed boundary turn three").await;
    let failed_before = active_compaction_boundary(&failed.events(), &failed.agent_id);

    // When: replacement generation fails and the run later finishes normally.
    let failed_result = failed
        .coordinator
        .compact_agent_context(failed.agent_id.clone(), None, "manual")
        .await;
    failed.stop().await;
    let failed_events = failed.events();
    let failed_after = active_compaction_boundary(&failed_events, &failed.agent_id);

    // Then: failure events and RunFinished are allowed, but the active boundary is unchanged.
    assert!(failed_result.is_err());
    assert!(failed_events
        .iter()
        .any(|event| matches!(event.payload, EventV1::RunFinished(_))));
    assert_eq!(failed_after, failed_before);

    // Given: a separate active boundary and a blocked replacement generation.
    let (provider, entered, release) = BlockingSummaryProvider::new(
        vec![
            provider_text_events("cancel one"),
            provider_text_events("cancel two"),
            provider_text_events("stable cancelled-case boundary"),
            provider_text_events("cancel three"),
            provider_text_events("late cancelled replacement"),
        ],
        4,
    );
    let cancelled =
        CompactionV2Harness::with_provider(Arc::new(provider), CompactionRuntimeConfig::default())
            .await;
    cancelled.turn("cancel boundary turn one").await;
    cancelled.turn("cancel boundary turn two").await;
    cancelled
        .coordinator
        .compact_agent_context(cancelled.agent_id.clone(), None, "manual")
        .await
        .unwrap_or_abort();
    cancelled.turn("cancel boundary turn three").await;
    let cancelled_before = active_compaction_boundary(&cancelled.events(), &cancelled.agent_id);
    let replacement = spawn_compaction(&cancelled);
    tokio::time::timeout(Duration::from_secs(1), entered)
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();

    // When: stop cancellation is subscribed before releasing the provider.
    let stop =
        tokio::time::timeout(Duration::from_millis(100), cancelled.coordinator.stop_run()).await;
    release.notify_waiters();
    let _ = replacement.await.unwrap_or_abort();
    let cancelled_events = cancelled.events();
    let cancelled_after = active_compaction_boundary(&cancelled_events, &cancelled.agent_id);

    // Then: lifecycle completion may append, but no replacement compaction may become active.
    assert!(stop.is_ok(), "stop must cancel blocked summary generation");
    assert!(cancelled_events
        .iter()
        .any(|event| matches!(event.payload, EventV1::RunFinished(_))));
    assert_eq!(cancelled_after, cancelled_before);
}
