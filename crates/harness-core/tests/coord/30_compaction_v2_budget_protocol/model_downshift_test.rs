#[tokio::test]
async fn compaction_v2_model_downshift_regenerates_summary() {
    // arrange
    // act
    // assert
    let (provider, entered, release) = BlockingSummaryProvider::new(
        vec![
            provider_text_events("model source one"),
            provider_text_events("model source two"),
            provider_text_events("obsolete model summary"),
            provider_text_events("small model answer"),
        ],
        2,
    );
    let harness = CompactionV2Harness::with_provider(
        Arc::new(provider),
        CompactionRuntimeConfig::default(),
    )
    .await;
    harness.turn("model turn one").await;
    harness.turn("model turn two").await;
    let boundary_before = active_compaction_boundary(&harness.events(), &harness.agent_id);
    let coordinator = harness.coordinator.clone();
    let agent_id = harness.agent_id.clone();
    let stale_generation = tokio::spawn(async move {
        coordinator
            .compact_agent_context(agent_id, None, "manual")
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), entered)
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();
    let store = harness.coordinator.event_store().await.unwrap_or_abort();
    let mut events = store.subscribe(1).unwrap_or_abort();

    let request_id = harness
        .coordinator
        .request_agent_turn_with_model_target(
            supervisor_actor(),
            harness.agent_id.clone(),
            "small model turn",
            compaction_v2_target("model-small", 30_000, 500),
        )
        .await
        .unwrap_or_abort();
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let event = events.next().await.unwrap_or_abort().unwrap_or_abort();
            if event.correlation_id.as_deref() == Some(request_id.as_str())
                && matches!(
                    &event.payload,
                    EventV1::ProviderRequestStarted(started) if started.model_id == "model-small"
                )
            {
                break;
            }
        }
    })
    .await
    .unwrap_or_abort();
    release.notify_waiters();
    let stale_result = stale_generation.await.unwrap_or_abort();
    harness.stop().await;

    assert!(matches!(
        stale_result,
        Err(CoordinatorError::CompactionStale { ref agent_id })
            if agent_id == &harness.agent_id
    ));
    assert_eq!(
        active_compaction_boundary(&harness.events(), &harness.agent_id),
        boundary_before,
    );
}
