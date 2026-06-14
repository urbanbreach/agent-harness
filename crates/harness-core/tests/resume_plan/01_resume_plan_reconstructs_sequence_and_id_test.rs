#[test]
fn resume_plan_reconstructs_sequence_and_id_watermarks() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_resume_ok");
    write_events(
        &run_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
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
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_000003".to_string(),
                    text: "hello".to_string(),
                }),
            ),
            envelope(
                4,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000004".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5".to_string(),
                    prompt_summary: "prompt".to_string(),
                    request_digest: "digest-req".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                5,
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000002".to_string(),
                    tool_id: "shell.run".to_string(),
                    args_summary: "{\"cmd\":\"true\"}".to_string(),
                    args_digest: "digest-tool-args".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                6,
                EventV1::ToolCallStarted(ToolCallStartedEvent {
                    tool_call_id: "toolcall_000002".to_string(),
                }),
            ),
            envelope(
                7,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000002".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("ok".to_string()),
                    output_digest: Some("digest-tool-out".to_string()),
                    output_json: None,
                    metadata: None,
                }),
            ),
            envelope(
                8,
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "perm_000002".to_string(),
                    kind: "shell".to_string(),
                    tool_call_id: Some("toolcall_000002".to_string()),
                    summary: "allow shell".to_string(),
                    request_digest: "digest-perm".to_string(),
                    timeout_ms: 1_000,
                    default_decision: PermissionDecision::Deny,
                }),
            ),
            envelope(
                9,
                EventV1::PermissionResolved(PermissionResolvedEvent {
                    permission_id: "perm_000002".to_string(),
                    decision: PermissionDecision::Allow,
                    reason: Some("approved".to_string()),
                }),
            ),
            envelope(
                10,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000004".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("tool:shell.run".to_string()),
                }),
            ),
            envelope(
                11,
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000004".to_string(),
                    result_summary: "done".to_string(),
                    result_digest: "digest-task".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                12,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&run_dir);

    assert_eq!(plan.max_seq, 12);
    assert_eq!(plan.id_watermarks.max_request_id, 4);
    assert_eq!(plan.id_watermarks.max_task_id, 4);
    assert_eq!(plan.id_watermarks.max_tool_call_id, 2);
    assert_eq!(plan.id_watermarks.max_permission_id, 2);
    assert_eq!(
        plan.latest_lifecycle_status,
        LifecycleSegmentStatus::Finished
    );
    assert_eq!(
        plan.known_agents.get("agent_000001").map(String::as_str),
        Some("default")
    );
    assert!(plan.pending_permissions.is_empty());
    assert!(plan.tasks_in_flight.is_empty());
    assert_eq!(plan.workspace_root.as_deref(), Some("/workspace/project"));
    assert_eq!(plan.provider_model.as_deref(), Some("default/gpt-5"));
    assert!(plan.is_resumable);
    assert_eq!(plan.resume_disabled_reason, None);
}
#[test]
fn resume_plan_preserves_run_scoped_permission_grants_across_resume_markers() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_permission_grant_resume");
    let grant = PermissionGrant {
        grant_id: "grant_perm_000001".to_string(),
        permission_id: "perm_000001".to_string(),
        scope: PermissionGrantScope::Run,
        expires_at: None,
        kind: PermissionKind::Shell,
        tool: PermissionToolSelector {
            effective_tool_id: "shell.run".to_string(),
            canonical_tool_id: Some("shell.run".to_string()),
        },
        matcher: PermissionGrantMatcher::RequestDigest {
            request_digest: "digest-perm".to_string(),
        },
    };
    write_events(
        &run_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::PermissionGrantRecorded(PermissionGrantRecordedEvent {
                    grant: grant.clone(),
                }),
            ),
            envelope(
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
            envelope(
                4,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "continued".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                5,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "continued finished".to_string(),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&run_dir);
    let request = PermissionGrantRequest {
        kind: PermissionKind::Shell,
        tool: grant.tool,
        matcher: PermissionGrantMatcher::RequestDigest {
            request_digest: "digest-perm".to_string(),
        },
    };

    assert!(plan.active_permission_grants.authorizes(&request));
}
#[test]
fn replay_old_loop_events_without_provider_metadata() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_old_loop_events");
    write_events(
        &run_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5".to_string(),
                    prompt_summary: "prompt".to_string(),
                    request_digest: "digest-req".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                3,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: None,
                }),
            ),
            envelope(
                4,
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string(),
                    result_summary: "done".to_string(),
                    result_digest: "digest-task".to_string(),
                    metadata: None,
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
    assert_eq!(plan.provider_model.as_deref(), Some("default/gpt-5"));
    assert!(plan.tool_calls.is_empty());
    assert!(plan.completed_tasks.contains_key("task_000001"));
}
#[test]
fn replay_new_loop_metadata_is_non_semantic_for_run_summary() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let base_dir = temp_dir.path().join("run_metadata_equivalence");
    let legacy_dir = base_dir.join("legacy");
    let metadata_dir = base_dir.join("metadata");

    let legacy_events = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/workspace/project".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-5".to_string(),
                prompt_summary: "prompt".to_string(),
                request_digest: "digest-req".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            3,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_000001".to_string(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        envelope(
            4,
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_000001".to_string(),
                state: TaskScheduleState::Started,
                queue_key: Some("tool:shell.run".to_string()),
            }),
        ),
        envelope(
            5,
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_000001".to_string(),
                result_summary: "done".to_string(),
                result_digest: "digest-task".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            6,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished".to_string(),
            }),
        ),
    ];

    let metadata_events = vec![
        envelope(
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/workspace/project".to_string(),
            }),
        ),
        envelope(
            2,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-5".to_string(),
                prompt_summary: "prompt".to_string(),
                request_digest: "digest-req".to_string(),
                metadata: Some(ProviderRequestStartedMetadata {
                    turn_id: Some("turn_000001".to_string()),
                    provider_call_id: Some("provider-call-redacted".to_string()),
                    provider_session_id: Some("provider-session-digest".to_string()),
                    provider_cache_id: Some("provider-cache-digest".to_string()),
                    retry: None,
                }),
            }),
        ),
        envelope(
            3,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_000001".to_string(),
                finish_reason: "done".to_string(),
                output_digest: Some("digest-output".to_string()),
                usage: None,
                metadata: Some(ProviderRequestFinishedMetadata {
                    turn_id: Some("turn_000001".to_string()),
                    provider_call_id: Some("provider-call-redacted".to_string()),
                    provider_response_id: Some("provider-response-redacted".to_string()),
                    provider_session_id: Some("provider-session-digest".to_string()),
                    provider_cache_id: Some("provider-cache-digest".to_string()),
                    provider_stop_reason: Some("stop".to_string()),
                    cache_read_tokens: Some(11),
                    cache_write_tokens: Some(7),
                    assistant_message: Some(ProviderAssistantMessageMetadata {
                        message_id: Some("assistant-message-redacted".to_string()),
                        text_digest: Some("digest-output".to_string()),
                        reasoning_digest: Some("digest-reasoning".to_string()),
                    }),
                    thinking: Some(ProviderThinkingMetadata {
                        summary: Some("redacted reasoning summary".to_string()),
                        summary_digest: Some("digest-thinking-summary".to_string()),
                        signature: Some("thinking-signature-redacted".to_string()),
                    }),
                    provider_error_category: None,
                    provider_error_remediation: None,
                }),
            }),
        ),
        envelope(
            4,
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_000001".to_string(),
                state: TaskScheduleState::Started,
                queue_key: Some("tool:shell.run".to_string()),
            }),
        ),
        envelope(
            5,
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_000001".to_string(),
                result_summary: "done".to_string(),
                result_digest: "digest-task".to_string(),
                metadata: Some(harness_core::event::TaskCompletionMetadata {
                    lineage: Some(TaskLineageMetadata {
                        parent_tool_call_id: Some("toolcall_000001".to_string()),
                        parent_task_id: None,
                        parent_request_id: Some("req_000001".to_string()),
                        parent_session_id: None,
                        child_session_id: Some("agent_000002".to_string()),
                        child_request_id: Some("req_000002".to_string()),
                        child_provider_id: Some("default".to_string()),
                        child_model_id: Some("gpt-5".to_string()),
                    }),
                    task_scope: None,
                    timing: None,
                    hook_executions: Vec::new(),
                }),
            }),
        ),
        envelope(
            6,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished".to_string(),
            }),
        ),
    ];

    write_events(&legacy_dir, &legacy_events);
    write_events(&metadata_dir, &metadata_events);

    let legacy_summary = project_run_summary(legacy_events.iter()).expect("legacy summary");
    let metadata_summary = project_run_summary(metadata_events.iter()).expect("metadata summary");

    assert_eq!(legacy_summary, metadata_summary);
    assert_eq!(legacy_summary.status, RunStatus::Finished);
    assert!(legacy_summary.tasks_in_flight.is_empty());
    assert!(legacy_summary.pending_permissions.is_empty());
}
