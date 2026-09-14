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
        .compact_agent_context_with_instructions(harness.agent_id.clone(), None, "manual", Some("Preserve the rollback procedure".to_string()))
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
    assert!(root_summary_request.contains("Preserve the rollback procedure"));
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
    tokio::time::timeout(Duration::from_millis(100), harness.coordinator.cancel_compaction(harness.agent_id.clone())).await.unwrap_or_abort().unwrap_or_abort();
    let cancelled = tokio::time::timeout(Duration::from_millis(100), compaction).await.unwrap_or_abort().unwrap_or_abort();
    assert!(matches!(cancelled, Err(CoordinatorError::CompactionCancelled { .. })));
    release.notify_waiters();
    assert!(session_compaction_values(&harness.events()).is_empty());
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

#[tokio::test]
async fn compaction_warm_summary_rebases_appended_turn_and_commits_once() {
    use harness_core::event::{LiveEventV1, RuntimeEvent};
    let first_answer = "retained answer ".repeat(2_000);
    let (provider, entered, release) = BlockingSummaryProvider::new(vec![
        vec![ProviderStreamEvent::Start, ProviderStreamEvent::TextDelta(first_answer.clone()),
            ProviderStreamEvent::Done { usage: Some(CompletionUsage { prompt_tokens: 6_000, completion_tokens: 8_000, total_tokens: 14_000 }) }],
        provider_text_events("<task-intent>Keep the original task</task-intent><summary>warm checkpoint</summary>"),
        provider_text_events("appended answer"),
        provider_text_events("continued answer"),
    ], 1);
    let harness = CompactionV2Harness::with_provider(Arc::new(provider.clone()), CompactionRuntimeConfig {
        keep_recent_tokens: 2_000, fallback_input_tokens: 32_000, reserve_tokens: 4_096, ..Default::default()
    }).await;
    harness.turn(&"original task ".repeat(1_800)).await;
    let store = harness.coordinator.event_store().await.unwrap_or_abort();
    let runtime = store.subscribe_runtime(1).unwrap_or_abort();
    let outcome = harness.coordinator.compact_agent_context(harness.agent_id.clone(), None, "threshold").await.unwrap_or_abort();
    assert_eq!(outcome, ManualCompactionOutcome::NoOp);
    tokio::time::timeout(Duration::from_secs(1), entered).await.unwrap_or_abort().unwrap_or_abort();
    harness.turn("appended task").await;
    assert!(session_compaction_values(&harness.events()).is_empty());
    release.notify_waiters();
    let previews = runtime.filter_map(|event| match event.unwrap_or_abort() {
        RuntimeEvent::Live(event) => Some(event.payload),
        _ => None,
    }).filter_map(|payload| match payload {
        LiveEventV1::CompactionProgress { preview, .. } => Some(preview),
        _ => None,
    }).take_while(Option::is_some).collect::<Vec<_>>();
    let previews = tokio::time::timeout(Duration::from_secs(1), previews).await.unwrap_or_abort();
    assert!(previews.iter().flatten().any(|text| text.contains("warm checkpoint")));
    let applied = harness.coordinator.compact_agent_context(harness.agent_id.clone(), None, "manual").await.unwrap_or_abort();
    assert!(matches!(applied, ManualCompactionOutcome::Compacted { .. }));
    assert_eq!(provider.requests().len(), 3, "reuse must not generate another summary");
    harness.turn("continue after compaction").await;
    let requests = provider.requests();
    let resumed = &requests.last().unwrap_or_abort().messages;
    for text in ["warm checkpoint", "appended task", "appended answer"] {
        assert_eq!(resumed.iter().filter(|message| message.content.contains(text)).count(), 1);
    }
    harness.stop().await;
    let values = session_compaction_values(&harness.events());
    assert_eq!(values.len(), 1);
    assert_eq!(values[0]["task_intent"], "Keep the original task");
    assert!(!std::fs::read_to_string(&harness.run.events_path).unwrap_or_abort().contains("CompactionProgress"));
}
