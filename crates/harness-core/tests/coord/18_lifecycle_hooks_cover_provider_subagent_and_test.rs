use harness_core::UnwrapOrAbort;
#[tokio::test]
async fn lifecycle_hooks_cover_provider_subagent_and_permission_events() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let hook_output_path = temp_dir.path().join("hook-lifecycle-events.txt");
    let hook_command = "printf '%s|agent=%s|parent=%s|request=%s|permission=%s|tool_call=%s|provider=%s|outcome=%s\\n' \"$HARNESS_HOOK_EVENT\" \"${HARNESS_HOOK_AGENT_ID:-}\" \"${HARNESS_HOOK_PARENT_AGENT_ID:-}\" \"${HARNESS_HOOK_REQUEST_ID:-}\" \"${HARNESS_HOOK_PERMISSION_ID:-}\" \"${HARNESS_HOOK_TOOL_CALL_ID:-}\" \"${HARNESS_HOOK_PROVIDER_ID:-}\" \"${HARNESS_HOOK_OUTCOME:-}\" >> \"$HOOK_OUTPUT_PATH\"";
    let hook_env = BTreeMap::from([(
        "HOOK_OUTPUT_PATH".to_string(),
        hook_output_path.display().to_string(),
    )]);
    let hook_runtime_config = HookRuntimeConfig {
        hooks: HooksConfig {
            lifecycle: vec![
                LifecycleHookConfig {
                    id: Some("subagent-spawned".to_string()),
                    event: HookLifecycleEvent::SubagentSpawned,
                    command: vec![
                        "bash".to_string(),
                        "-lc".to_string(),
                        hook_command.to_string(),
                    ],
                    cwd: Some(".".to_string()),
                    timeout_ms: 4_000,
                    critical: false,
                    env: hook_env.clone(),
                },
                LifecycleHookConfig {
                    id: Some("provider-started".to_string()),
                    event: HookLifecycleEvent::ProviderRequestStarted,
                    command: vec![
                        "bash".to_string(),
                        "-lc".to_string(),
                        hook_command.to_string(),
                    ],
                    cwd: Some(".".to_string()),
                    timeout_ms: 4_000,
                    critical: false,
                    env: hook_env.clone(),
                },
                LifecycleHookConfig {
                    id: Some("provider-finished".to_string()),
                    event: HookLifecycleEvent::ProviderRequestFinished,
                    command: vec![
                        "bash".to_string(),
                        "-lc".to_string(),
                        hook_command.to_string(),
                    ],
                    cwd: Some(".".to_string()),
                    timeout_ms: 4_000,
                    critical: false,
                    env: hook_env.clone(),
                },
                LifecycleHookConfig {
                    id: Some("subagent-finished".to_string()),
                    event: HookLifecycleEvent::SubagentFinished,
                    command: vec![
                        "bash".to_string(),
                        "-lc".to_string(),
                        hook_command.to_string(),
                    ],
                    cwd: Some(".".to_string()),
                    timeout_ms: 4_000,
                    critical: false,
                    env: hook_env.clone(),
                },
                LifecycleHookConfig {
                    id: Some("permission-requested".to_string()),
                    event: HookLifecycleEvent::PermissionRequested,
                    command: vec![
                        "bash".to_string(),
                        "-lc".to_string(),
                        hook_command.to_string(),
                    ],
                    cwd: Some(".".to_string()),
                    timeout_ms: 4_000,
                    critical: false,
                    env: hook_env.clone(),
                },
                LifecycleHookConfig {
                    id: Some("permission-resolved".to_string()),
                    event: HookLifecycleEvent::PermissionResolved,
                    command: vec![
                        "bash".to_string(),
                        "-lc".to_string(),
                        hook_command.to_string(),
                    ],
                    cwd: Some(".".to_string()),
                    timeout_ms: 4_000,
                    critical: false,
                    env: hook_env,
                },
            ],
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
    config.permission_policy = ask_shell_permission_policy();
    config.tool_registry = lifecycle_tool_registry(Arc::new(Notify::new()));
    config.provider = Arc::new(SlowMockProvider {
        inner: test_mock_provider(),
        delay: Duration::from_millis(5),
    });
    config.agent_profiles = agent_profiles();
    config.hook_runtime_config = hook_runtime_config;

    let clock = Arc::new(FakeClock::new());
    let redactor = Arc::new(DefaultRedactor::default());
    let coordinator = spawn_coordinator(config, clock, redactor);

    let run = coordinator
        .start_run(
            "lifecycle_hooks_cover_provider_subagent_and_permission_events",
            temp_dir.path().to_path_buf(),
        )
        .await
        .unwrap_or_abort();

    let subagent_id = coordinator
        .spawn_agent(
            supervisor_actor(),
            "alpha",
            Some("agent_parent_001".to_string()),
        )
        .await
        .unwrap_or_abort();
    assert_eq!(subagent_id, "agent_000001");

    wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
        events.iter().any(|event| {
            event.actor.agent_id.as_deref() == Some(subagent_id.as_str())
                && matches!(event.payload, EventV1::TaskCompleted(_))
        })
    })
    .await;

    let tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "shell.run",
            json!({"cmd": "true"}),
        )
        .await
        .unwrap_or_abort();
    assert_eq!(tool_call_id, "toolcall_000001");

    let events = wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::PermissionRequested(data) if data.tool_call_id.as_deref() == Some(tool_call_id.as_str())
            )
        })
    })
    .await;
    let permission_id = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::PermissionRequested(data)
                if data.tool_call_id.as_deref() == Some(tool_call_id.as_str()) =>
            {
                Some(data.permission_id.clone())
            }
            _ => None,
        })
        .unwrap_or_abort();

    coordinator
        .resolve_permission(
            permission_id,
            RuntimePermissionDecision::Allow,
            Some("approved".to_string()),
        )
        .await
        .unwrap_or_abort();

    wait_for_events(&run.events_path, Duration::from_secs(2), |events| {
        events.iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(data)
                    if data.tool_call_id == tool_call_id
                        && data.status == ToolCallStatus::Succeeded
            )
        })
    })
    .await;
    coordinator.stop_run().await.unwrap_or_abort();

    let hook_output = fs::read_to_string(&hook_output_path).unwrap_or_abort();
    let lines = hook_output.lines().collect::<Vec<_>>();

    assert!(lines.iter().any(|line| {
        line.starts_with("subagent_spawned|")
            && line.contains("agent=agent_000001")
            && line.contains("parent=agent_parent_001")
            && line.contains("outcome=spawned")
    }));
    assert!(lines.iter().any(|line| {
        line.starts_with("provider_request_started|")
            && line.contains("agent=agent_000001")
            && line.contains("request=req_000001")
            && line.contains("provider=mock")
    }));
    assert!(lines.iter().any(|line| {
        line.starts_with("provider_request_finished|") && line.contains("request=req_000001")
    }));
    assert!(lines.iter().any(|line| {
        line.starts_with("subagent_finished|")
            && line.contains("agent=agent_000001")
            && line.contains("parent=agent_parent_001")
            && line.contains("outcome=succeeded")
    }));
    assert!(lines.iter().any(|line| {
        line.starts_with("permission_requested|")
            && line.contains("permission=perm_000001")
            && line.contains("tool_call=toolcall_000001")
            && line.contains("outcome=requested")
    }));
    assert!(lines.iter().any(|line| {
        line.starts_with("permission_resolved|")
            && line.contains("permission=perm_000001")
            && line.contains("tool_call=toolcall_000001")
            && line.contains("outcome=allow")
    }));
}
#[test]
fn replay_reconstructs_parallel_child_sessions_and_timings() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_replay_parallel_child_sessions";

    let lineage_a = TaskLineageMetadata {
        parent_tool_call_id: Some("toolcall_000201".to_string()),
        parent_task_id: Some("task_000201".to_string()),
        parent_request_id: Some("req_000010".to_string()),
        parent_session_id: Some("agent_000001".to_string()),
        child_session_id: Some("agent_000101".to_string()),
        child_request_id: Some("req_000101".to_string()),
        child_provider_id: Some("mock".to_string()),
        child_model_id: Some("model-a".to_string()),
    };
    let lineage_b = TaskLineageMetadata {
        parent_tool_call_id: Some("toolcall_000202".to_string()),
        parent_task_id: Some("task_000202".to_string()),
        parent_request_id: Some("req_000010".to_string()),
        parent_session_id: Some("agent_000001".to_string()),
        child_session_id: Some("agent_000102".to_string()),
        child_request_id: Some("req_000102".to_string()),
        child_provider_id: Some("mock".to_string()),
        child_model_id: Some("model-b".to_string()),
    };

    write_resume_fixture(
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
            resume_fixture_event(
                run_id,
                3,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000101".to_string(),
                    profile: "build".to_string(),
                    parent_agent_id: Some("agent_000001".to_string()),
                }),
            ),
            resume_fixture_event(
                run_id,
                4,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000102".to_string(),
                    profile: "librarian".to_string(),
                    parent_agent_id: Some("agent_000001".to_string()),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                5,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000010"),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000201".to_string(),
                    tool_id: "agent.spawn".to_string(),
                    args_summary: "{\"task\":\"child A\"}".to_string(),
                    args_digest: "digest-tool-a-req".to_string(),
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: None,
                        lineage: Some(lineage_a.clone()),
                        artifact_refs: Vec::new(),
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                6,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000010"),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000201".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("child A scheduled".to_string()),
                    output_digest: Some("digest-tool-a-out".to_string()),
                    output_json: None,
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: None,
                        lineage: Some(lineage_a),
                        artifact_refs: Vec::new(),
                        timing: Some(ExecutionTimingMetadata {
                            started_mono_ms: Some(5),
                            finished_mono_ms: Some(6),
                            elapsed_ms: Some(1),
                        }),
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                7,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000010"),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000202".to_string(),
                    tool_id: "agent.spawn".to_string(),
                    args_summary: "{\"task\":\"child B\"}".to_string(),
                    args_digest: "digest-tool-b-req".to_string(),
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: None,
                        lineage: Some(lineage_b.clone()),
                        artifact_refs: Vec::new(),
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                8,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000010"),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000202".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("child B scheduled".to_string()),
                    output_digest: Some("digest-tool-b-out".to_string()),
                    output_json: None,
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: None,
                        lineage: Some(lineage_b),
                        artifact_refs: Vec::new(),
                        timing: Some(ExecutionTimingMetadata {
                            started_mono_ms: Some(7),
                            finished_mono_ms: Some(8),
                            elapsed_ms: Some(1),
                        }),
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                9,
                EventActor::new(ActorKind::Worker, Some("agent_000101".to_string())),
                Some("req_000101"),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000301".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-a".to_string()),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                10,
                EventActor::new(ActorKind::Worker, Some("agent_000101".to_string())),
                Some("req_000101"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000101".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-a".to_string(),
                    prompt_summary: "child a prompt".to_string(),
                    request_digest: "digest-child-a".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                11,
                EventActor::new(ActorKind::Worker, Some("agent_000101".to_string())),
                Some("req_000101"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000301".to_string(),
                    result_summary: "child a done".to_string(),
                    result_digest: "digest-child-a-done".to_string(),
                    metadata: Some(TaskCompletionMetadata {
                        lineage: None,
                        task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                        timing: Some(ExecutionTimingMetadata {
                            started_mono_ms: Some(9),
                            finished_mono_ms: Some(60),
                            elapsed_ms: Some(51),
                        }),
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                12,
                EventActor::new(ActorKind::Worker, Some("agent_000102".to_string())),
                Some("req_000102"),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000302".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-b".to_string()),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                13,
                EventActor::new(ActorKind::Worker, Some("agent_000102".to_string())),
                Some("req_000102"),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000102".to_string(),
                    provider_id: "mock".to_string(),
                    model_id: "model-b".to_string(),
                    prompt_summary: "child b prompt".to_string(),
                    request_digest: "digest-child-b".to_string(),
                    metadata: None,
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                14,
                EventActor::new(ActorKind::Worker, Some("agent_000102".to_string())),
                Some("req_000102"),
                EventV1::TaskCancelled(TaskCancelledEvent {
                    task_id: "task_000302".to_string(),
                    reason: "cancelled while running".to_string(),
                    task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                15,
                EventActor::new(ActorKind::Worker, Some("agent_000102".to_string())),
                Some("req_000102"),
                EventV1::TaskResultLate(TaskResultLateEvent {
                    task_id: "task_000302".to_string(),
                    result_digest: "digest-child-b-late".to_string(),
                }),
            ),
            resume_fixture_event(
                run_id,
                16,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&temp_dir.path().join(run_id));

    assert_eq!(
        plan.latest_lifecycle_status,
        LifecycleSegmentStatus::Finished,
        "replay should preserve final lifecycle status"
    );

    let child_a = plan
        .child_sessions
        .get("agent_000101")
        .unwrap_or_abort();
    assert_eq!(child_a.parent_session_id.as_deref(), Some("agent_000001"));
    assert_eq!(
        child_a.parent_tool_call_id.as_deref(),
        Some("toolcall_000201")
    );
    assert_eq!(
        child_a.latest_child_request_id.as_deref(),
        Some("req_000101")
    );
    assert_eq!(child_a.provider_id.as_deref(), Some("mock"));
    assert_eq!(child_a.model_id.as_deref(), Some("model-a"));
    assert_eq!(
        child_a.terminal_state,
        Some(ChildSessionTerminalState::Completed)
    );
    assert_eq!(
        child_a.timing.as_ref().and_then(|timing| timing.elapsed_ms),
        Some(51)
    );

    let child_b = plan
        .child_sessions
        .get("agent_000102")
        .unwrap_or_abort();
    assert_eq!(child_b.parent_session_id.as_deref(), Some("agent_000001"));
    assert_eq!(
        child_b.parent_tool_call_id.as_deref(),
        Some("toolcall_000202")
    );
    assert_eq!(
        child_b.latest_child_request_id.as_deref(),
        Some("req_000102")
    );
    assert_eq!(child_b.provider_id.as_deref(), Some("mock"));
    assert_eq!(child_b.model_id.as_deref(), Some("model-b"));
    assert_eq!(
        child_b.terminal_state,
        Some(ChildSessionTerminalState::LateResult)
    );
    assert_eq!(
        child_b.timing.as_ref().and_then(|timing| timing.elapsed_ms),
        Some(3),
        "late-result terminal timing should be derived from scheduled start"
    );
}
