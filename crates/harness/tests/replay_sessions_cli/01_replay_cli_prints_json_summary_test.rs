#[test]
fn replay_cli_prints_json_summary() {
    let run_dir = tempdir().expect("tempdir");
    write_events_jsonl(
        run_dir.path(),
        &[
            envelope(
                "run_replay_json",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "json-fixture".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_replay_json",
                2,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_123".to_string(),
                    state: TaskScheduleState::Queued,
                    queue_key: Some("deep/default:gpt-5.4-mini".to_string()),
                }),
            ),
            envelope(
                "run_replay_json",
                3,
                EventV1::RunFailed(RunFailedEvent {
                    error: "boom".to_string(),
                }),
            ),
        ],
    );

    let output = run_harness([
            "replay",
            "--session",
            run_dir.path().to_str().expect("run dir utf-8"),
            "--json",
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("replay json output should parse");
    assert_eq!(summary["run_id"], "run_replay_json");
    assert_eq!(summary["run_name"], "json-fixture");
    assert_eq!(summary["status"], "failed");
    assert_eq!(summary["last_error"], "boom");
    assert_eq!(summary["tasks_in_flight"], serde_json::json!(["task_123"]));
}
#[test]
fn replay_cli_prints_human_summary() {
    let run_dir = tempdir().expect("tempdir");
    write_events_jsonl(
        run_dir.path(),
        &[
            envelope(
                "run_replay_human",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "human-fixture".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_replay_human",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let output = run_harness([
            "replay",
            "--session",
            run_dir.path().to_str().expect("run dir utf-8"),
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("run_id: run_replay_human"));
    assert!(stdout.contains("run_name: human-fixture"));
    assert!(stdout.contains("session_path:"));
    assert!(stdout.contains("status: Finished"));
    assert!(stdout.contains("next_steps:"));
    assert!(stdout.contains("counts:"));
    assert!(stdout.contains("artifacts: 0"));
    assert!(stdout.contains("child_sessions: 0"));
}
#[test]
fn replay_cli_surfaces_recovery_story_details_from_resume_metadata() {
    let run_dir = tempdir().expect("tempdir");
    write_events_jsonl(
        run_dir.path(),
        &delegated_recovery_events("run_recovery_replay"),
    );

    let human = run_harness([
            "replay",
            "--session",
            run_dir.path().to_str().expect("run dir utf-8"),
        ]);

    assert!(
        human.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&human.stderr)
    );
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert!(stdout.contains("artifacts: 1"));
    assert!(stdout.contains("artifacts/delegated/task-output.json"));
    assert!(stdout.contains("tool_call=toolcall_1"));
    assert!(stdout.contains("canonical=agent.spawn"));
    assert!(stdout.contains("alias=task"));
    assert!(stdout.contains("child_session=child-run-001"));
    assert!(stdout.contains("child_sessions: 1"));
    assert!(stdout.contains("child-run-001"));
    assert!(stdout.contains("parent_tool=toolcall_1"));
    assert!(stdout.contains("provider_model=openai/gpt-5.4-mini"));
    assert!(stdout.contains("notification=completed"));
    assert!(stdout.contains("notification_summary=background child completed"));
    assert!(stdout.contains("artifacts=artifacts/delegated/task-output.json"));
    assert!(stdout.contains("next_actions:"));
    assert!(stdout.contains("background_output(request_id=\"child-req-001\", block=false)"));
    assert!(stdout.contains("task(session_id=\"child-run-001\""));

    let json = run_harness([
            "replay",
            "--session",
            run_dir.path().to_str().expect("run dir utf-8"),
            "--json",
        ]);

    assert!(
        json.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&json.stderr)
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("replay recovery json should parse");
    assert_eq!(summary["artifact_count"], 1);
    assert_eq!(summary["child_session_count"], 1);
    assert_eq!(
        summary["session_path"],
        run_dir.path().to_str().expect("run dir utf-8")
    );
    assert_eq!(
        summary["artifacts"][0]["path"],
        "artifacts/delegated/task-output.json"
    );
    assert_eq!(summary["artifacts"][0]["tool_call_id"], "toolcall_1");
    assert_eq!(summary["artifacts"][0]["canonical_tool_id"], "agent.spawn");
    assert_eq!(summary["artifacts"][0]["alias_source_tool_id"], "task");
    assert_eq!(summary["artifacts"][0]["child_session_id"], "child-run-001");
    assert_eq!(
        summary["child_sessions"][0]["child_session_id"],
        "child-run-001"
    );
    assert_eq!(
        summary["child_sessions"][0]["provider_model"],
        "openai/gpt-5.4-mini"
    );
    assert_eq!(
        summary["child_sessions"][0]["artifact_paths"][0],
        "artifacts/delegated/task-output.json"
    );
    assert_eq!(
        summary["child_sessions"][0]["notification_status"],
        "completed"
    );
    assert_eq!(
        summary["child_sessions"][0]["notification_summary"],
        "background child completed"
    );
    assert_eq!(
        summary["child_sessions"][0]["notification_terminal_event_id"],
        "evt-0006"
    );
    assert_eq!(
        summary["child_sessions"][0]["next_actions"][0],
        "background_output(request_id=\"child-req-001\", block=false)"
    );
    assert!(summary["child_sessions"][0]["next_actions"]
        .as_array()
        .expect("child next actions")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .any(|action| action.contains("task(session_id=\"child-run-001\"")));
}
#[test]
fn replay_cli_sanitizes_control_char_metadata_in_human_output() {
    let run_dir = tempdir().expect("tempdir");
    write_events_jsonl(
        run_dir.path(),
        &delegated_recovery_events_with_control_chars("run_recovery_controls"),
    );

    let json_output = run_harness([
            "replay",
            "--session",
            run_dir.path().to_str().expect("run dir utf-8"),
            "--json",
        ]);

    assert!(
        json_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&json_output.stderr)
    );
    let summary: serde_json::Value = serde_json::from_slice(&json_output.stdout)
        .expect("replay json output with control chars should parse");
    assert_eq!(summary["artifact_count"], 1);
    assert_eq!(summary["child_session_count"], 1);
    assert_eq!(
        summary["artifacts"][0]["path"],
        "artifacts/delegated/task-output\n.json"
    );
    assert_eq!(summary["artifacts"][0]["tool_id"], "task");
    assert_eq!(summary["artifacts"][0]["effective_tool_id"], "agent.spawn");
    assert_eq!(summary["artifacts"][0]["canonical_tool_id"], "agent.spawn");
    assert_eq!(summary["artifacts"][0]["alias_source_tool_id"], "task");
    assert_eq!(
        summary["artifacts"][0]["child_session_id"],
        "child-run-001\n\tcontrol"
    );
    assert_eq!(
        summary["child_sessions"][0]["child_session_id"],
        "child-run-001\n\tcontrol"
    );
    assert_eq!(
        summary["child_sessions"][0]["parent_tool_call_id"],
        "toolcall_parent\rcontrol"
    );
    assert_eq!(
        summary["child_sessions"][0]["provider_model"],
        "openai/gpt-5.4-mini"
    );

    let human_output = run_harness([
            "replay",
            "--session",
            run_dir.path().to_str().expect("run dir utf-8"),
        ]);

    assert!(
        human_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&human_output.stderr)
    );
    let stdout = String::from_utf8_lossy(&human_output.stdout);
    assert!(!stdout.contains("artifacts/delegated/task-output\n.json"));
    assert!(!stdout.contains("child-run-001\n\tcontrol"));
    assert!(!stdout.contains("toolcall_parent\rcontrol"));
    assert!(stdout.contains("artifacts/delegated/task-output\\n.json"));
    assert!(stdout.contains("child-run-001\\n\\tcontrol"));
    assert!(stdout.contains("parent_tool=toolcall_parent\\rcontrol"));
    assert!(stdout.contains("tool=task"));
    assert!(stdout.contains("effective=agent.spawn"));
    assert!(stdout.contains("canonical=agent.spawn"));
    assert!(stdout.contains("alias=task"));
}
#[test]
fn replay_cli_surfaces_recovery_context_in_json_summary() {
    let run_dir = tempdir().expect("tempdir");
    write_events_jsonl(
        run_dir.path(),
        &[
            envelope(
                "run_replay_context",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_replay_context",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "worker".to_string(),
                    parent_agent_id: Some("agent_root".to_string()),
                }),
            ),
            envelope(
                "run_replay_context",
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_1".to_string(),
                    provider_id: "openai".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "resume safely".to_string(),
                    request_digest: "digest-replay-context".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                "run_replay_context",
                4,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_1".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("wrote diff".to_string()),
                    output_digest: Some("digest-tool-output".to_string()),
                    output_json: None,
                    metadata: Some(ToolCallMetadata {
                        lineage: Some(TaskLineageMetadata {
                            parent_session_id: Some("run_parent".to_string()),
                            ..TaskLineageMetadata::default()
                        }),
                        artifact_refs: vec![EventArtifactRef {
                            path: "artifacts/patch.diff".to_string(),
                            digest: Some("digest-artifact".to_string()),
                        }],
                        ..ToolCallMetadata::default()
                    }),
                }),
            ),
            envelope(
                "run_replay_context",
                5,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let output = run_harness([
            "replay",
            "--session",
            run_dir.path().to_str().expect("run dir utf-8"),
            "--json",
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("replay json output should parse");
    assert_eq!(summary["mode_source"], "interactive_live");
    assert_eq!(summary["is_resumable"], true);
    assert_eq!(summary["artifact_count"], 1);
    assert_eq!(summary["child_session_count"], 1);
    assert_eq!(summary["parent_session_id"], "run_parent");
    assert_eq!(summary["workspace_root"], "/tmp/workspace");
}
