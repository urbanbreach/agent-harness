use harness_core::UnwrapOrAbort;
#[tokio::test]
async fn resume_existing_run_persists_bindings_for_future_reresume() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_reresume";
    write_resume_fixture(
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
            resume_fixture_event(
                run_id,
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "existing prompt".to_string(),
                    request_digest: "digest-existing".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );

    let first = test_resume_coordinator(temp_dir.path());
    let run = first
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_abort();
    first
        .request_agent_turn(supervisor_actor(), "agent_000001", "follow up")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    first.stop_run().await.unwrap_or_abort();

    let plan_after_first_resume = inspect_resume_plan(&run.run_dir);
    assert_eq!(
        plan_after_first_resume.latest_lifecycle_status,
        LifecycleSegmentStatus::Finished
    );
    assert_eq!(
        plan_after_first_resume
            .known_agents
            .get("agent_000001")
            .map(String::as_str),
        Some("alpha")
    );
    assert!(
        plan_after_first_resume.is_resumable,
        "resumed segment should remain resumable after stop"
    );

    let second = test_resume_coordinator(temp_dir.path());
    second
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_abort();
    let second_request_id = second
        .request_agent_turn(supervisor_actor(), "agent_000001", "second resume turn")
        .await
        .unwrap_or_abort();
    assert_eq!(second_request_id, "req_000004");
    second
        .stop_run()
        .await
        .unwrap_or_abort();
}
#[tokio::test]
async fn resume_existing_run_remains_resumable_after_open_and_quit_without_prompt() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_open_quit";
    write_resume_fixture(
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
            resume_fixture_event(
                run_id,
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "existing prompt".to_string(),
                    request_digest: "digest-existing".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );

    let first = test_resume_coordinator(temp_dir.path());
    let run = first
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_abort();
    first
        .stop_run()
        .await
        .unwrap_or_abort();

    let plan_after_quit = inspect_resume_plan(&run.run_dir);
    assert_eq!(
        plan_after_quit.latest_lifecycle_status,
        LifecycleSegmentStatus::Finished
    );
    assert!(
        plan_after_quit.is_resumable,
        "open-and-quit resumed segments should remain resumable"
    );
    assert_eq!(
        plan_after_quit.provider_model.as_deref(),
        Some("mock/model-1")
    );

    let second = test_resume_coordinator(temp_dir.path());
    second
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_abort();
    let request_id = second
        .request_agent_turn(supervisor_actor(), "agent_000001", "second segment prompt")
        .await
        .unwrap_or_abort();
    assert_eq!(request_id, "req_000002");
    second
        .stop_run()
        .await
        .unwrap_or_abort();
}
#[tokio::test]
async fn resume_existing_run_rejects_missing_historical_profile_binding() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_missing_profile";
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
                    profile: "gamma".to_string(),
                    parent_agent_id: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "existing prompt".to_string(),
                    request_digest: "digest-existing".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                4,
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
        .expect_err("missing profile binding should fail closed");

    let CoordinatorError::ResumeRestoreFailed {
        run_id: restored_run_id,
        reason,
    } = error
    else {
        panic!("expected resume restore failure");
    };
    assert_eq!(restored_run_id, run_id);
    assert!(
        reason
            .contains("historical agent `agent_000001` references missing profile binding `gamma`"),
        "unexpected restore failure reason: {reason}"
    );

    let after = load_events(&events_path);
    assert_eq!(after.len(), before.len(), "resume failure must not append");
    assert_eq!(after.last().map(|event| event.seq), Some(4));
}
#[tokio::test]
async fn resume_existing_run_rejects_second_writer_lock() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let first = test_resume_coordinator(temp_dir.path());
    let run = first
        .start_run("interactive", PathBuf::from("/workspace/project"))
        .await
        .unwrap_or_abort();

    let second = test_resume_coordinator(temp_dir.path());
    let error = second
        .resume_run(run.run_id.as_str(), "interactive")
        .await
        .expect_err("second writer must fail lock acquisition");

    assert!(matches!(
        error,
        CoordinatorError::EventStore(EventStoreError::AcquireWriterLock { .. })
    ));

    first.stop_run().await.unwrap_or_abort();
}
#[tokio::test]
async fn resume_existing_run_does_not_append_on_restore_failure() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_invalid_agent";
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
                    agent_id: "agent_invalid".to_string(),
                    profile: "alpha".to_string(),
                    parent_agent_id: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "existing prompt".to_string(),
                    request_digest: "digest-existing".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                4,
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
        .expect_err("invalid restore metadata should fail closed");

    assert!(matches!(
        error,
        CoordinatorError::ResumeRestoreFailed { .. }
    ));

    let after = load_events(&events_path);
    assert_eq!(after.len(), before.len(), "resume failure must not append");
    assert_eq!(after.last().map(|event| event.seq), Some(4));
}
#[tokio::test]
async fn resume_restores_interactive_provider_context() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_context";
    write_resumable_history_fixture(temp_dir.path(), run_id);

    let provider = CapturingProvider::new(vec!["second answer"]);
    let coordinator =
        test_resume_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()));

    coordinator
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_abort();
    coordinator
        .request_agent_turn(supervisor_actor(), "agent_000001", "second question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

    let requests = provider.requests();
    assert_eq!(requests.len(), 1, "expected one resumed provider request");

    let shape = requests[0]
        .messages
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        shape,
        vec![
            (MessageRole::System, "alpha-prompt".to_string()),
            (MessageRole::User, "first question".to_string()),
            (MessageRole::Assistant, "first answer".to_string()),
            (MessageRole::User, "second question".to_string()),
        ]
    );
}
#[tokio::test]
async fn resumed_turn_matches_uninterrupted_conversation_request_shape() {
    let uninterrupted_dir = tempfile::tempdir().unwrap_or_abort();
    let uninterrupted_provider = CapturingProvider::new(vec!["first answer", "second answer"]);
    let uninterrupted = test_resume_coordinator_with_provider(
        uninterrupted_dir.path(),
        Arc::new(uninterrupted_provider.clone()),
    );

    uninterrupted
        .start_run("interactive", PathBuf::from("/workspace/project"))
        .await
        .unwrap_or_abort();
    uninterrupted
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .unwrap_or_abort();
    uninterrupted
        .request_agent_turn(supervisor_actor(), "agent_000001", "first question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    uninterrupted
        .request_agent_turn(supervisor_actor(), "agent_000001", "second question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    uninterrupted
        .stop_run()
        .await
        .unwrap_or_abort();

    let uninterrupted_shape = uninterrupted_provider
        .requests()
        .last()
        .unwrap_or_abort()
        .messages
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect::<Vec<_>>();

    let resumed_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_matches_uninterrupted";
    write_resumable_history_fixture(resumed_dir.path(), run_id);
    let resumed_provider = CapturingProvider::new(vec!["second answer"]);
    let resumed = test_resume_coordinator_with_provider(
        resumed_dir.path(),
        Arc::new(resumed_provider.clone()),
    );

    resumed
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_abort();
    resumed
        .request_agent_turn(supervisor_actor(), "agent_000001", "second question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    resumed.stop_run().await.unwrap_or_abort();

    let resumed_shape = resumed_provider
        .requests()
        .last()
        .unwrap_or_abort()
        .messages
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect::<Vec<_>>();

    assert_eq!(
        resumed_shape, uninterrupted_shape,
        "resumed turns should use the same provider request shape as uninterrupted conversations"
    );
}
#[tokio::test]
async fn resume_restores_multi_turn_historical_context_with_final_task_output() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_multi_turn_context";
    write_resumable_multi_turn_history_fixture(temp_dir.path(), run_id);

    let provider = CapturingProvider::new(vec!["second answer"]);
    let coordinator =
        test_resume_coordinator_with_provider(temp_dir.path(), Arc::new(provider.clone()));

    coordinator
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_abort();
    coordinator
        .request_agent_turn(supervisor_actor(), "agent_000001", "second question")
        .await
        .unwrap_or_abort();
    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

    let requests = provider.requests();
    assert_eq!(requests.len(), 1, "expected one resumed provider request");

    let shape = requests[0]
        .messages
        .iter()
        .map(|message| (message.role.clone(), message.content.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        shape,
        vec![
            (MessageRole::System, "alpha-prompt".to_string()),
            (MessageRole::User, "first question".to_string()),
            (MessageRole::Assistant, "first final answer".to_string()),
            (MessageRole::User, "second question".to_string()),
        ]
    );
}
