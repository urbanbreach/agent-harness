#[test]
fn replay_cli_merges_on_disk_artifact_discovery_with_recovery_metadata() {
    let run_dir = tempdir().expect("tempdir");
    std::fs::create_dir_all(run_dir.path().join("artifacts/notes")).expect("create artifacts dir");
    std::fs::write(
        run_dir.path().join("artifacts/notes/output.txt"),
        "artifact body\n",
    )
    .expect("write artifact");

    write_events_jsonl(
        run_dir.path(),
        &[
            envelope(
                "run_recovery_detail",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_recovery_detail",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "worker".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                }),
            ),
            agent_envelope(
                "run_recovery_detail",
                3,
                "agent_child",
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_1".to_string(),
                    provider_id: "openai".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "delegate".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                "run_recovery_detail",
                4,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_1".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("wrote artifact".to_string()),
                    output_digest: Some("tool-digest".to_string()),
                    output_json: None,
                    metadata: Some(ToolCallMetadata {
                        lineage: Some(TaskLineageMetadata {
                            parent_tool_call_id: Some("toolcall_parent".to_string()),
                            parent_task_id: Some("task_1".to_string()),
                            parent_request_id: Some("req_0".to_string()),
                            parent_session_id: Some("agent_parent".to_string()),
                            child_session_id: Some("agent_child".to_string()),
                            child_request_id: Some("req_1".to_string()),
                            child_provider_id: Some("openai".to_string()),
                            child_model_id: Some("gpt-5.4-mini".to_string()),
                        }),
                        timing: Some(ExecutionTimingMetadata {
                            started_mono_ms: Some(3),
                            finished_mono_ms: Some(7),
                            elapsed_ms: Some(4),
                        }),
                        ..ToolCallMetadata::default()
                    }),
                }),
            ),
            envelope(
                "run_recovery_detail",
                5,
                EventV1::ArtifactWritten(ArtifactWrittenEvent {
                    path: "artifacts/notes/output.txt".to_string(),
                    digest: "artifact-digest".to_string(),
                    bytes: 14,
                    tool_call_id: Some("toolcall_1".to_string()),
                    tool_metadata: Default::default(),
                    metadata: Default::default(),
                }),
            ),
            envelope(
                "run_recovery_detail",
                6,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
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
    let summary: serde_json::Value =
        serde_json::from_slice(&json_output.stdout).expect("replay json output should parse");
    assert_eq!(summary["artifact_count"], 1);
    assert_eq!(summary["child_session_count"], 1);
    assert_eq!(
        summary["artifacts"][0]["path"],
        "artifacts/notes/output.txt"
    );
    assert_eq!(summary["artifacts"][0]["tool_call_id"], "toolcall_1");
    assert_eq!(summary["artifacts"][0]["child_session_id"], "agent_child");
    assert_eq!(summary["artifacts"][0]["present_on_disk"], true);
    assert_eq!(
        summary["child_sessions"][0]["child_session_id"],
        "agent_child"
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
    assert!(stdout.contains("session_path:"));
    assert!(stdout.contains("artifacts: 1"));
    assert!(stdout.contains("artifacts/notes/output.txt"));
    assert!(stdout.contains("present=yes"));
    assert!(stdout.contains("child_sessions: 1"));
    assert!(stdout.contains("agent_child"));
    assert!(stdout.contains("openai/gpt-5.4-mini"));
}
#[test]
fn sessions_list_cli_prints_finished_and_failed_runs() {
    let session_dir = tempdir().expect("tempdir");
    let finished_dir = session_dir.path().join("run_a");
    let failed_dir = session_dir.path().join("run_b");
    std::fs::create_dir_all(&finished_dir).expect("create finished run dir");
    std::fs::create_dir_all(&failed_dir).expect("create failed run dir");

    write_events_jsonl(
        &finished_dir,
        &[
            envelope(
                "run_finished",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_finished",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    write_events_jsonl(
        &failed_dir,
        &[
            envelope(
                "run_failed",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_failed",
                2,
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "perm-1".to_string(),
                    kind: "shell".to_string(),
                    tool_call_id: None,
                    summary: "needs shell".to_string(),
                    request_digest: "digest-1".to_string(),
                    timeout_ms: 30_000,
                    default_decision: PermissionDecision::Deny,
                }),
            ),
            envelope(
                "run_failed",
                3,
                EventV1::RunFailed(RunFailedEvent {
                    error: "nope".to_string(),
                }),
            ),
        ],
    );

    let output = run_harness([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "list",
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("run_id"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("run_name"));
    assert!(stdout.contains("profile"));
    assert!(stdout.contains("provider/model"));
    assert!(stdout.contains("artifacts"));
    assert!(stdout.contains("children"));
    assert!(stdout.contains("session_path"));
    assert!(stdout.contains("run_finished"));
    assert!(stdout.contains("finished"));
    assert!(stdout.contains("interactive"));
    assert!(stdout.contains("run_failed"));
    assert!(stdout.contains("failed"));
}
#[test]
fn sessions_list_cli_surfaces_recovery_counts_run_path_and_parent() {
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("run_recovery");
    std::fs::create_dir_all(&run_dir).expect("create recovery run dir");
    write_events_jsonl(&run_dir, &delegated_recovery_events("run_recovery_catalog"));

    let output = run_harness([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "list",
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let row = stdout
        .lines()
        .find(|line| line.contains("run_recovery_catalog"))
        .expect("recovery row");
    let columns = row.split_whitespace().collect::<Vec<_>>();
    assert_eq!(columns[0], "run_recovery_catalog");
    assert_eq!(columns[7], "1");
    assert_eq!(columns[8], "1");
    assert_eq!(columns[9], run_dir.to_str().expect("run dir utf-8"));
    assert_eq!(columns[10], "agent_supervisor");
}
#[test]
fn sessions_inspect_cli_surfaces_recovery_details() {
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("run_recovery_inspect");
    std::fs::create_dir_all(run_dir.join("artifacts/notes")).expect("create run artifacts");
    std::fs::write(
        run_dir.join("artifacts/notes/output.txt"),
        "artifact body\n",
    )
    .expect("write artifact");

    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_recovery_inspect",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_recovery_inspect",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_root".to_string(),
                    profile: "deep".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                "run_recovery_inspect",
                3,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "worker".to_string(),
                    parent_agent_id: Some("agent_root".to_string()),
                }),
            ),
            agent_envelope(
                "run_recovery_inspect",
                4,
                "agent_child",
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_1".to_string(),
                    provider_id: "openai".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "delegate".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                "run_recovery_inspect",
                5,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_1".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("wrote artifact".to_string()),
                    output_digest: Some("tool-digest".to_string()),
                    output_json: None,
                    metadata: Some(ToolCallMetadata {
                        lineage: Some(TaskLineageMetadata {
                            parent_tool_call_id: Some("toolcall_parent".to_string()),
                            parent_task_id: Some("task_1".to_string()),
                            parent_request_id: Some("req_0".to_string()),
                            parent_session_id: Some("agent_root".to_string()),
                            child_session_id: Some("agent_child".to_string()),
                            child_request_id: Some("req_1".to_string()),
                            child_provider_id: Some("openai".to_string()),
                            child_model_id: Some("gpt-5.4-mini".to_string()),
                        }),
                        timing: Some(ExecutionTimingMetadata {
                            started_mono_ms: Some(4),
                            finished_mono_ms: Some(5),
                            elapsed_ms: Some(1),
                        }),
                        ..ToolCallMetadata::default()
                    }),
                }),
            ),
            envelope(
                "run_recovery_inspect",
                6,
                EventV1::ArtifactWritten(ArtifactWrittenEvent {
                    path: "artifacts/notes/output.txt".to_string(),
                    digest: "artifact-digest".to_string(),
                    bytes: 14,
                    tool_call_id: Some("toolcall_1".to_string()),
                    tool_metadata: Default::default(),
                    metadata: Default::default(),
                }),
            ),
            envelope(
                "run_recovery_inspect",
                7,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let output = run_harness([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "inspect",
            "--run",
            "run_recovery_inspect",
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("mode: interactive_live"));
    assert!(stdout.contains("resume: yes"));
    assert!(stdout.contains("session_path:"));
    assert!(stdout.contains("artifacts: 1"));
    assert!(stdout.contains("artifacts/notes/output.txt"));
    assert!(stdout.contains("present=yes"));
    assert!(stdout.contains("child_sessions: 1"));
    assert!(stdout.contains("agent_child"));

    let json_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "inspect",
            "--run",
            "run_recovery_inspect",
            "--json",
        ]);

    assert!(
        json_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&json_output.stderr)
    );
    let inspected: serde_json::Value =
        serde_json::from_slice(&json_output.stdout).expect("sessions inspect json should parse");
    assert_eq!(inspected["catalog"]["run_id"], "run_recovery_inspect");
    assert_eq!(inspected["replay"]["artifact_count"], 1);
    assert_eq!(inspected["replay"]["child_session_count"], 1);
    assert_eq!(
        inspected["replay"]["session_path"],
        run_dir.to_str().expect("run dir utf-8")
    );
    assert_eq!(
        inspected["replay"]["artifacts"][0]["path"],
        "artifacts/notes/output.txt"
    );
    assert_eq!(inspected["replay"]["artifacts"][0]["present_on_disk"], true);
    assert_eq!(
        inspected["replay"]["child_sessions"][0]["child_session_id"],
        "agent_child"
    );
}
