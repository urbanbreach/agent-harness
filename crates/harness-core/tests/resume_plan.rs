use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use harness_core::event::{
    ActorKind, AgentSpawnedEvent, ArtifactWrittenEvent, EventActor, EventEnvelopeV1, EventV1,
    PermissionDecision, PermissionGrantRecordedEvent, PermissionRequestedEvent,
    PermissionResolvedEvent, ProviderAssistantMessageMetadata, ProviderRequestFinishedEvent,
    ProviderRequestFinishedMetadata, ProviderRequestStartedEvent, ProviderRequestStartedMetadata,
    ProviderThinkingMetadata, RunFinishedEvent, RunStartedEvent, TaskCompletedEvent,
    TaskLineageMetadata, TaskScheduleState, TaskScheduledEvent, ToolCallFinishedEvent,
    ToolCallLifecycleState, ToolCallMetadata, ToolCallRequestedEvent, ToolCallStartedEvent,
    ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::perm::{
    PermissionGrant, PermissionGrantMatcher, PermissionGrantRequest, PermissionGrantScope,
    PermissionKind, PermissionToolSelector,
};
use harness_core::proj::{
    inspect_resume_plan, project_run_summary, project_session_catalog_entry,
    LifecycleSegmentStatus, RunStatus,
};

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

#[test]
fn resume_plan_resolves_tool_identity_and_lifecycle_without_tui_inference() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_tool_lifecycle_identity");
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
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000101".to_string(),
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
                    tool_call_id: "toolcall_000102".to_string(),
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
                    tool_call_id: "toolcall_000102".to_string(),
                }),
            ),
            envelope(
                5,
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000103".to_string(),
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
                    tool_call_id: "toolcall_000103".to_string(),
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
                    tool_call_id: "toolcall_000104".to_string(),
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
                    tool_call_id: "toolcall_000104".to_string(),
                }),
            ),
            envelope(
                9,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000104".to_string(),
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
        .expect("pending alias tool snapshot");
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
        .expect("running direct MCP tool snapshot");
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
        .expect("failed wrapper MCP tool snapshot");
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
        .expect("completed native tool snapshot");
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
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_pending_permission");
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
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
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
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_tasks_in_flight");
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
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
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
                    task_id: "task_000001".to_string(),
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
    let temp_dir = tempfile::tempdir().expect("tempdir");

    let non_monotonic_dir = temp_dir.path().join("run_non_monotonic");
    write_events(
        &non_monotonic_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
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
    fs::create_dir_all(&corrupt_dir).expect("create run dir");
    let valid_first_line = serde_json::to_string(&envelope(
        1,
        EventV1::RunStarted(RunStartedEvent {
            run_name: "interactive".to_string(),
            workspace_root: "/workspace/project".to_string(),
        }),
    ))
    .expect("serialize first event");
    fs::write(
        corrupt_dir.join("events.jsonl"),
        format!("{valid_first_line}\n{{bad-json}}\n"),
    )
    .expect("write corrupt events");

    let corrupt_plan = inspect_resume_plan(&corrupt_dir);
    assert!(!corrupt_plan.is_resumable);
    assert!(corrupt_plan
        .resume_disabled_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("invalid JSON event")));
}

#[test]
fn resume_plan_uses_latest_lifecycle_segment_instead_of_any_terminal_event() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_latest_segment");
    write_events(
        &run_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/older".to_string(),
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
                    request_id: "req_000001".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5".to_string(),
                    prompt_summary: "old".to_string(),
                    request_digest: "digest-old".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "first segment done".to_string(),
                }),
            ),
            envelope(
                5,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/newer".to_string(),
                }),
            ),
            envelope(
                6,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000002".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                7,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000002".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5".to_string(),
                    prompt_summary: "new".to_string(),
                    request_digest: "digest-new".to_string(),
                    metadata: None,
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&run_dir);

    assert_eq!(plan.latest_lifecycle_status, LifecycleSegmentStatus::Active);
    assert_eq!(plan.workspace_root.as_deref(), Some("/workspace/newer"));
    assert_eq!(
        plan.known_agents.keys().cloned().collect::<Vec<_>>(),
        vec!["agent_000002".to_string()]
    );
    assert!(!plan.is_resumable);
    assert_eq!(
        plan.resume_disabled_reason.as_deref(),
        Some("run is still active")
    );
}

#[test]
fn resume_plan_keeps_provider_model_after_open_and_quit_resumed_segment() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_open_quit_latest_segment");
    write_events(
        &run_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/original".to_string(),
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
                    request_id: "req_000001".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5".to_string(),
                    prompt_summary: "hello".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "first segment done".to_string(),
                }),
            ),
            envelope(
                5,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/resumed".to_string(),
                }),
            ),
            envelope(
                6,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                7,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "resumed segment quit without prompt".to_string(),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&run_dir);

    assert_eq!(
        plan.latest_lifecycle_status,
        LifecycleSegmentStatus::Finished
    );
    assert_eq!(plan.workspace_root.as_deref(), Some("/workspace/resumed"));
    assert_eq!(
        plan.known_agents.get("agent_000001").map(String::as_str),
        Some("default")
    );
    assert_eq!(plan.provider_model.as_deref(), Some("default/gpt-5"));
    assert!(
        plan.is_resumable,
        "open-and-quit resumed segment should remain resumable"
    );
}

#[test]
fn resume_plan_preserves_child_session_lineage_across_open_and_quit_resumed_segment() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_child_lineage_open_quit");
    let lineage = TaskLineageMetadata {
        parent_tool_call_id: Some("toolcall_000777".to_string()),
        parent_task_id: Some("task_000777".to_string()),
        parent_request_id: Some("req_000001".to_string()),
        parent_session_id: Some("agent_000001".to_string()),
        child_session_id: Some("agent_000777".to_string()),
        child_request_id: Some("req_000777".to_string()),
        child_provider_id: Some("default".to_string()),
        child_model_id: Some("gpt-5".to_string()),
    };
    write_events(
        &run_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/original".to_string(),
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
                    request_id: "req_000001".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5".to_string(),
                    prompt_summary: "parent turn".to_string(),
                    request_digest: "digest-parent-turn".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                4,
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000777".to_string(),
                    tool_id: "agent.spawn".to_string(),
                    args_summary: "{\"task\":\"spawn child\"}".to_string(),
                    args_digest: "digest-child-spawn-req".to_string(),
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: None,
                        lineage: Some(lineage.clone()),
                        artifact_refs: Vec::new(),
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            envelope(
                5,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000777".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("child spawned".to_string()),
                    output_digest: Some("digest-child-spawn-finished".to_string()),
                    output_json: None,
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: None,
                        lineage: Some(lineage),
                        artifact_refs: Vec::new(),
                        timing: None,
                        hook_executions: Vec::new(),
                    }),
                }),
            ),
            envelope(
                6,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "first segment done".to_string(),
                }),
            ),
            envelope(
                7,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/resumed".to_string(),
                }),
            ),
            envelope(
                8,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                9,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "resumed segment quit without prompt".to_string(),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&run_dir);
    let child = plan
        .child_sessions
        .get("agent_000777")
        .expect("child lineage should survive open-and-quit resumed segment");
    assert_eq!(child.parent_session_id.as_deref(), Some("agent_000001"));
    assert_eq!(
        child.parent_tool_call_id.as_deref(),
        Some("toolcall_000777")
    );
    assert_eq!(child.latest_child_request_id.as_deref(), Some("req_000777"));
}

#[test]
fn session_catalog_counts_checkpoint_artifacts_alongside_tool_artifacts() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_resume_checkpoint_artifacts");

    let mut tool_artifact = envelope(
        3,
        EventV1::ArtifactWritten(ArtifactWrittenEvent {
            path: "artifacts/toolcalls/toolcall_000001/result.json".to_string(),
            digest: "digest-tool".to_string(),
            bytes: 42,
            tool_call_id: Some("toolcall_000001".to_string()),
            tool_metadata: None,
            metadata: BTreeMap::new(),
        }),
    );
    tool_artifact.correlation_id = Some("req_000001".to_string());

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
            tool_artifact,
            envelope(
                4,
                EventV1::ArtifactWritten(ArtifactWrittenEvent {
                    path: "artifacts/compactions/agent_000001/checkpoint_000004.json".to_string(),
                    digest: "digest-checkpoint".to_string(),
                    bytes: 84,
                    tool_call_id: None,
                    tool_metadata: None,
                    metadata: BTreeMap::from([
                        (
                            "artifact_kind".to_string(),
                            "provider_context_checkpoint".to_string(),
                        ),
                        ("checkpoint_id".to_string(), "checkpoint_000004".to_string()),
                        ("agent_id".to_string(), "agent_000001".to_string()),
                        ("summary_contract_version".to_string(), "2".to_string()),
                        ("read_file_count".to_string(), "3".to_string()),
                        ("modified_file_count".to_string(), "1".to_string()),
                    ]),
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
    assert_eq!(plan.session_artifacts.len(), 2);
    assert!(plan.session_artifacts.values().any(|artifact| {
        artifact.artifact_kind.as_deref() == Some("provider_context_checkpoint")
            && artifact.tool_call_id.is_none()
            && artifact.summary_contract_version == Some(2)
            && artifact.read_file_count == Some(3)
            && artifact.modified_file_count == Some(1)
    }));

    let events_path = run_dir.join("events.jsonl");
    let body = fs::read_to_string(&events_path).expect("read events");
    let events = body
        .lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).expect("valid event"))
        .collect::<Vec<_>>();
    let entry = project_session_catalog_entry(
        events.iter(),
        "run_resume_fixture",
        None,
        Some("2026-04-23T00:00:00Z".to_string()),
        None,
    )
    .expect("project session catalog entry");
    assert_eq!(entry.artifact_count, 2);
}

fn envelope(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: "run_resume_fixture".to_string(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some("run:run_resume_fixture".to_string()),
        payload,
    }
}

fn write_events(run_dir: &Path, events: &[EventEnvelopeV1]) {
    fs::create_dir_all(run_dir).expect("create run directory");
    let mut body = String::new();
    for event in events {
        let line = serde_json::to_string(event).expect("serialize event line");
        body.push_str(&line);
        body.push('\n');
    }
    fs::write(run_dir.join("events.jsonl"), body).expect("write events file");
}
