use harness_core::UnwrapOrAbort;
#[test]
fn replay_suppresses_hooks_but_preserves_hook_history() {
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_id = "run_replay_hook_suppression";
    let side_effect_path = temp_dir.path().join("hook-side-effect.txt");
    let side_effect_digest = format!("digest:{}", side_effect_path.display());

    let hook_execution = HookExecutionMetadata {
        hook_name: "after_task".to_string(),
        status: HookExecutionStatus::Succeeded,
        hook_event: Some("task_completed".to_string()),
        command_digest: Some(side_effect_digest),
        output_digest: Some("hook-output-digest".to_string()),
        output_summary: Some("hook already executed live".to_string()),
        duration_ms: Some(12),
    };

    let lineage = TaskLineageMetadata {
        parent_tool_call_id: Some("toolcall_000401".to_string()),
        parent_task_id: Some("task_000401".to_string()),
        parent_request_id: Some("req_000401".to_string()),
        parent_session_id: Some("agent_000001".to_string()),
        child_session_id: Some("agent_000401".to_string()),
        child_request_id: Some("req_000401".to_string()),
        child_provider_id: Some("mock".to_string()),
        child_model_id: Some("model-hook".to_string()),
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
                    agent_id: "agent_000401".to_string(),
                    profile: "hook-runner".to_string(),
                    parent_agent_id: Some("agent_000001".to_string()),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                4,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000401"),
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000401".into(),
                    tool_id: "agent.spawn".to_string(),
                    args_summary: "{\"task\":\"run with hooks\"}".to_string(),
                    args_digest: "digest-hook-req".to_string(),
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: None,
                        lineage: Some(lineage.clone()),
                        artifact_refs: Vec::new(),
                        timing: None,
                        hook_executions: vec![hook_execution.clone()],
                    }),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                5,
                EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
                Some("req_000401"),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000401".into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("hook already executed live".to_string()),
                    output_digest: Some("digest-hook-finish".to_string()),
                    output_json: Some(json!({
                        "hook_executions": [
                            {
                                "hook_name": "after_task",
                                "status": "succeeded",
                                "hook_event": "task_completed",
                                "command_digest": "hook-command-digest",
                                "output_digest": "hook-output-digest",
                                "output_summary": "hook already executed live",
                                "duration_ms": 12
                            }
                        ]
                    })),
                    metadata: Some(ToolCallMetadata {
                        canonical_tool_id: Some("agent.spawn".to_string()),
                        alias_source_tool_id: None,
                        lineage: Some(lineage.clone()),
                        artifact_refs: Vec::new(),
                        timing: Some(ExecutionTimingMetadata {
                            started_mono_ms: Some(4),
                            finished_mono_ms: Some(5),
                            elapsed_ms: Some(1),
                        }),
                        hook_executions: vec![hook_execution.clone()],
                    }),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                6,
                EventActor::new(ActorKind::Worker, Some("agent_000401".to_string())),
                Some("req_000401"),
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000401".to_string().into(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:mock:model-hook".to_string()),
                }),
            ),
            resume_fixture_event_with_actor_and_correlation(
                run_id,
                7,
                EventActor::new(ActorKind::Worker, Some("agent_000401".to_string())),
                Some("req_000401"),
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000401".to_string().into(),
                    result_summary: "child done".to_string(),
                    result_digest: "digest-child-hook".to_string(),
                    metadata: Some(TaskCompletionMetadata {
                        lineage: Some(lineage),
                        task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                        timing: Some(ExecutionTimingMetadata {
                            started_mono_ms: Some(6),
                            finished_mono_ms: Some(7),
                            elapsed_ms: Some(1),
                        }),
                        hook_executions: vec![hook_execution.clone()],
                    }),
                }),
            ),
            resume_fixture_event(
                run_id,
                8,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "segment complete".to_string(),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&temp_dir.path().join(run_id));

    assert!(
        !side_effect_path.exists(),
        "replay must not execute historical hook side effects"
    );

    let tool_call_hooks = plan
        .tool_calls
        .get("toolcall_000401")
        .and_then(|snapshot| snapshot.metadata.as_ref())
        .map(|metadata| metadata.hook_executions.clone())
        .unwrap_or_abort();
    assert_eq!(tool_call_hooks, vec![hook_execution.clone()]);

    let completed_task_hooks = plan
        .completed_tasks
        .get("task_000401")
        .and_then(|snapshot| snapshot.metadata.as_ref())
        .map(|metadata| metadata.hook_executions.clone())
        .unwrap_or_abort();
    assert_eq!(completed_task_hooks, vec![hook_execution.clone()]);

    let child = plan
        .child_sessions
        .get("agent_000401")
        .unwrap_or_abort();
    assert_eq!(
        child.terminal_state,
        Some(ChildSessionTerminalState::Completed)
    );
    assert_eq!(child.hook_executions, vec![hook_execution]);
}
#[test]
fn replay_suppresses_hook_execution_but_preserves_hook_events() {
    replay_suppresses_hooks_but_preserves_hook_history();
}
#[tokio::test]
async fn hook_runner_is_suppressed_in_replay_and_deterministic_modes() {
    replay_suppresses_hooks_but_preserves_hook_history();
    deterministic_runs_suppress_live_hook_execution().await;
}
