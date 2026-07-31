use harness_core::memory::{DurableMemoryStore, MemoryScope};
use harness_core::prompt_queue::DurablePromptQueue;
use harness_core::transcript_projection::{
    project_transcript, CompactionCheckpointStatus, ProjectedPart,
};

include!("common/coord_fixtures.rs");

#[test]
fn prompt_queue_contract_preserves_interjection_reconciliation_and_post_turn_drain() {
    // Given: an active session with an immutable event history and a running turn.
    let dir = tempfile::tempdir().unwrap();
    let session_dir = dir.path().join("session");
    std::fs::create_dir_all(&session_dir).unwrap();
    let events_path = session_dir.join("events.jsonl");
    let source_events = b"{\"seq\":1,\"type\":\"run_started\"}\n";
    std::fs::write(&events_path, source_events).unwrap();
    let queue = DurablePromptQueue::for_session(&session_dir);
    queue.enqueue("queued-1", "after this turn", 1).unwrap();
    queue.enqueue("queued-2", "after the next turn", 2).unwrap();
    let interjection = queue
        .interject_mid_turn("urgent", "send this now", 3, true)
        .unwrap();

    // When: the process restarts, reconciles the mid-turn interjection, then
    // sends the next queued prompt immediately and drains the completed turn.
    let resumed = DurablePromptQueue::for_session(&session_dir);
    let listed = resumed.list().unwrap();
    let reconciled = resumed.drain_interjections().unwrap();
    let send_now = resumed.dequeue().unwrap().unwrap();
    let post_turn_drain = resumed.drain().unwrap();
    let after_drain_restart = DurablePromptQueue::for_session(&session_dir);

    // Then: queue order survives restart, interjection state stays separate,
    // send-now selects the FIFO head, and the post-turn drain empties storage.
    assert!(interjection.turn_was_running);
    assert!(!interjection.mutates_conversation_events);
    assert_eq!(
        listed
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["urgent", "queued-1", "queued-2"]
    );
    assert_eq!(
        reconciled
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["urgent"]
    );
    assert_eq!(send_now.id, "queued-1");
    assert_eq!(
        post_turn_drain
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["queued-2"]
    );
    assert!(after_drain_restart.is_empty().unwrap());
    assert_eq!(std::fs::read(&events_path).unwrap(), source_events);
}

#[test]
fn local_memory_contract_flushes_scopes_and_returns_stable_search_order_after_restart() {
    // Given: local entries in each durable memory scope.
    let dir = tempfile::tempdir().unwrap();
    let store = DurableMemoryStore::for_workspace(dir.path());
    store
        .put_scoped("alpha.global", "shared preference", MemoryScope::Global)
        .unwrap();
    store
        .put_scoped(
            "alpha.workspace",
            "project convention",
            MemoryScope::Workspace,
        )
        .unwrap();
    store
        .put_scoped("alpha.session", "current turn detail", MemoryScope::Session)
        .unwrap();

    // When: local memory is flushed and the store is reopened after restart.
    store.flush_existing().unwrap();
    let resumed = DurableMemoryStore::for_workspace(dir.path());
    let all_matches = resumed.search_scoped("alpha", None).unwrap();
    let workspace_matches = resumed
        .search_scoped("alpha", Some(MemoryScope::Workspace))
        .unwrap();
    let grouped = resumed.list_by_scope().unwrap();

    // Then: BTree-backed search ordering, scope filtering, and TUI grouping are
    // durable and deterministic without reducing entries to plain key/value data.
    assert_eq!(
        all_matches
            .iter()
            .map(|entry| entry.key.as_str())
            .collect::<Vec<_>>(),
        ["alpha.global", "alpha.session", "alpha.workspace"]
    );
    assert_eq!(workspace_matches.len(), 1);
    assert_eq!(workspace_matches[0].value, "project convention");
    assert_eq!(grouped.get(&MemoryScope::Global).unwrap().len(), 1);
    assert_eq!(grouped.get(&MemoryScope::Workspace).unwrap().len(), 1);
    assert_eq!(grouped.get(&MemoryScope::Session).unwrap().len(), 1);
}

#[tokio::test]
async fn near_threshold_compaction_appends_checkpoint_and_projects_it_without_rewriting_history() {
    // Given: two completed large turns, session-local memory, and a coordinator
    // configured to compact before an oversized third prompt.
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = temp_dir.path().join("workspace");
    let memory = DurableMemoryStore::for_workspace(&workspace);
    memory
        .put_scoped(
            "turn.fact",
            "preserve through compaction",
            MemoryScope::Session,
        )
        .unwrap();
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
        provider_text_events("checkpoint summary"),
        provider_text_events("third answer"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider),
        1,
        CompactionRuntimeConfig {
            reserve_tokens: 4_096,
            fallback_input_tokens: 12_000,
            ..CompactionRuntimeConfig::default()
        },
    );
    let run = coordinator
        .start_run("prompt_queue_compaction_memory_parity", workspace.clone())
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    for question in ["first question", "second question"] {
        let request_id = coordinator
            .request_agent_turn(supervisor_actor(), agent_id.clone(), question)
            .await
            .unwrap_or_abort();
        wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
            events.iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::TaskCompleted(_)
                        if event.correlation_id.as_deref() == Some(request_id.as_str())
                )
            })
        })
        .await;
    }
    let immutable_prefix = std::fs::read(&run.events_path).unwrap();

    // When: the next prompt crosses the configured context threshold.
    let third_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, &"C".repeat(12_000))
        .await
        .unwrap_or_abort();
    wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::TaskCompleted(_)
                    if event.correlation_id.as_deref() == Some(third_request_id.as_str())
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();
    let events = load_events(&run.events_path);
    let transcript = project_transcript(&events).unwrap_or_abort();

    // Then: compaction is an append-only, replayable checkpoint; its summary is
    // TUI-projectable, and the durable session memory has been flushed by scope.
    assert!(std::fs::read(&run.events_path)
        .unwrap()
        .starts_with(&immutable_prefix));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::SessionCompaction(payload)
                if payload.trigger_reason == "pre_prompt" && payload.summary == "checkpoint summary"
        )
    }));
    assert!(transcript
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .any(|part| {
            matches!(
                part,
                ProjectedPart::Compaction(checkpoint)
                    if checkpoint.status == CompactionCheckpointStatus::SessionCompacted
                        && checkpoint.trigger_reason.as_deref() == Some("pre_prompt")
                        && checkpoint.summary.as_deref() == Some("checkpoint summary")
            )
        }));
    assert_eq!(
        DurableMemoryStore::for_workspace(&workspace)
            .get_scoped("turn.fact")
            .unwrap()
            .unwrap()
            .scope,
        MemoryScope::Workspace
    );
}
