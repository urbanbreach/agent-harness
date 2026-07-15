use harness_core::UnwrapOrAbort;
#[tokio::test]
async fn overflow_compaction_appends_session_compaction_event_and_retries() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let oversized_answer = format!(
        "MODEL_PREFIX_ANCHOR {} MODEL_SUFFIX_ANCHOR",
        "M".repeat(12_000)
    );
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
            "coord_overflow_compaction_retry",
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

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        5,
        "two turns, overflow error, summary call, retry"
    );

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::SessionCompaction(payload)
                if payload.trigger_reason == "overflow"
        )
    }));
}
#[tokio::test]
async fn overflow_auto_retry_can_be_disabled_by_compaction_config() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
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
    let second_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "second question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

    assert_eq!(provider.requests().len(), 2);
    let events = load_events(&run.events_path);
    assert!(events
        .iter()
        .all(|event| !matches!(event.payload, EventV1::SessionCompaction(_))));
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
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let provider = SequentialScriptedProvider::new(vec![vec![
        ProviderStreamEvent::Start,
        ProviderStreamEvent::TextDelta("first answer".to_string()),
        ProviderStreamEvent::Done {
            usage: Some(CompletionUsage {
                prompt_tokens: 32,
                completion_tokens: 8,
                total_tokens: 40,
            }),
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
        .unwrap_or_abort();

    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    let first_request_id = coordinator
        .request_agent_turn(supervisor_actor(), agent_id.clone(), "first question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;

    let outcome = coordinator
        .compact_agent_context(agent_id, Some(first_request_id), "manual")
        .await
        .unwrap_or_abort();
    assert_eq!(outcome, ManualCompactionOutcome::NoOp);

    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    assert!(events.iter().all(|event| {
        !matches!(
            event.payload,
            EventV1::SessionCompaction(_)
        )
    }));
}
#[tokio::test]
async fn resume_rejects_missing_user_message_when_prompt_summary_is_truncated() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_truncated_prompt_summary";
    let events_path = write_resume_fixture(
        temp_dir.path(),
        run_id,
        &[
            resume_fixture_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
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
                    request_id: "req_000001".into(),
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
                    request_id: "req_000001".into(),
                    delta: "first answer".to_string(),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                5,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000001"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string().into(),
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
