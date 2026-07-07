use harness_core::UnwrapOrAbort;
#[tokio::test]
async fn manual_compaction_after_four_small_turns_writes_checkpoint_with_latest_turn() {
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
                    usage: CompletionUsage {
                        prompt_tokens: 100,
                        completion_tokens: 100,
                        total_tokens: 200,
                    },
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
    let ManualCompactionOutcome::CheckpointWritten { checkpoint_id, .. } = outcome else {
        panic!("expected manual compaction to force a checkpoint");
    };

    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "manual" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(written.checkpoint_id, checkpoint_id);
    assert_eq!(written.preserved_turns, 1);

    let checkpoint_path = run.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).unwrap_or_abort();
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).unwrap_or_abort();
    assert_eq!(
        checkpoint.metadata.trigger_reason.as_deref(),
        Some("manual")
    );
    assert_eq!(checkpoint.recent_turns.len(), 1);
    assert_eq!(
        checkpoint.recent_turns[0].user_prompt,
        "fourth small question"
    );
    assert_eq!(
        checkpoint.recent_turns[0].assistant_response,
        "fourth answer"
    );
    assert!(checkpoint.summary.contains("first small question"));
    assert!(checkpoint.summary.contains("second small question"));
    assert!(checkpoint.summary.contains("third small question"));
    assert!(!checkpoint.summary.contains("fourth small question"));
}
#[tokio::test]
async fn manual_compaction_after_two_turns_summarizes_first_and_preserves_latest() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(
        ["first answer", "second answer"]
            .into_iter()
            .map(|answer| {
                vec![
                    ProviderStreamEvent::Start,
                    ProviderStreamEvent::TextDelta(answer.to_string()),
                    ProviderStreamEvent::Done {
                        usage: CompletionUsage {
                            prompt_tokens: 100,
                            completion_tokens: 100,
                            total_tokens: 200,
                        },
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
    let ManualCompactionOutcome::CheckpointWritten { checkpoint_id, .. } = outcome else {
        panic!("expected manual compaction to force a checkpoint");
    };

    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    let written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "manual" => {
                Some(payload.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(written.checkpoint_id, checkpoint_id);
    assert_eq!(written.trigger_reason, "manual");
    assert_eq!(written.preserved_turns, 1);

    let checkpoint_path = run.run_dir.join(&written.artifact_path);
    let checkpoint_body = fs::read_to_string(&checkpoint_path).unwrap_or_abort();
    let checkpoint: ProviderContextCheckpoint =
        serde_json::from_str(&checkpoint_body).unwrap_or_abort();
    assert_eq!(
        checkpoint.metadata.trigger_reason.as_deref(),
        Some("manual")
    );
    assert_eq!(checkpoint.recent_turns.len(), 1);
    assert_eq!(checkpoint.recent_turns[0].user_prompt, "second question");
    assert_eq!(
        checkpoint.recent_turns[0].assistant_response,
        "second answer"
    );
    assert!(checkpoint.summary.contains("first question"));
    assert!(checkpoint.summary.contains("first answer"));
    assert!(!checkpoint.summary.contains("second question"));
    assert!(!checkpoint.summary.contains("second answer"));
}
#[tokio::test]
async fn manual_compaction_uses_optional_model_backed_summary_without_provider_events() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let model_summary = structured_model_summary("model kept the goal", "model next step");
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
        provider_text_events(&model_summary),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            model_backed: true,
            model_ref: Some("mock:model-1".to_string()),
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
        "compaction model calls stay out of events"
    );
    assert_eq!(
        provider.requests().len(),
        3,
        "two turns plus one summary model call"
    );
    let checkpoint = manual_checkpoint(&run, &events);
    assert_eq!(checkpoint.summary.trim(), model_summary.trim());
    let source = checkpoint.summary_source.unwrap_or_abort();
    assert_eq!(source.strategy, "model_backed_summary");
    assert!(source.model_backed);
    assert!(!source.deterministic_fallback);
}
#[tokio::test]
async fn model_backed_compaction_falls_back_for_invalid_summary_and_records_metadata() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
        provider_text_events("not a structured checkpoint"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            model_backed: true,
            model_ref: Some("mock:model-1".to_string()),
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_model_compaction_fallback",
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
    let checkpoint = manual_checkpoint(&run, &events);
    let written_event = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::CompactionWritten(payload) if payload.trigger_reason == "manual" => {
                Some(payload)
            }
            _ => None,
        })
        .unwrap_or_abort();
    let written_json = serde_json::to_value(written_event).unwrap_or_abort();
    assert_eq!(
        written_json["summary_source"]["strategy"],
        "model_backed_deterministic_fallback"
    );
    let source = checkpoint.summary_source.unwrap_or_abort();
    assert_eq!(source.strategy, "model_backed_deterministic_fallback");
    assert!(source.model_backed);
    assert!(source.deterministic_fallback);
    assert!(checkpoint.summary.contains("## Goal"));
    assert!(checkpoint.summary.contains("first question"));
    assert!(!checkpoint.summary.contains("not a structured checkpoint"));
    assert_eq!(
        provider.requests().len(),
        3,
        "fallback should use two prompt requests plus one failed summary request without looping"
    );
}
#[tokio::test]
async fn hook_summary_override_takes_precedence_over_model_backed_compaction() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events(&"A".repeat(12_000)),
        provider_text_events(&"B".repeat(12_000)),
    ]);
    let hook_runtime_config = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("compaction-summary".to_string()),
                event: HookLifecycleEvent::CompactionRequested,
                command: vec![
                    "bash".to_string(),
                    "-lc".to_string(),
                    "printf 'compaction_summary: hook supplied checkpoint recap'".to_string(),
                ],
                cwd: Some(".".to_string()),
                timeout_ms: 4_000,
                critical: false,
                env: BTreeMap::new(),
            }],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["bash".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        },
        suppress_execution: false,
    };
    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider_model_concurrency = 1;
    config.provider = Arc::new(provider.clone());
    config.agent_profiles = agent_profiles();
    config.hook_runtime_config = hook_runtime_config;
    config.compaction = CompactionRuntimeConfig {
        model_backed: true,
        model_ref: Some("mock:model-1".to_string()),
        ..CompactionRuntimeConfig::default()
    };
    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );

    let run = coordinator
        .start_run(
            "coord_hook_compaction_summary_precedence",
            temp_dir.path().to_path_buf(),
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

    assert_eq!(
        provider.requests().len(),
        2,
        "hook override prevents model summary call"
    );
    let events = load_events(&run.events_path);
    let checkpoint = manual_checkpoint(&run, &events);
    assert_eq!(checkpoint.summary, "hook supplied checkpoint recap");
    let source = checkpoint.summary_source.unwrap_or_abort();
    assert_eq!(source.strategy, "hook_supplied_summary");
    assert!(!source.model_backed);
    assert!(!source.deterministic_fallback);
}
#[tokio::test]
async fn overflow_retry_split_oversized_latest_turn_preserves_suffix_context() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let oversized_answer = "B".repeat(12_000);
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events("first compacted answer"),
        provider_text_events(&oversized_answer),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::error("prompt token count of 128713 exceeds the limit of 128000"),
        ],
        provider_text_events("recovered answer"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            split_oversized_turns: true,
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

    let retried_messages = provider
        .requests()
        .last()
        .unwrap_or_abort()
        .messages
        .clone();
    assert!(retried_messages.iter().any(|message| {
        message.role == MessageRole::User
            && message
                .content
                .contains("preserved suffix of an oversized latest turn")
            && message
                .content
                .contains("earlier prefix is summarized in the checkpoint")
    }));
    assert!(retried_messages.iter().any(|message| {
        message.role == MessageRole::Assistant && message.content.len() < 12_000
    }));

    let events = load_events(&run.events_path);
    let checkpoint = overflow_checkpoint(&run, &events);
    assert_eq!(checkpoint.recent_turns.len(), 1);
    assert!(checkpoint
        .summary
        .contains("earlier prefix of an oversized latest turn"));
    assert!(checkpoint
        .facts
        .compacted_turns
        .iter()
        .any(|fact| fact.user_excerpt.contains("first question")));
    assert!(checkpoint.facts.compacted_turns.iter().any(|fact| fact
        .user_excerpt
        .contains("earlier prefix of an oversized latest turn")));
    assert!(checkpoint.recent_turns[0]
        .user_prompt
        .contains("preserved suffix of an oversized latest turn"));
    let tail_boundary = checkpoint.tail_boundary.as_ref().unwrap_or_abort();
    assert_eq!(tail_boundary.mode, "split_oversized_turn_tail");
    assert!(tail_boundary
        .split_prefix_summary
        .as_deref()
        .is_some_and(|summary| summary.contains('B')));
    assert!(checkpoint.summary.contains("Split prefix summary"));
    assert!(checkpoint
        .summary
        .contains("Source facts: split prefix summary"));
}
