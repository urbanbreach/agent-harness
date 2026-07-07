use harness::UnwrapOrAbort;
#[test]
fn sessions_list_cli_prints_json_entries() {
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_json");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();

    write_events_jsonl(&run_dir, &delegated_recovery_events("run_json"));

    let output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "list",
            "--json",
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let rows: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_abort();
    assert_eq!(rows.as_array().map(Vec::len), Some(1));
    let row = &rows[0];
    assert_eq!(row["run_dir"], run_dir.to_str().unwrap_or_abort());
    assert_eq!(row["run_id"], "run_json");
    assert_eq!(row["run_name"], "recovery-fixture");
    assert_eq!(row["status"], "finished");
    assert_eq!(row["profile_preset"], "worker");
    assert_eq!(row["provider_model"], serde_json::Value::Null);
    assert_eq!(row["mode_source"], "unknown");
    assert_eq!(row["is_resumable"], false);
    assert_eq!(row["artifact_count"], 1);
    assert_eq!(row["child_session_count"], 1);
    assert_eq!(row["parent_session_id"], "agent_supervisor");
}
#[test]
fn sessions_reopen_json_surfaces_prompt_context_child_sessions_and_artifacts() {
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("resume_fixture_dir");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();

    let mut events = vec![
        envelope(
            "run_resume_fixture",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            "run_resume_fixture",
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "worker".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope(
            "run_resume_fixture",
            3,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_000001".to_string(),
                text: "Recover this session headlessly".to_string(),
            }),
        ),
        envelope_with_actor(
            "run_resume_fixture",
            4,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-4o-mini".to_string(),
                prompt_summary: "Recover this session headlessly".to_string(),
                request_digest: "digest-user".to_string(),
                metadata: None,
            }),
        ),
    ];
    let mut completed_parent_turn = envelope_with_actor(
        "run_resume_fixture",
        5,
        EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000099".to_string(),
            result_summary: "Recovered summary".to_string(),
            result_digest: "digest-parent".to_string(),
            metadata: None,
        }),
    );
    completed_parent_turn.correlation_id = Some("req_000001".to_string());
    events.push(completed_parent_turn);
    events.push(envelope(
        "run_resume_fixture",
        6,
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: "agent_000002".to_string(),
            profile: "worker".to_string(),
            parent_agent_id: Some("agent_000001".to_string()),
        }),
    ));
    events.push(envelope_with_actor(
        "run_resume_fixture",
        7,
        EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "toolcall_000001".to_string(),
            tool_id: "fs.read".to_string(),
            args_summary: "read report.txt".to_string(),
            args_digest: "digest-tool".to_string(),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("fs.read".to_string()),
                alias_source_tool_id: None,
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: None,
                    parent_task_id: Some("task_000001".to_string()),
                    parent_request_id: Some("req_000001".to_string()),
                    parent_session_id: Some("agent_000001".to_string()),
                    child_session_id: Some("agent_000002".to_string()),
                    child_request_id: Some("req_000002".to_string()),
                    child_provider_id: Some("default".to_string()),
                    child_model_id: Some("gpt-4o-mini".to_string()),
                }),
                artifact_refs: vec![EventArtifactRef {
                    path: "artifacts/report.txt".to_string(),
                    digest: Some("digest-report".to_string()),
                }],
                timing: Some(ExecutionTimingMetadata {
                    started_mono_ms: Some(6),
                    finished_mono_ms: Some(7),
                    elapsed_ms: Some(1),
                }),
                hook_executions: Vec::new(),
            }),
        }),
    ));
    events.push(envelope_with_actor(
        "run_resume_fixture",
        8,
        EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "toolcall_000001".to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("read artifact".to_string()),
            output_digest: Some("digest-output".to_string()),
            output_json: None,
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("fs.read".to_string()),
                alias_source_tool_id: None,
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: None,
                    parent_task_id: Some("task_000001".to_string()),
                    parent_request_id: Some("req_000001".to_string()),
                    parent_session_id: Some("agent_000001".to_string()),
                    child_session_id: Some("agent_000002".to_string()),
                    child_request_id: Some("req_000002".to_string()),
                    child_provider_id: Some("default".to_string()),
                    child_model_id: Some("gpt-4o-mini".to_string()),
                }),
                artifact_refs: vec![EventArtifactRef {
                    path: "artifacts/report.txt".to_string(),
                    digest: Some("digest-report".to_string()),
                }],
                timing: Some(ExecutionTimingMetadata {
                    started_mono_ms: Some(6),
                    finished_mono_ms: Some(7),
                    elapsed_ms: Some(1),
                }),
                hook_executions: Vec::new(),
            }),
        }),
    ));
    events.push(envelope_with_actor(
        "run_resume_fixture",
        9,
        EventActor::new(ActorKind::Worker, Some("agent_000002".to_string())),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_000001".to_string(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:gpt-4o-mini".to_string()),
        }),
    ));
    events.push(envelope_with_actor(
        "run_resume_fixture",
        10,
        EventActor::new(ActorKind::Worker, Some("agent_000002".to_string())),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000001".to_string(),
            result_summary: "child completed".to_string(),
            result_digest: "digest-child".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("toolcall_000001".to_string()),
                    parent_task_id: Some("task_000001".to_string()),
                    parent_request_id: Some("req_000001".to_string()),
                    parent_session_id: Some("agent_000001".to_string()),
                    child_session_id: Some("agent_000002".to_string()),
                    child_request_id: Some("req_000002".to_string()),
                    child_provider_id: Some("default".to_string()),
                    child_model_id: Some("gpt-4o-mini".to_string()),
                }),
                task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                timing: Some(ExecutionTimingMetadata {
                    started_mono_ms: Some(8),
                    finished_mono_ms: Some(9),
                    elapsed_ms: Some(1),
                }),
                hook_executions: Vec::new(),
            }),
        }),
    ));
    events.push(envelope(
        "run_resume_fixture",
        11,
        EventV1::RunFinished(RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));

    write_events_jsonl(&run_dir, &events);

    let output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "reopen",
            "--session",
            "run_resume_fixture",
            "--json",
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let summary: serde_json::Value =
        serde_json::from_slice(&output.stdout).unwrap_or_abort();
    assert_eq!(summary["run_id"], "run_resume_fixture");
    assert_eq!(summary["resumable"], true);
    assert_eq!(summary["resume_agent_id"], "agent_000002");
    assert_eq!(
        summary["continue_hint"],
        "harness prompt --resume run_resume_fixture --text \"<next prompt>\""
    );
    assert_eq!(
        summary["prompt_context"][0]["text"],
        "Recover this session headlessly"
    );
    assert_eq!(
        summary["prompt_context"][1]["text"],
        "Recover this session headlessly"
    );
    assert_eq!(
        summary["child_sessions"][0]["parent_tool_call_id"],
        serde_json::Value::Null
    );
    assert_eq!(
        summary["child_sessions"][1]["parent_tool_call_id"],
        "toolcall_000001"
    );
    assert_eq!(summary["artifacts"][0]["path"], "artifacts/report.txt");
}
#[test]
fn sessions_surfaces_checkpoint_artifacts_in_catalog_and_recovery_views() {
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_checkpoint_artifacts");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();

    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_checkpoint_artifacts",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_checkpoint_artifacts",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "worker".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                "run_checkpoint_artifacts",
                3,
                EventV1::ArtifactWritten(ArtifactWrittenEvent {
                    path: "artifacts/compactions/agent_000001/checkpoint_000003.json".to_string(),
                    digest: "digest-checkpoint".to_string(),
                    bytes: 84,
                    tool_call_id: None,
                    tool_metadata: None,
                    metadata: BTreeMap::from([
                        (
                            "artifact_kind".to_string(),
                            "provider_context_checkpoint".to_string(),
                        ),
                        ("checkpoint_id".to_string(), "checkpoint_000003".to_string()),
                        ("agent_id".to_string(), "agent_000001".to_string()),
                    ]),
                }),
            ),
            envelope(
                "run_checkpoint_artifacts",
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let list_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "list",
            "--json",
        ]);

    assert!(
        list_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&list_output.stderr)
    );
    let rows: serde_json::Value =
        serde_json::from_slice(&list_output.stdout).unwrap_or_abort();
    assert_eq!(rows.as_array().map(Vec::len), Some(1));
    assert_eq!(rows[0]["run_id"], "run_checkpoint_artifacts");
    assert_eq!(rows[0]["artifact_count"], 1);

    let reopen_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "reopen",
            "--session",
            "run_checkpoint_artifacts",
            "--json",
        ]);

    assert!(
        reopen_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&reopen_output.stderr)
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&reopen_output.stdout).unwrap_or_abort();
    assert_eq!(
        summary["artifacts"][0]["path"],
        "artifacts/compactions/agent_000001/checkpoint_000003.json"
    );
    assert_eq!(
        summary["artifacts"][0]["kind"],
        "provider_context_checkpoint"
    );
    assert_eq!(
        summary["artifacts"][0]["tool_call_id"],
        serde_json::Value::Null
    );
}
