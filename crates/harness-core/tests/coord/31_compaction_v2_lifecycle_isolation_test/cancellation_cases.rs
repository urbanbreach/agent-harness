#[tokio::test]
async fn compaction_v2_lifecycle_cancel_then_late_result_is_inert() {
    // Given: a blocked generation and its exact pre-generation journal hash.
    let (harness, _provider, entered, release) = lifecycle_harness().await;
    let boundary_before = active_compaction_boundary(&harness.events(), &harness.agent_id);
    let compaction = support::spawn_compaction(&harness);
    tokio::time::timeout(Duration::from_secs(1), entered)
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();

    // When: stop cancellation is requested before provider release.
    let stop =
        tokio::time::timeout(Duration::from_millis(100), harness.coordinator.stop_run()).await;
    let events_at_stop = harness.events();
    let event_count_at_stop = events_at_stop.len();
    let journal_hash_at_stop = journal_hash(&harness.run);
    release.notify_waiters();
    let compaction_result = compaction.await.unwrap_or_abort();

    let events = harness.events();
    let boundary_after = active_compaction_boundary(&events, &harness.agent_id);

    // Then: RunFinished is allowed, but the released late result is fully inert.
    assert!(
        stop.is_ok(),
        "stop must cancel generation without waiting for provider release"
    );
    assert!(events
        .iter()
        .any(|event| matches!(event.payload, EventV1::RunFinished(_))));
    assert!(matches!(
        &compaction_result,
        Err(CoordinatorError::CompactionCancelled { agent_id, .. })
            if agent_id == &harness.agent_id
    ));
    assert_eq!(boundary_after, boundary_before);
    assert_eq!(events.len(), event_count_at_stop);
    assert_eq!(journal_hash(&harness.run), journal_hash_at_stop);
}

#[tokio::test]
async fn compaction_v2_lifecycle_unrelated_agent_boundary_does_not_stale_completion() {
    // Given: populated root/child histories and a blocked root generation.
    let (provider, entered, release) = BlockingSummaryProvider::new(
        vec![
            provider_text_events("root old one"),
            provider_text_events("root old two"),
            provider_text_events("child new one"),
            provider_text_events("child new two"),
            provider_text_events("current root summary"),
            provider_text_events("newer child boundary"),
        ],
        4,
    );
    let harness =
        CompactionV2Harness::with_provider(Arc::new(provider), CompactionRuntimeConfig::default())
            .await;
    let child = harness
        .coordinator
        .spawn_agent_idle(supervisor_actor(), "beta", Some(harness.agent_id.clone()))
        .await
        .unwrap_or_abort();
    harness.turn("root old turn one").await;
    harness.turn("root old turn two").await;
    agent_turn(&harness.coordinator, &child, "child new turn one").await;
    agent_turn(&harness.coordinator, &child, "child new turn two").await;
    let root_before = active_compaction_boundary(&harness.events(), &harness.agent_id);
    let root_compaction = support::spawn_compaction(&harness);
    tokio::time::timeout(Duration::from_secs(1), entered)
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();

    // When: another owner commits its independent boundary before the root result is released.
    let newer_result = tokio::time::timeout(
        Duration::from_secs(1),
        harness
            .coordinator
            .compact_agent_context(child.clone(), None, "manual"),
    )
    .await;
    let child_at_release = active_compaction_boundary(&harness.events(), &child);
    release.notify_waiters();
    let root_result = root_compaction.await.unwrap_or_abort();
    harness.stop().await;

    // Then: the child activity does not stale the unchanged root durable tail.
    assert!(matches!(
        newer_result,
        Ok(Ok(ManualCompactionOutcome::Compacted { .. }))
    ));
    assert_eq!(
        child_at_release.count, 1,
        "newer boundary must commit before stale release"
    );
    assert!(matches!(
        root_result,
        Ok(ManualCompactionOutcome::Compacted { .. })
    ));
    let events = harness.events();
    assert_eq!(
        active_compaction_boundary(&events, &harness.agent_id).count,
        root_before.count + 1
    );
    assert_eq!(
        active_compaction_boundary(&events, &child),
        child_at_release
    );
}

#[tokio::test]
async fn compaction_v2_lifecycle_stale_completion_preserves_newer_boundary() {
    // Given: a blocked generation prepared from a completed same-agent history.
    let (provider, entered, release) = BlockingSummaryProvider::new(
        vec![
            provider_text_events("root old one"),
            provider_text_events("root old two"),
            provider_text_events("stale root summary"),
        ],
        2,
    );
    let harness =
        CompactionV2Harness::with_provider(Arc::new(provider), CompactionRuntimeConfig::default())
            .await;
    harness.turn("root old turn one").await;
    harness.turn("root old turn two").await;
    let root_before = active_compaction_boundary(&harness.events(), &harness.agent_id);
    let stale_root = support::spawn_compaction(&harness);
    tokio::time::timeout(Duration::from_secs(1), entered)
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();

    // When: the same agent appends a durable event without changing its provider-context cache.
    harness
        .coordinator
        .spawn_agent_idle(
            EventActor::new(ActorKind::Supervisor, Some(harness.agent_id.clone())),
            "beta",
            None,
        )
        .await
        .unwrap_or_abort();
    release.notify_waiters();
    let stale_result = stale_root.await.unwrap_or_abort();
    harness.stop().await;

    // Then: the late result is stale and cannot replace the active boundary.
    assert!(matches!(
        &stale_result,
        Err(CoordinatorError::CompactionStale { agent_id })
            if agent_id == &harness.agent_id
    ));
    assert_eq!(
        active_compaction_boundary(&harness.events(), &harness.agent_id),
        root_before
    );
}
