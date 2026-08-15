use harness_core::UnwrapOrAbort;
#[tokio::test]
async fn replay_preserves_native_tool_artifacts_and_task_lineage() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let workspace_root = temp_dir.path().join("workspace");
    fs::create_dir_all(&workspace_root).unwrap_or_abort();

    let coordinator = test_coordinator(temp_dir.path(), task_alias_registry());
    let run = coordinator
        .start_run("native_metadata_replay", workspace_root)
        .await
        .unwrap_or_abort();

    let tool_call_id = coordinator
        .request_tool_call(
            supervisor_actor(),
            Some("deep".to_string()),
            "task",
            json!({
                "child_session_id": "child-run-001",
                "child_request_id": "child-req-001",
            }),
        )
        .await
        .unwrap_or_abort();

    wait_for_tool_call_finish(&run.events_path, &tool_call_id).await;
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&run.events_path);

    let requested = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallRequested(payload) if payload.tool_call_id.as_str() == tool_call_id => {
                Some(payload)
            }
            _ => None,
        })
        .unwrap_or_abort();
    let requested_metadata = requested
        .metadata
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(
        requested_metadata.canonical_tool_id.as_deref(),
        Some("task")
    );
    assert_eq!(requested_metadata.alias_source_tool_id.as_deref(), None);

    let finished = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(payload) if payload.tool_call_id.as_str() == tool_call_id => {
                Some(payload)
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(finished.status, ToolCallStatus::Succeeded);
    let finished_metadata = finished
        .metadata
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(finished_metadata.canonical_tool_id.as_deref(), Some("task"));
    assert_eq!(finished_metadata.alias_source_tool_id.as_deref(), None);
    assert_eq!(
        finished_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.as_deref()),
        Some("child-run-001")
    );
    assert_eq!(
        finished_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_request_id.as_deref()),
        Some("child-req-001")
    );
    assert_eq!(finished_metadata.artifact_refs.len(), 1);
    let output_json = finished
        .output_json
        .as_ref()
        .unwrap_or_abort();
    let harness = output_json
        .get("_harness")
        .unwrap_or_abort();
    assert_eq!(
        harness
            .get("artifact_refs")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );

    let completed = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::TaskCompleted(payload) => Some(payload),
            _ => None,
        })
        .unwrap_or_abort();
    let completed_metadata = completed
        .metadata
        .as_ref()
        .unwrap_or_abort();
    assert_eq!(
        completed_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.parent_tool_call_id.as_deref()),
        Some(tool_call_id.as_str())
    );
    assert_eq!(
        completed_metadata
            .lineage
            .as_ref()
            .and_then(|lineage| lineage.child_session_id.as_deref()),
        Some("child-run-001")
    );
    assert!(completed_metadata.timing.is_some());

    let artifact_written = events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ArtifactWritten(payload)
                if payload.tool_call_id.as_ref().map(|id| id.as_str()) == Some(tool_call_id.as_str()) =>
            {
                Some(payload)
            }
            _ => None,
        })
        .unwrap_or_abort();
    assert_eq!(
        artifact_written
            .tool_metadata
            .as_ref()
            .and_then(|metadata| metadata.canonical_tool_id.as_deref()),
        Some("task")
    );

    let plan = inspect_resume_plan(&run.run_dir);
    let replay_tool = plan
        .tool_calls
        .get(&tool_call_id)
        .unwrap_or_abort();
    assert_eq!(replay_tool.tool_id.as_deref(), Some("task"));
    assert_eq!(
        replay_tool
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.invoked_tool_id.as_deref()),
        Some("task")
    );
    assert_eq!(
        replay_tool
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.effective_tool_id.as_deref()),
        Some("task")
    );
    assert_eq!(
        replay_tool
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.canonical_tool_id.as_deref()),
        Some("task")
    );
    assert_eq!(
        replay_tool
            .resolved_tool_identity
            .as_ref()
            .and_then(|identity| identity.alias_source_tool_id.as_deref()),
        None
    );
    assert_eq!(
        replay_tool.lifecycle_state,
        Some(harness_core::event::ToolCallLifecycleState::Completed)
    );
    assert_eq!(replay_tool.status, Some(ToolCallStatus::Succeeded));
    assert_eq!(
        replay_tool
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.canonical_tool_id.as_deref()),
        Some("task")
    );
    assert_eq!(
        replay_tool
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.alias_source_tool_id.as_deref()),
        None
    );
    assert_eq!(
        replay_tool
            .metadata
            .as_ref()
            .map(|metadata| metadata.artifact_refs.len()),
        Some(1)
    );

    let replay_task = plan
        .completed_tasks
        .get(completed.task_id.as_str())
        .unwrap_or_abort();
    assert_eq!(
        replay_task
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.lineage.as_ref())
            .and_then(|lineage| lineage.child_request_id.as_deref()),
        Some("child-req-001")
    );
}
#[tokio::test]
async fn legacy_sessions_remain_loadable_after_native_metadata_extension() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_legacy_native_metadata";
    let run_dir = temp_dir.path().join(run_id);
    write_events(
        &run_dir,
        &[
            envelope(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                run_id,
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                run_id,
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".into(),
                    provider_id: "mock".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "legacy prompt".to_string(),
                    request_digest: "digest-legacy".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                run_id,
                4,
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000001".into(),
                    tool_id: "shell.run".to_string(),
                    args_summary: "{\"cmd\":\"true\"}".to_string(),
                    args_digest: "digest-tool-args".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                run_id,
                5,
                EventV1::ToolCallStarted(ToolCallStartedEvent {
                    tool_call_id: "toolcall_000001".into(),
                }),
            ),
            envelope(
                run_id,
                6,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string().into(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("tool:shell.run".to_string()),
                    metadata: None,
                }),
            ),
            envelope(
                run_id,
                7,
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000001".to_string().into(),
                    result_summary: "legacy done".to_string(),
                    result_digest: "digest-task".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                run_id,
                8,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000001".into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("legacy ok".to_string()),
                    output_digest: Some("digest-tool-out".to_string()),
                    output_json: None,
                    metadata: None,
                }),
            ),
            envelope(
                run_id,
                9,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "legacy segment done".to_string(),
                }),
            ),
        ],
    );

    let legacy_plan = inspect_resume_plan(&run_dir);
    assert!(
        legacy_plan.is_resumable,
        "legacy run should remain resumable"
    );
    assert_eq!(
        legacy_plan
            .tool_calls
            .get("toolcall_000001")
            .and_then(|snapshot| snapshot.metadata.as_ref()),
        None,
        "legacy records should deserialize without requiring new metadata fields"
    );
    assert_eq!(
        legacy_plan
            .completed_tasks
            .get("task_000001")
            .and_then(|snapshot| snapshot.metadata.as_ref()),
        None,
        "legacy task completion records should remain valid without metadata"
    );

    let coordinator = test_coordinator(temp_dir.path(), Arc::new(ToolRegistry::new()));
    let resumed = coordinator
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_abort();
    coordinator.stop_run().await.unwrap_or_abort();

    let events = load_events(&resumed.events_path);
    assert!(
        events.len() > 9,
        "resume should append a new segment without rewriting old events"
    );
}
#[tokio::test]
async fn resume_projection_handles_checkpoint_between_turn_and_provider_restart() {
    // arrange
    // act
    // assert
    REPLAY_GUARD_TOOL_CALLS.store(0, Ordering::SeqCst);

    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_resume_checkpoint_between_turn_and_provider_restart";
    let run_dir = temp_dir.path().join(run_id);
    fs::create_dir_all(run_dir.join("artifacts/compactions/agent_000001"))
        .unwrap_or_abort();
    fs::write(
        run_dir.join("artifacts/compactions/agent_000001/checkpoint_000002.json"),
        serde_json::to_string(&ProviderContextCheckpoint {
            metadata: ProviderContextCheckpointMetadata {
                checkpoint_id: "checkpoint_000002".to_string(),
                agent_id: "agent_000001".to_string(),
                run_id: run_id.to_string(),
                through_seq: 3,
                through_request_id: Some("req_000001".to_string()),
                provider_id: Some("default".to_string()),
                model_id: Some("model-1".to_string()),
                tokens_before: Some(4_000),
                tokens_before_estimate: None,
                tokens_after_estimate: None,
                summary_tokens_estimate: None,
                compacted_turns: None,
                preserved_turns: None,
                reduction_tokens_estimate: None,
                reduction_percent_estimate: None,
                trigger_reason: Some("manual".to_string()),
            },
            summary: "checkpointed turn summary".to_string(),
            recent_turns: vec![ProviderConversationTurn {
                user_prompt: "turn before checkpoint".to_string(),
                assistant_response: "assistant before checkpoint".to_string(),
                ..Default::default()
            }],
            pruned_tool_artifacts: Vec::new(),
            facts: ProviderCompactionFacts::default(),
            tail_boundary: None,
            summary_source: None,
            timeline_entry: None,
        })
        .unwrap_or_abort(),
    )
    .unwrap_or_abort();

    write_events(
        &run_dir,
        &[
            envelope(
                run_id,
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                run_id,
                2,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".into(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "turn before checkpoint".to_string(),
                    request_digest: "digest-before".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                run_id,
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "checkpointed segment ended".to_string(),
                }),
            ),
            envelope(
                run_id,
                4,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                run_id,
                5,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                run_id,
                6,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000002".into(),
                    provider_id: "default".to_string(),
                    model_id: "model-1".to_string(),
                    prompt_summary: "turn after restart".to_string(),
                    request_digest: "digest-after".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                run_id,
                7,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "resumed segment finished".to_string(),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&run_dir);
    assert_eq!(plan.provider_model.as_deref(), Some("default/model-1"));
    assert!(plan.is_resumable);

    let coordinator = test_coordinator(temp_dir.path(), tool_registry_with_guard());
    let resumed = coordinator
        .resume_run(run_id, "interactive")
        .await
        .unwrap_or_abort();
    coordinator.stop_run().await.unwrap_or_abort();

    assert_eq!(REPLAY_GUARD_TOOL_CALLS.load(Ordering::SeqCst), 0);
    assert!(load_events(&resumed.events_path).len() > 6);
}
