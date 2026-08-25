use super::*;

#[tokio::test]
async fn compaction_v2_automatic_suppression_uses_provider_and_event_counts() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let current_prompt = "C".repeat(12_000);
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
        provider_text_events("unexpected summary request"),
        provider_text_events("unexpected split-turn request"),
        provider_text_events("third answer"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            reserve_tokens: 4_096,
            fallback_input_tokens: 12_000,
            suppress_auto_compaction: true,
            ..CompactionRuntimeConfig::default()
        },
    );
    let run = coordinator
        .start_run(
            "compaction_v2_suppression",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let store = coordinator.event_store().await.unwrap_or_abort();
    let mut subscription = store.subscribe(1).unwrap_or_abort();

    for prompt in ["first question", "second question", current_prompt.as_str()] {
        coordinator
            .request_agent_turn(supervisor_actor(), agent_id.clone(), prompt)
            .await
            .unwrap_or_abort();
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut completed_turns = 0;
        while completed_turns < 3 {
            let event = subscription
                .next()
                .await
                .unwrap_or_abort()
                .unwrap_or_abort();
            if matches!(event.payload, EventV1::TaskCompleted(_)) {
                completed_turns += 1;
            }
        }
    })
    .await
    .unwrap_or_abort();
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    let provider_request_count = provider.requests().len();
    let compaction_count = events
        .iter()
        .filter(|event| matches!(event.payload, EventV1::SessionCompaction(_)))
        .count();
    assert_eq!((provider_request_count, compaction_count), (3, 0));
}
