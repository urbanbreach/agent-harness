#[tokio::test]
async fn model_backed_overflow_split_uses_model_prefix_summary() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let oversized_answer = format!(
        "MODEL_PREFIX_ANCHOR {} MODEL_SUFFIX_ANCHOR",
        "M".repeat(12_000)
    );
    let model_prefix_summary = "## Original Request\nSummarize the latest oversized model-backed turn.\n\n## Early Progress\n- MODEL_PREFIX_SUMMARY captured early progress from the prefix.\n\n## Context for Suffix\n- Continue from the retained suffix using MODEL_PREFIX_SUMMARY.";
    let model_checkpoint_summary = structured_split_model_summary(
        "model split goal",
        "continue after split",
        model_prefix_summary,
    );
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events("first compacted answer"),
        provider_text_events(&oversized_answer),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::error("prompt token count of 128713 exceeds the limit of 128000"),
        ],
        provider_text_events(model_prefix_summary),
        provider_text_events(&model_checkpoint_summary),
        provider_text_events("recovered answer"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            model_backed: true,
            model_ref: Some("mock:model-1".to_string()),
            split_oversized_turns: true,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_model_backed_overflow_split_prefix",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    tokio::task::yield_now().await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "second question")
        .await
        .expect("second turn");
    tokio::task::yield_now().await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "third question")
        .await
        .expect("third turn triggers overflow retry");
    tokio::task::yield_now().await;
    coordinator.stop_run().await.expect("stop run");

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        6,
        "two turns, failed turn, prefix summary, checkpoint summary, retry"
    );
    let prefix_request = &requests[3];
    assert!(prefix_request
        .messages
        .iter()
        .any(|message| message.content.contains("This is the PREFIX of a turn")));
    assert!(prefix_request
        .messages
        .iter()
        .any(|message| message.content.contains("MODEL_PREFIX_ANCHOR")));
    assert!(requests[4]
        .messages
        .iter()
        .any(|message| message.content.contains("MODEL_PREFIX_SUMMARY")));

    let events = load_events(&run.events_path);
    let checkpoint = overflow_checkpoint(&run, &events);
    let tail_boundary = checkpoint.tail_boundary.as_ref().expect("tail boundary");
    assert_eq!(tail_boundary.mode, "split_oversized_turn_tail");
    assert!(tail_boundary
        .split_prefix_summary
        .as_deref()
        .is_some_and(|summary| summary.contains("MODEL_PREFIX_SUMMARY")));
    assert!(tail_boundary
        .note
        .as_deref()
        .is_some_and(|note| note.contains("Split prefix summary source: model_backed.")));
    assert!(checkpoint.summary.contains("MODEL_PREFIX_SUMMARY"));
    let source = checkpoint.summary_source.expect("summary source metadata");
    assert_eq!(source.strategy, "model_backed_summary");
    assert!(source.model_backed);
    assert!(!source.deterministic_fallback);
}
#[tokio::test]
async fn model_backed_overflow_split_summary_without_prefix_content_falls_back() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let oversized_answer = format!(
        "MISSING_PREFIX_CONTENT_ANCHOR {} MISSING_SUFFIX_ANCHOR",
        "P".repeat(12_000)
    );
    let model_prefix_summary = "## Original Request\nSummarize the latest oversized turn.\n\n## Early Progress\n- MISSING_MODEL_PREFIX_SUMMARY captured early work.\n\n## Context for Suffix\n- Continue with MISSING_MODEL_PREFIX_SUMMARY.";
    let invalid_checkpoint_summary = structured_split_model_summary(
        "invalid split goal",
        "continue after invalid split",
        "label present but actual prefix content omitted",
    );
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events("first compacted answer"),
        provider_text_events(&oversized_answer),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::error("prompt token count of 128713 exceeds the limit of 128000"),
        ],
        provider_text_events(model_prefix_summary),
        provider_text_events(&invalid_checkpoint_summary),
        provider_text_events("recovered answer"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            model_backed: true,
            model_ref: Some("mock:model-1".to_string()),
            split_oversized_turns: true,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_model_backed_overflow_split_missing_prefix_content",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    tokio::task::yield_now().await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "second question")
        .await
        .expect("second turn");
    tokio::task::yield_now().await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "third question")
        .await
        .expect("third turn triggers overflow retry");
    tokio::task::yield_now().await;
    coordinator.stop_run().await.expect("stop run");

    assert_eq!(
        provider.requests().len(),
        6,
        "invalid checkpoint summary still allows deterministic compaction and retry"
    );
    let events = load_events(&run.events_path);
    let checkpoint = overflow_checkpoint(&run, &events);
    let source = checkpoint.summary_source.expect("summary source metadata");
    assert_eq!(source.strategy, "model_backed_deterministic_fallback");
    assert!(source.model_backed);
    assert!(source.deterministic_fallback);
    assert!(checkpoint.summary.contains("MISSING_PREFIX_CONTENT_ANCHOR"));
    assert!(!checkpoint
        .summary
        .contains("label present but actual prefix content omitted"));
    assert!(checkpoint
        .tail_boundary
        .as_ref()
        .and_then(|boundary| boundary.split_prefix_summary.as_deref())
        .is_some_and(|summary| summary.contains("MISSING_PREFIX_CONTENT_ANCHOR")));
}
#[tokio::test]
async fn model_backed_overflow_split_empty_prefix_summary_falls_back_deterministically() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let oversized_answer = format!(
        "FALLBACK_PREFIX_ANCHOR {} FALLBACK_SUFFIX_ANCHOR",
        "N".repeat(12_000)
    );
    let deterministic_prefix_excerpt = test_compaction_excerpt(&oversized_answer);
    let model_checkpoint_summary = structured_split_model_summary(
        "fallback split goal",
        "continue after fallback split",
        &deterministic_prefix_excerpt,
    );
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events("first compacted answer"),
        provider_text_events(&oversized_answer),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::error("prompt token count of 128713 exceeds the limit of 128000"),
        ],
        provider_text_events(""),
        provider_text_events(&model_checkpoint_summary),
        provider_text_events("recovered answer"),
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            model_backed: true,
            model_ref: Some("mock:model-1".to_string()),
            split_oversized_turns: true,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_model_backed_overflow_split_prefix_fallback",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    tokio::task::yield_now().await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "second question")
        .await
        .expect("second turn");
    tokio::task::yield_now().await;
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "third question")
        .await
        .expect("third turn triggers overflow retry");
    tokio::task::yield_now().await;
    coordinator.stop_run().await.expect("stop run");

    assert_eq!(
        provider.requests().len(),
        6,
        "empty prefix output still falls through to checkpoint summary and retry"
    );
    let events = load_events(&run.events_path);
    let checkpoint = overflow_checkpoint(&run, &events);
    let tail_boundary = checkpoint.tail_boundary.as_ref().expect("tail boundary");
    assert_eq!(tail_boundary.mode, "split_oversized_turn_tail");
    assert!(tail_boundary
        .split_prefix_summary
        .as_deref()
        .is_some_and(|summary| summary.contains("FALLBACK_PREFIX_ANCHOR")));
    let note = tail_boundary.note.as_deref().expect("tail note");
    assert!(note.contains("Split prefix summary source: model_backed_deterministic_fallback."));
    assert!(note.contains("model split prefix summary was empty"));
    assert!(checkpoint.summary.contains("FALLBACK_PREFIX_ANCHOR"));
    let source = checkpoint.summary_source.expect("summary source metadata");
    assert_eq!(source.strategy, "model_backed_summary");
    assert!(source.model_backed);
    assert!(!source.deterministic_fallback);
}
#[tokio::test]
async fn overflow_auto_retry_can_be_disabled_by_compaction_config() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![
        provider_text_events("first answer"),
        vec![
            ProviderStreamEvent::Start,
            ProviderStreamEvent::error("prompt token count of 128713 exceeds the limit of 128000"),
        ],
    ]);
    let coordinator = test_agent_coordinator_with_provider_and_compaction(
        temp_dir.path(),
        Arc::new(provider.clone()),
        1,
        CompactionRuntimeConfig {
            auto_retry_overflow: false,
            ..CompactionRuntimeConfig::default()
        },
    );

    let run = coordinator
        .start_run(
            "coord_overflow_retry_disabled",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    tokio::task::yield_now().await;
    let second_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "second question")
        .await
        .expect("second turn");
    tokio::task::yield_now().await;
    coordinator.stop_run().await.expect("stop run");

    assert_eq!(provider.requests().len(), 2);
    let events = load_events(&run.events_path);
    assert!(events
        .iter()
        .all(|event| !matches!(event.payload, EventV1::CompactionRequested(_))));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskCancelled(payload)
                if event.correlation_id.as_deref() == Some(second_request_id.as_str())
                    && payload.reason.contains("prompt token count")
        )
    }));
}
#[tokio::test]
async fn manual_compaction_returns_noop_when_context_has_single_turn() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = SequentialScriptedProvider::new(vec![vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta("first answer".to_string()),
        ProviderStreamEvent::Done {
            usage: CompletionUsage {
                prompt_tokens: 32,
                completion_tokens: 8,
                total_tokens: 40,
            },
        },
    ]]);
    let coordinator =
        test_agent_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()), 1);

    let run = coordinator
        .start_run(
            "coord_manual_compaction_noop",
            PathBuf::from("/workspace/project"),
        )
        .await
        .expect("start run");

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn idle alpha");
    let first_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .expect("first turn");
    tokio::task::yield_now().await;

    let outcome = coordinator
        .compact_agent_context(agent_id, Some(first_request_id), "manual")
        .await
        .expect("manual noop succeeds");
    assert_eq!(outcome, ManualCompactionOutcome::NoOp);

    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert!(events.iter().all(|event| {
        !matches!(
            event.payload,
            EventV1::CompactionRequested(_)
                | EventV1::CompactionWritten(_)
                | EventV1::CompactionApplied(_)
        )
    }));
}
#[tokio::test]
async fn resume_rejects_missing_user_message_when_prompt_summary_is_truncated() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_id = "run_resume_truncated_prompt_summary";
    let events_path = write_resume_fixture(
        temp_dir.path(),
        run_id,
        &[
            resume_fixture_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            resume_fixture_event(
                run_id,
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                3,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "truncated historical prompt…".to_string(),
                    request_digest: "digest-req-1".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                4,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
                    request_id: "req_000001".to_string(),
                    delta: "first answer".to_string(),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                5,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: "first answer".to_string(),
                    result_digest: "digest-task-1".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                6,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );

    let before = load_events(&events_path);
    let coordinator = test_resume_coordinator(temp_dir.path());
    let error = coordinator
        .resume_run(run_id, "interactive")
        .await
        .expect_err("truncated prompt summaries must fail closed");

    let CoordinatorError::ResumeRestoreFailed {
        run_id: restored_run_id,
        reason,
    } = error
    else {
        panic!("expected resume restore failure");
    };
    assert_eq!(restored_run_id, run_id);
    assert!(
        reason.contains("prompt_summary is truncated"),
        "unexpected restore failure reason: {reason}"
    );

    let after = load_events(&events_path);
    assert_eq!(after.len(), before.len(), "resume failure must not append");
    assert_eq!(after.last().map(|event| event.seq), Some(6));
}
