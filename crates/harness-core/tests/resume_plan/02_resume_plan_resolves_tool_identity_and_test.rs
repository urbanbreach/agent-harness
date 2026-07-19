use harness_core::UnwrapOrAbort;
#[test]
fn resume_plan_resolves_tool_identity_and_lifecycle_without_tui_inference() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = temp_dir.path().join("run_tool_lifecycle_identity");
    write_events(
        &run_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000101".into(),
                    tool_id: "task".to_string(),
                    args_summary: "{\"prompt\":\"delegate\"}".to_string(),
                    args_digest: "digest-task-request".to_string(),
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: Some("task".to_string()),
                        lineage: None,
                        artifact_refs: Vec::new(),
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            envelope(
                3,
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000102".into(),
                    tool_id: "mcp.fixture.echo".to_string(),
                    args_summary: "{\"text\":\"hello\"}".to_string(),
                    args_digest: "digest-mcp-direct-request".to_string(),
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("mcp.fixture.echo".to_string()),
                        alias_source_tool_id: None,
                        lineage: None,
                        artifact_refs: Vec::new(),
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            envelope(
                4,
                EventV1::ToolCallStarted(ToolCallStartedEvent {
                    tool_call_id: "toolcall_000102".into(),
                }),
            ),
            envelope(
                5,
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000103".into(),
                    tool_id: "mcp.fixture.tool.call".to_string(),
                    args_summary: "{\"tool\":\"echo\"}".to_string(),
                    args_digest: "digest-mcp-wrapper-request".to_string(),
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("mcp.fixture.echo".to_string()),
                        alias_source_tool_id: None,
                        lineage: None,
                        artifact_refs: Vec::new(),
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            envelope(
                6,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000103".into(),
                    status: ToolCallStatus::Failed,
                    output_summary: Some("wrapper failed".to_string()),
                    output_digest: Some("digest-mcp-wrapper-failed".to_string()),
                    output_json: None,
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("mcp.fixture.echo".to_string()),
                        alias_source_tool_id: None,
                        lineage: None,
                        artifact_refs: Vec::new(),
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            envelope(
                7,
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000104".into(),
                    tool_id: "agent.spawn".to_string(),
                    args_summary: "{\"prompt\":\"native\"}".to_string(),
                    args_digest: "digest-native-request".to_string(),
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: None,
                        lineage: None,
                        artifact_refs: Vec::new(),
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            envelope(
                8,
                EventV1::ToolCallStarted(ToolCallStartedEvent {
                    tool_call_id: "toolcall_000104".into(),
                }),
            ),
            envelope(
                9,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000104".into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("native ok".to_string()),
                    output_digest: Some("digest-native-ok".to_string()),
                    output_json: None,
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: None,
                        lineage: None,
                        artifact_refs: Vec::new(),
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&run_dir);

    let pending_alias = plan
        .tool_calls
        .get("toolcall_000101")
        .unwrap_or_abort();
    assert_eq!(
        pending_alias.lifecycle_state,
        Some(ToolCallLifecycleState::Pending)
    );
    assert_eq!(pending_alias.status, None);
    assert_eq!(
        pending_alias
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.invoked_tool_id.as_deref()),
        Some("task")
    );
    assert_eq!(
        pending_alias
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.effective_tool_id.as_deref()),
        Some("agent.spawn")
    );
    assert_eq!(
        pending_alias
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.canonical_tool_id.as_deref()),
        Some("agent.spawn")
    );
    assert_eq!(
        pending_alias
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.alias_source_tool_id.as_deref()),
        Some("task")
    );

    let running_mcp_direct = plan
        .tool_calls
        .get("toolcall_000102")
        .unwrap_or_abort();
    assert_eq!(
        running_mcp_direct.lifecycle_state,
        Some(ToolCallLifecycleState::Running)
    );
    assert_eq!(running_mcp_direct.status, None);
    assert_eq!(
        running_mcp_direct
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.effective_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    assert_eq!(
        running_mcp_direct
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.canonical_tool_id.as_deref()),
        None
    );
    assert_eq!(
        running_mcp_direct
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.alias_source_tool_id.as_deref()),
        None
    );

    let error_mcp_wrapper = plan
        .tool_calls
        .get("toolcall_000103")
        .unwrap_or_abort();
    assert_eq!(
        error_mcp_wrapper.lifecycle_state,
        Some(ToolCallLifecycleState::Error)
    );
    assert_eq!(error_mcp_wrapper.status, Some(ToolCallStatus::Failed));
    assert_eq!(
        error_mcp_wrapper
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.invoked_tool_id.as_deref()),
        Some("mcp.fixture.tool.call")
    );
    assert_eq!(
        error_mcp_wrapper
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.effective_tool_id.as_deref()),
        Some("mcp.fixture.echo")
    );
    assert_eq!(
        error_mcp_wrapper
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.canonical_tool_id.as_deref()),
        None
    );

    let completed_native = plan
        .tool_calls
        .get("toolcall_000104")
        .unwrap_or_abort();
    assert_eq!(
        completed_native.lifecycle_state,
        Some(ToolCallLifecycleState::Completed)
    );
    assert_eq!(completed_native.status, Some(ToolCallStatus::Succeeded));
    assert_eq!(
        completed_native
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.effective_tool_id.as_deref()),
        Some("agent.spawn")
    );
    assert_eq!(
        completed_native
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.canonical_tool_id.as_deref()),
        Some("agent.spawn")
    );
}
#[test]
fn resume_plan_rejects_sessions_with_pending_permissions() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = temp_dir.path().join("run_pending_permission");
    write_events(
        &run_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".into(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5".to_string(),
                    prompt_summary: "prompt".to_string(),
                    request_digest: "digest".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                4,
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "perm_000001".to_string(),
                    kind: "shell".to_string(),
                    tool_call_id: None,
                    summary: "ask".to_string(),
                    request_digest: "digest-perm".to_string(),
                    timeout_ms: 1_000,
                    default_decision: PermissionDecision::Deny,
                }),
            ),
            envelope(
                5,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&run_dir);
    assert!(!plan.is_resumable);
    assert_eq!(
        plan.resume_disabled_reason.as_deref(),
        Some("pending permissions must be resolved")
    );
}
#[test]
fn resume_plan_rejects_sessions_with_tasks_in_flight() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = temp_dir.path().join("run_tasks_in_flight");
    write_events(
        &run_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".into(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5".to_string(),
                    prompt_summary: "prompt".to_string(),
                    request_digest: "digest".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                4,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string().into(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("tool:shell.run".to_string()),
                }),
            ),
            envelope(
                5,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&run_dir);
    assert!(!plan.is_resumable);
    assert_eq!(
        plan.resume_disabled_reason.as_deref(),
        Some("tasks are still in flight")
    );
}
#[test]
fn resume_plan_rejects_non_monotonic_or_corrupt_logs() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();

    let non_monotonic_dir = temp_dir.path().join("run_non_monotonic");
    write_events(
        &non_monotonic_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ],
    );

    let non_monotonic_plan = inspect_resume_plan(&non_monotonic_dir);
    assert!(!non_monotonic_plan.is_resumable);
    assert!(non_monotonic_plan
        .resume_disabled_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("corrupt or non-monotonic")));

    let corrupt_dir = temp_dir.path().join("run_corrupt");
    fs::create_dir_all(&corrupt_dir).unwrap_or_abort();
    let valid_first_line = serde_json::to_string(&envelope(
        1,
        EventV1::RunStarted(RunStartedEvent {
            run_name: "interactive".into(),
            workspace_root: "/workspace/project".to_string(),
        }),
    ))
    .unwrap_or_abort();
    fs::write(
        corrupt_dir.join("events.jsonl"),
        format!("{valid_first_line}\n{{bad-json}}\n"),
    )
    .unwrap_or_abort();

    let corrupt_plan = inspect_resume_plan(&corrupt_dir);
    assert!(!corrupt_plan.is_resumable);
    assert!(corrupt_plan
        .resume_disabled_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("invalid JSON event")));
}
