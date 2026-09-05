use harness_core::UnwrapOrAbort;
#[test]
fn resume_plan_uses_latest_lifecycle_segment_instead_of_any_terminal_event() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = temp_dir.path().join("run_latest_segment");
    write_events(
        &run_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
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
                    request_id: "req_000001".into(),
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
                    run_name: "interactive".into(),
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
                    request_id: "req_000002".into(),
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
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = temp_dir.path().join("run_open_quit_latest_segment");
    write_events(
        &run_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
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
                    request_id: "req_000001".into(),
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
                    run_name: "interactive".into(),
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
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
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
                    run_name: "interactive".into(),
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
                    request_id: "req_000001".into(),
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
                    tool_call_id: "toolcall_000777".into(),
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
                    tool_call_id: "toolcall_000777".into(),
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
                    run_name: "interactive".into(),
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
        .unwrap_or_abort();
    assert_eq!(child.parent_session_id.as_deref(), Some("agent_000001"));
    assert_eq!(
        child.parent_tool_call_id.as_deref(),
        Some("toolcall_000777")
    );
    assert_eq!(child.latest_child_request_id.as_deref(), Some("req_000777"));
}
#[test]
fn session_catalog_counts_checkpoint_artifacts_alongside_tool_artifacts() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = temp_dir.path().join("run_resume_checkpoint_artifacts");

    let mut tool_artifact = envelope(
        3,
        EventV1::ArtifactWritten(ArtifactWrittenEvent {
            path: "artifacts/toolcalls/toolcall_000001/result.json".to_string(),
            digest: "digest-tool".to_string(),
            bytes: 42,
            tool_call_id: Some("toolcall_000001".into()),
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

    let events = load_events(&run_dir.join("events.jsonl"));
    let entry = project_session_catalog_entry(
        events.iter(),
        "run_resume_fixture",
        None,
        Some("2026-04-23T00:00:00Z".to_string()),
        None,
    )
    .unwrap_or_abort();
    assert_eq!(entry.artifact_count, 2);
}
