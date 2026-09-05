use harness_core::UnwrapOrAbort;
#[tokio::test]
async fn resume_existing_run_restores_sequence_and_ids() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_ids";
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
                    agent_id: "agent_000003".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000008".into(),
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
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000005".into(),
                    tool_id: "shell.run".to_string(),
                    args_summary: "{\"cmd\":\"true\"}".to_string(),
                    args_digest: "digest-tool".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                5,
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "perm_000004".to_string(),
                    kind: "shell".to_string(),
                    tool_call_id: Some("toolcall_000005".into()),
                    summary: "allow shell".to_string(),
                    request_digest: "digest-perm".to_string(),
                    timeout_ms: 1_000,
                    default_decision: EventPermissionDecision::Deny,
                }),
            ),
            resume_fixture_event(
                run_id,
                6,
                EventV1::PermissionResolved(PermissionResolvedEvent {
                    permission_id: "perm_000004".to_string(),
                    decision: EventPermissionDecision::Allow,
                    reason: Some("approved".to_string()),
                }),
            ),
            resume_fixture_event(
                run_id,
                7,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000009".to_string().into(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("tool:shell.run".to_string()),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                8,
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000009".to_string().into(),
                    result_summary: "done".to_string(),
                    result_digest: "digest-task".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                9,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );

    let coordinator = test_resume_coordinator(temp_dir.path());
    let run = coordinator
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_abort();

    let post_resume_events = load_events(&run.events_path);
    assert!(post_resume_events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::RunStarted(data)
                if event.seq == 10
                    && data.run_name.as_str() == "interactive"
                    && data.workspace_root == "/workspace/project"
        )
    }));
    assert!(post_resume_events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::AgentSpawned(data)
                if event.seq == 11
                    && data.agent_id == "agent_000003"
                && data.profile == "default"
        )
    }));

    let new_agent_id = coordinator
        .spawn_agent_idle(supervisor_actor(), "default", None)
        .await
        .unwrap_or_abort();
    assert_eq!(new_agent_id, "agent_000004");

    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), "agent_000003", "resume prompt")
        .await
        .unwrap_or_abort();
    assert_eq!(request_id, "req_000009");

    let tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .unwrap_or_abort();
    assert_eq!(tool_call_id, "toolcall_000006");

    coordinator
        .resolve_permission("perm_000005", RuntimePermissionDecision::Allow, None)
        .await
        .unwrap_or_abort();

    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::PermissionRequested(data)
                if data.permission_id == "perm_000005"
                    && data.tool_call_id.as_ref().map(|id| id.as_str()) == Some("toolcall_000006")
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskScheduled(data) if data.task_id.as_str() == "task_000010"
        )
    }));
    assert!(events.iter().any(|event| {
        matches!(
            &event.payload,
            EventV1::TaskScheduled(data) if data.task_id.as_str() == "task_000011"
        )
    }));
}
#[tokio::test]
async fn resume_existing_run_reuses_same_run_id_and_directory() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_same_dir";
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
                    profile: "default".to_string(),
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

    let coordinator = test_resume_coordinator(temp_dir.path());
    let run = coordinator
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_abort();

    assert_eq!(run.run_id.as_str(), run_id);
    assert_eq!(run.run_dir, temp_dir.path().join(run_id));
    assert_eq!(
        run.events_path,
        temp_dir.path().join(run_id).join("events.jsonl")
    );

    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);
    assert_eq!(
        events.len(),
        7,
        "resume should append start+bindings+finish"
    );
    assert_eq!(events.last().map(|event| event.seq), Some(7));
}
#[tokio::test]
async fn resume_existing_run_restores_subagent_parent_lineage_for_hooks_and_replay() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_subagent_parent_lineage";
    let workspace_root = temp_dir.path().display().to_string();
    let hook_output_path = temp_dir.path().join("resume-subagent-parent-hooks.txt");
    let hook_command = "printf '%s|agent=%s|parent=%s|request=%s\\n' \"$HARNESS_HOOK_EVENT\" \"${HARNESS_HOOK_AGENT_ID:-}\" \"${HARNESS_HOOK_PARENT_AGENT_ID:-}\" \"${HARNESS_HOOK_REQUEST_ID:-}\" >> \"$HOOK_OUTPUT_PATH\"";
    let hook_env = BTreeMap::from([(
        "HOOK_OUTPUT_PATH".to_string(),
        hook_output_path.display().to_string(),
    )]);
    let hook_runtime_config = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![LifecycleHookConfig {
                id: Some("provider-started".to_string()),
                event: HookLifecycleEvent::ProviderRequestStarted,
                command: vec![
                    "bash".to_string(),
                    "-lc".to_string(),
                    hook_command.to_string(),
                ],
                cwd: None,
                timeout_ms: 4_000,
                critical: false,
                env: hook_env,
            }],
        },
        shell_allowlist: ShellAllowlist {
            executables: vec!["bash".to_string()],
            cwd_roots: vec![".".to_string()],
            ..ShellAllowlist::default()
        },
        suppress_execution: false,
    };

    write_resume_fixture(
        temp_dir.path(),
        run_id,
        &[
            resume_fixture_event(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: workspace_root.clone(),
                }),
            ),
            resume_fixture_event(
                run_id,
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            resume_fixture_event(
                run_id,
                3,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000002".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: Some("agent_000001".to_string()),
                }),
            ),
            resume_fixture_event(
                run_id,
                4,
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
                5,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );

    let mut config = CoordinatorConfig::new(temp_dir.path().to_path_buf());
    config.deterministic_store = true;
    config.command_buffer = 64;
    config.provider = Arc::new(CapturingProvider::new(vec!["resumed child answer"]));
    config.agent_profiles = agent_profiles();
    config.hook_runtime_config = hook_runtime_config;

    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    let coordinator = spawn_coordinator(config, clock, redactor);

    let run = coordinator
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_abort();

    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), "agent_000002", "resume child prompt")
        .await
        .unwrap_or_abort();
    assert_eq!(request_id, "req_000002");

    tokio::task::yield_now().await;
    coordinator.stop_run().await.unwrap_or_abort();

    let hook_output = fs::read_to_string(&hook_output_path).unwrap_or_abort();
    assert!(hook_output.lines().any(|line| {
        line.starts_with("provider_request_started|")
            && line.contains("agent=agent_000002")
            && line.contains("parent=agent_000001")
            && line.contains("request=req_000002")
    }));

    let plan = inspect_resume_plan(&run.run_dir);
    let child = plan.child_sessions.get("agent_000002").unwrap_or_abort();
    assert_eq!(child.parent_session_id.as_deref(), Some("agent_000001"));
}
#[tokio::test]
async fn resume_existing_run_restores_agent_profile_bindings() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_agents";
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
                    profile: "default".to_string(),
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

    let coordinator = test_resume_coordinator(temp_dir.path());
    coordinator
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_abort();

    let request_id = coordinator
        .request_agent_turn(supervisor_actor(), "agent_000001", "continue")
        .await
        .unwrap_or_abort();
    assert_eq!(request_id, "req_000002");

    coordinator.stop_run().await.unwrap_or_abort();
}
#[tokio::test]
async fn resumed_run_agent_ids_skip_existing_child_session_directories() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_skips_stale_child_dir";
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
                    profile: "default".to_string(),
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
    let stale_child_dir = temp_dir.path().join("agent_000002");
    fs::create_dir_all(&stale_child_dir).unwrap_or_abort();
    fs::write(stale_child_dir.join(".writer.lock"), "").unwrap_or_abort();
    fs::write(stale_child_dir.join("events.jsonl"), "").unwrap_or_abort();

    let coordinator = test_resume_coordinator(temp_dir.path());

    coordinator
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_abort();
    let child_agent_id = coordinator
        .spawn_agent_idle(
            supervisor_actor(),
            "default",
            Some("agent_000001".to_string()),
        )
        .await
        .unwrap_or_abort();

    assert_eq!(child_agent_id, "agent_000003");
    assert!(temp_dir.path().join("agent_000003/events.jsonl").exists());
    assert_eq!(
        fs::read_to_string(stale_child_dir.join(".writer.lock")).unwrap_or_abort(),
        ""
    );
    coordinator.stop_run().await.unwrap_or_abort();
}
