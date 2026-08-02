use harness_core::UnwrapOrAbort;
#[tokio::test]
async fn manual_compaction_after_four_small_turns_writes_checkpoint_with_latest_turn() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(
        [
            "first answer",
            "second answer",
            "third answer",
            "fourth answer",
        ]
        .into_iter()
        .map(|answer| {
            vec![
                ProviderStreamEvent::Start,
                ProviderStreamEvent::TextDelta(answer.to_string()),
                ProviderStreamEvent::Done {
                    usage: Some(CompletionUsage {
                        prompt_tokens: 100,
                        completion_tokens: 100,
                        total_tokens: 200,
                    }),
                },
            ]
        })
        .collect(),
    );
    let coordinator =
        test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);

    let run = coordinator
        .start_run(
            "coord_manual_compaction_forced_checkpoint",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    for question in [
        "first small question",
        "second small question",
        "third small question",
        "fourth small question",
    ] {
        coordinator
            .request_agent_turn(supervisor_actor(), agent_id.clone(), question)
            .await
            .unwrap_or_abort();
        tokio::task::yield_now().await;
    }

    let outcome = coordinator
        .compact_agent_context(agent_id, Some("req_000004".to_string()), "manual")
        .await
        .unwrap_or_abort();
    let ManualCompactionOutcome::Compacted { .. } = outcome else {
        panic!("expected manual compaction to apply");
    };

    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    let compaction = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::SessionCompaction(payload) if payload.trigger_reason == "manual" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(compaction.trigger_reason, "manual");
    assert!(compaction.tokens_before > 0);
    assert!(compaction.summary.contains("first small question"));
    assert!(compaction.summary.contains("second small question"));
    assert!(compaction.summary.contains("third small question"));
    assert!(!compaction.summary.contains("fourth small question"));
}
#[tokio::test]
async fn manual_compaction_after_two_turns_summarizes_first_and_preserves_latest() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(
        ["first answer", "second answer"]
            .into_iter()
            .map(|answer| {
                vec![
                    ProviderStreamEvent::Start,
                    ProviderStreamEvent::TextDelta(answer.to_string()),
                    ProviderStreamEvent::Done {
                        usage: Some(CompletionUsage {
                            prompt_tokens: 100,
                            completion_tokens: 100,
                            total_tokens: 200,
                        }),
                    },
                ]
            })
            .collect(),
    );
    let coordinator =
        test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);

    let run = coordinator
        .start_run(
            "coord_manual_compaction_two_turns",
            PathBuf::from("/workspace/project"),
        )
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

    let outcome = coordinator
        .compact_agent_context(agent_id, Some("req_000002".to_string()), "manual")
        .await
        .unwrap_or_abort();
    let ManualCompactionOutcome::Compacted { .. } = outcome else {
        panic!("expected manual compaction to apply");
    };

    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    let compaction = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::SessionCompaction(payload) if payload.trigger_reason == "manual" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(compaction.trigger_reason, "manual");
    assert!(compaction.tokens_before > 0);
    assert!(compaction.summary.contains("first question"));
    assert!(compaction.summary.contains("first answer"));
    assert!(!compaction.summary.contains("second question"));
    assert!(!compaction.summary.contains("second answer"));
}
#[tokio::test]
async fn manual_compaction_summary_call_uses_provider_without_emitting_provider_events() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
        provider_text_events("Compaction summary of earlier turns."),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_manual_model_backed_compaction",
            PathBuf::from("/workspace/project"),
        )
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

    coordinator
        .compact_agent_context(agent_id, Some("req_000002".to_string()), "manual")
        .await
        .unwrap_or_abort();
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    let provider_started_count = events
        .iter()
        .filter(|event| matches!(event.payload, EventV1::ProviderRequestStarted(_)))
        .count();
    assert_eq!(
        provider_started_count, 2,
        "compaction summary call stays out of provider events"
    );
    assert_eq!(
        provider.requests().len(),
        3,
        "two turns plus one summary model call"
    );
    let compaction = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::SessionCompaction(payload) if payload.trigger_reason == "manual" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(compaction.trigger_reason, "manual");
    assert!(compaction.tokens_before > 0);
}
#[tokio::test]
async fn overflow_retry_split_oversized_latest_turn_preserves_suffix_context() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let oversized_answer = "B".repeat(12_000);
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events("first compacted answer"),
        provider_text_events(&oversized_answer),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::error("prompt token count of 128713 exceeds the limit of 128000"),
        ],
        provider_text_events("Compaction summary of earlier turns."),
        provider_text_events("recovered answer"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_overflow_retry_split_tail",
            PathBuf::from("/workspace/project"),
        )
        .await
        .unwrap_or_abort();
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "second question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "third question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::SessionCompaction(payload)
                if payload.trigger_reason == "overflow"
        )
    }));
    assert_eq!(
        provider.requests().len(),
        5,
        "two turns + overflow error + summary + retry"
    );
}
