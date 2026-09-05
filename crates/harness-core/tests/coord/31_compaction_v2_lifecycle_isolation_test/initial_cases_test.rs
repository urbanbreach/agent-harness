#[tokio::test]
async fn compaction_v2_root_child_histories_isolated() {
    // Given: root and child agents with distinct sentinels and complete histories.
    let (harness, provider) = CompactionV2Harness::scripted(
        vec![
            provider_text_events("ROOT_ANSWER_ONE"),
            provider_text_events("ROOT_ANSWER_TWO"),
            provider_text_events("ROOT_SUMMARY_ONLY"),
            provider_text_events("CHILD_ANSWER_ONE"),
            provider_text_events("CHILD_ANSWER_TWO"),
            provider_text_events("CHILD_SUMMARY_ONLY"),
        ],
        CompactionRuntimeConfig::default(),
    )
    .await;
    harness.turn("ROOT_SENTINEL_ONE").await;
    harness.turn("ROOT_SENTINEL_TWO").await;
    harness
        .coordinator
        .compact_agent_context(harness.agent_id.clone(), None, "manual")
        .await
        .unwrap_or_abort();
    let child = harness
        .coordinator
        .spawn_agent_idle(supervisor_actor(), "beta", Some(harness.agent_id.clone()))
        .await
        .unwrap_or_abort();
    agent_turn(&harness.coordinator, &child, "CHILD_SENTINEL_ONE").await;
    agent_turn(&harness.coordinator, &child, "CHILD_SENTINEL_TWO").await;

    // When: the child history is compacted independently.
    harness
        .coordinator
        .compact_agent_context(child, None, "manual")
        .await
        .unwrap_or_abort();
    harness.stop().await;

    // Then: the actual summary requests and typed first-kept identities do not cross owners.
    let requests = provider.requests();
    let root_summary_request = serde_json::to_string(&requests[2]).unwrap_or_abort();
    let child_summary_request = serde_json::to_string(&requests[5]).unwrap_or_abort();
    assert!(root_summary_request.contains("ROOT_SENTINEL"));
    assert!(!root_summary_request.contains("CHILD_SENTINEL"));
    assert!(child_summary_request.contains("CHILD_SENTINEL"));
    assert!(!child_summary_request.contains("ROOT_SENTINEL"));
    let payloads = session_compaction_values(&harness.events());
    assert_eq!(payloads.len(), 2);
    assert!(!payloads[0]["summary"]
        .as_str()
        .unwrap_or_default()
        .contains("CHILD_SENTINEL"));
    assert!(!payloads[1]["summary"]
        .as_str()
        .unwrap_or_default()
        .contains("ROOT_SENTINEL"));
    let root_entry_id = payloads[0]
        .get("first_kept_entry_id")
        .and_then(serde_json::Value::as_str);
    let child_entry_id = payloads[1]
        .get("first_kept_entry_id")
        .and_then(serde_json::Value::as_str);
    assert!(root_entry_id.is_some() && child_entry_id.is_some());
    assert_ne!(
        root_entry_id, child_entry_id,
        "root and child boundaries require distinct owner-scoped EntryIds"
    );
}

#[tokio::test]
async fn compaction_v2_lifecycle_command_loop_remains_responsive() {
    // Given: summary generation blocked after its subscribed entry signal.
    let (harness, _provider, entered, release) = lifecycle_harness().await;
    let compaction = support::spawn_compaction(&harness);
    tokio::time::timeout(Duration::from_secs(1), entered)
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();

    // When: an unrelated command arrives while generation is in flight.
    let response = tokio::time::timeout(
        Duration::from_millis(100),
        harness
            .coordinator
            .spawn_agent_idle(supervisor_actor(), "beta", None),
    )
    .await;
    release.notify_waiters();
    let _ = compaction.await.unwrap_or_abort();
    harness.stop().await;

    // Then: coordinator authority remains available during provider work.
    assert!(
        matches!(response, Ok(Ok(_))),
        "command loop blocked on summary generation"
    );
}

#[tokio::test]
async fn compaction_v2_lifecycle_rejects_duplicate_generation_without_mutation() {
    // Given: one same-agent generation already in flight.
    let (harness, _provider, entered, release) = lifecycle_harness().await;
    let first = support::spawn_compaction(&harness);
    tokio::time::timeout(Duration::from_secs(1), entered)
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();
    let events_before = harness.events();
    let boundary_before = active_compaction_boundary(&events_before, &harness.agent_id);
    let event_count_before = events_before.len();
    let journal_hash_before = journal_hash(&harness.run);

    // When: a duplicate generation is requested for the same agent.
    let duplicate = tokio::time::timeout(
        Duration::from_millis(100),
        harness
            .coordinator
            .compact_agent_context(harness.agent_id.clone(), None, "manual"),
    )
    .await;
    let events_during = harness.events();
    let boundary_during = active_compaction_boundary(&events_during, &harness.agent_id);
    let event_count_during = events_during.len();
    let journal_hash_during = journal_hash(&harness.run);
    release.notify_waiters();
    let _ = first.await.unwrap_or_abort();
    harness.stop().await;

    // Then: it returns a typed rejection promptly without mutation.
    assert!(
        matches!(
            &duplicate,
            Ok(Err(CoordinatorError::CompactionInProgress { agent_id }))
                if agent_id == &harness.agent_id
        ),
        "duplicate generation did not return the typed in-progress error"
    );
    assert_eq!(boundary_during, boundary_before);
    assert_eq!(event_count_during, event_count_before);
    assert_eq!(journal_hash_during, journal_hash_before);
}

#[tokio::test]
async fn compaction_v2_lifecycle_other_agent_progresses_during_generation() {
    // Given: root generation is blocked and another agent already exists.
    let (harness, provider, entered, release) = lifecycle_harness().await;
    let other = harness
        .coordinator
        .spawn_agent_idle(supervisor_actor(), "beta", None)
        .await
        .unwrap_or_abort();
    let compaction = support::spawn_compaction(&harness);
    tokio::time::timeout(Duration::from_secs(1), entered)
        .await
        .unwrap_or_abort()
        .unwrap_or_abort();

    // When: the other agent requests a provider turn.
    let progress = tokio::time::timeout(
        Duration::from_millis(100),
        agent_turn(&harness.coordinator, &other, "other agent progresses"),
    )
    .await;
    release.notify_waiters();
    let _ = compaction.await.unwrap_or_abort();
    harness.stop().await;

    // Then: its provider request completes independently.
    assert!(
        progress.is_ok(),
        "same run's other agent was blocked by summary generation"
    );
    assert!(provider.requests().len() >= 4);
}
