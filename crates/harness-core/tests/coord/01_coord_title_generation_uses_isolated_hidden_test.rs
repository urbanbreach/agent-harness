#[tokio::test]
async fn coord_title_generation_uses_isolated_hidden_title_agent_request() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = CapturingProvider::new(vec![
        "<think>hidden</think>\n\nDebugging production 500 errors\nignored",
        "main response",
    ]);
    let mut config = CoordinatorConfig::new(temp_dir.path());
    config.provider = Arc::new(provider.clone());
    config.agent_profiles = agent_profiles_with_title_agent();

    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run(
            harness_core::session_title::create_default_title(&FakeClock::new(), false),
            temp_dir.path(),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn agent");

    coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, Some("user".to_string())),
            agent_id,
            "debug 500 errors in production",
        )
        .await
        .expect("request agent turn");

    let requests = provider.requests();
    let title_request = requests.first().expect("title request");
    assert_eq!(title_request.provider_id.as_deref(), Some("mock"));
    assert_eq!(title_request.model_id, "title-model");
    assert_eq!(
        title_request.temperature,
        Some(harness_core::session_title::TITLE_AGENT_TEMPERATURE)
    );
    assert_eq!(title_request.tools, None);
    assert_eq!(title_request.tool_choice, None);
    assert_eq!(title_request.messages.len(), 3);
    assert_eq!(title_request.messages[0].role, MessageRole::System);
    assert_eq!(
        title_request.messages[0].content,
        harness_core::session_title::TITLE_AGENT_SYSTEM_PROMPT
    );
    assert_eq!(title_request.messages[1].role, MessageRole::User);
    assert_eq!(
        title_request.messages[1].content,
        harness_core::session_title::TITLE_GENERATION_USER_PROMPT
    );
    assert_eq!(title_request.messages[2].role, MessageRole::User);
    assert_eq!(
        title_request.messages[2].content,
        "debug 500 errors in production"
    );

    let events = load_events(&run.events_path);
    assert_eq!(
        events.iter().find_map(|event| match &event.payload {
            EventV1::SessionTitleUpdated(payload) => Some(payload.title.as_str()),
            _ => None,
        }),
        Some("Debugging production 500 errors")
    );
}
#[tokio::test]
async fn coord_supervisor_first_turn_does_not_generate_session_title() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let provider = CapturingProvider::new(vec!["main response"]);
    let mut config = CoordinatorConfig::new(temp_dir.path());
    config.provider = Arc::new(provider.clone());
    config.agent_profiles = agent_profiles_with_title_agent();

    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run(
            harness_core::session_title::create_default_title(&FakeClock::new(), false),
            temp_dir.path(),
        )
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "alpha", None)
        .await
        .expect("spawn agent");

    coordinator
        .request_agent_turn(supervisor_actor(), agent_id, "supervisor bootstrap")
        .await
        .expect("request supervisor turn");
    tokio::task::yield_now().await;

    let requests = provider.requests();
    assert_eq!(
        requests.len(),
        1,
        "supervisor turn must not add title request"
    );
    assert!(!load_events(&run.events_path)
        .iter()
        .any(|event| matches!(event.payload, EventV1::SessionTitleUpdated(_))));
}
#[tokio::test]
async fn coord_plan_mode_prompt_includes_workflow_and_plan_file_lifecycle() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let workspace = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    let provider = CapturingProvider::new(vec!["first plan turn", "second plan turn"]);
    let mut config = CoordinatorConfig::new(temp_dir.path().join("sessions"));
    config.provider = Arc::new(provider.clone());
    config.agent_profiles = BTreeMap::from([(
        harness_core::plan::PLAN_AGENT_NAME.to_string(),
        AgentProfile {
            name: harness_core::plan::PLAN_AGENT_NAME.to_string(),
            category: harness_core::plan::PLAN_AGENT_NAME.to_string(),
            model_ref: "mock:model-1".to_string(),
            model_ref_explicit: true,
            system_prompt: "plan-prompt".to_string(),
            cache_retention: Default::default(),
            max_iters: Some(12),
            temperature: Some(0.0),
            tool_failure_mode: harness_core::config::ToolFailureMode::FailTurn,
            toolset: vec![],
        },
    )]);

    let coordinator = spawn_coordinator(
        config,
        Arc::new(FakeClock::new()),
        Arc::new(DefaultRedactor::default()),
    );
    let run = coordinator
        .start_run("coord_plan_prompt", &workspace)
        .await
        .expect("start run");
    let agent_id = coordinator
        .spawn_agent_idle(
            supervisor_actor(),
            harness_core::plan::PLAN_AGENT_NAME,
            None,
        )
        .await
        .expect("spawn plan agent");

    coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, Some("user".to_string())),
            agent_id.clone(),
            "plan a careful change",
        )
        .await
        .expect("request first plan turn");
    tokio::task::yield_now().await;

    let plan_file = harness_core::plan::plan_file_display_path(&run.run_id);
    let first_requests = provider.requests();
    let first_prompt = &first_requests
        .first()
        .expect("first provider request")
        .messages
        .last()
        .expect("first user message")
        .content;
    assert!(first_prompt.contains("No plan file exists yet"));
    assert!(first_prompt.contains(&plan_file));
    assert!(first_prompt.contains("Launch zero to three `explore` subagents"));
    assert!(first_prompt.contains("final recommended approach"));
    assert!(first_prompt.contains("call `plan_exit`"));
    assert!(first_prompt.contains("run non-readonly tools, change configs, or make commits"));

    let plan_path = workspace.join(harness_core::plan::plan_file_relative_path(&run.run_id));
    fs::create_dir_all(plan_path.parent().expect("plan parent")).expect("plan dir");
    fs::write(&plan_path, "# Plan\n").expect("write plan");

    coordinator
        .request_agent_turn(
            EventActor::new(ActorKind::User, Some("user".to_string())),
            agent_id,
            "update the existing plan",
        )
        .await
        .expect("request second plan turn");
    tokio::task::yield_now().await;

    let second_requests = provider.requests();
    let second_prompt = &second_requests
        .get(1)
        .expect("second provider request")
        .messages
        .last()
        .expect("second user message")
        .content;
    assert!(second_prompt.contains("An active plan file already exists"));
    assert!(second_prompt.contains(&plan_file));
}
#[tokio::test]
async fn coord_start_run_appends_run_started() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_coordinator(temp_dir.path());

    let run = coordinator
        .start_run("coord_start", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");
    coordinator.stop_run().await.expect("stop run");

    assert!(run.run_dir.exists(), "run directory must exist");
    assert!(run.artifacts_dir.exists(), "artifacts directory must exist");
    assert!(run.events_path.exists(), "events log must exist");

    let events = load_events(&run.events_path);
    assert!(
        matches!(
            events.first().map(|event| &event.payload),
            Some(EventV1::RunStarted(_))
        ),
        "first event must be RunStarted"
    );
}
#[tokio::test]
async fn coord_spawn_agent_appends_agent_spawned() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(0));

    let run = coordinator
        .start_run("coord_spawn", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent_supervisor".to_string()));
    let _agent_id = coordinator
        .spawn_agent(actor, "alpha", None)
        .await
        .expect("spawn agent");

    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert!(
        events
            .iter()
            .any(|event| matches!(event.payload, EventV1::AgentSpawned(_))),
        "expected AgentSpawned event"
    );
}
#[tokio::test]
async fn coord_stop_run_appends_run_finished() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_coordinator(temp_dir.path());

    let run = coordinator
        .start_run("coord_stop", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert!(
        matches!(
            events.last().map(|event| &event.payload),
            Some(EventV1::RunFinished(_))
        ),
        "last event must be RunFinished"
    );
}
#[tokio::test]
async fn coord_event_store_subscribe_emits_live_events() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_coordinator(temp_dir.path());

    let _run = coordinator
        .start_run("coord_live_subscribe", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let store = coordinator.event_store().await.expect("get event store");
    let mut stream = store.subscribe(2).expect("subscribe from live boundary");

    coordinator.stop_run().await.expect("stop run");

    let event = tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await
        .expect("event should arrive")
        .expect("stream should produce item")
        .expect("stream item should be valid");

    assert!(matches!(event.payload, EventV1::RunFinished(_)));
}
#[tokio::test]
async fn coord_worker_spawn_attempt_records_policy_violation_and_returns_error() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(0));

    let run = coordinator
        .start_run("coord_policy", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Worker, Some("agent_worker".to_string()));
    let result = coordinator.spawn_agent(actor.clone(), "alpha", None).await;
    assert!(result.is_err(), "worker spawn must fail");
    let idle_result = coordinator.spawn_agent_idle(actor, "alpha", None).await;
    assert!(idle_result.is_err(), "worker idle spawn must fail");

    coordinator.stop_run().await.expect("stop run");

    let events = load_events(&run.events_path);
    assert!(
        events
            .iter()
            .any(|event| matches!(event.payload, EventV1::PolicyViolationDetected(_))),
        "expected PolicyViolationDetected event"
    );
}
#[tokio::test]
async fn coord_spawn_two_agents_respects_provider_concurrency_and_queues() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_agent_coordinator(temp_dir.path(), Duration::from_millis(25));

    let run = coordinator
        .start_run("coord_agents_queue", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent_supervisor".to_string()));
    let spawn_a = coordinator.spawn_agent(actor.clone(), "alpha", None);
    let spawn_b = coordinator.spawn_agent(actor, "beta", None);
    let (_agent_a, _agent_b) = tokio::join!(spawn_a, spawn_b);

    let events = wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
        let queued = events
            .iter()
            .filter(|event| {
                matches!(
                    event.payload,
                    EventV1::TaskScheduled(ref data)
                        if data.state == harness_core::event::TaskScheduleState::Queued
                )
            })
            .count();
        let completed = events
            .iter()
            .filter(|event| matches!(event.payload, EventV1::TaskCompleted(_)))
            .count();
        queued >= 1 && completed == 2
    })
    .await;
    coordinator.stop_run().await.expect("stop run");

    let queued = events
        .iter()
        .filter(|event| {
            matches!(
                event.payload,
                EventV1::TaskScheduled(ref data)
                    if data.state == harness_core::event::TaskScheduleState::Queued
            )
        })
        .count();
    let completed = events
        .iter()
        .filter(|event| matches!(event.payload, EventV1::TaskCompleted(_)))
        .count();

    assert!(
        queued >= 1,
        "expected at least one queued task for concurrency limit 1"
    );
    assert_eq!(completed, 2, "both spawned agents should complete");
}
#[tokio::test]
async fn coord_spawn_unknown_profile_returns_error() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let coordinator = test_coordinator(temp_dir.path());

    let run = coordinator
        .start_run("coord_unknown_profile", PathBuf::from("/workspace/project"))
        .await
        .expect("start run");

    let actor = EventActor::new(ActorKind::Supervisor, Some("agent_supervisor".to_string()));
    let err = coordinator
        .spawn_agent(actor, "missing_profile", None)
        .await
        .expect_err("unknown profile should fail");

    assert!(matches!(err, CoordinatorError::UnknownAgent(profile) if profile == "missing_profile"));

    coordinator.stop_run().await.expect("stop run");
    let events = load_events(&run.events_path);
    assert!(
        !events.iter().any(|event| matches!(
            &event.payload,
            EventV1::AgentSpawned(AgentSpawnedEvent { profile, .. }) if profile == "missing_profile"
        )),
        "unknown profiles should not emit AgentSpawned events"
    );
}
