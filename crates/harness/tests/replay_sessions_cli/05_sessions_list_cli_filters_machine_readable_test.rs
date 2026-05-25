#[test]
fn sessions_list_cli_filters_machine_readable_selectors() {
    let session_dir = tempdir().expect("tempdir");
    let resumable_dir = session_dir.path().join("run_resumable");
    let prompt_dir = session_dir.path().join("run_prompt");
    let failed_dir = session_dir.path().join("run_failed");
    std::fs::create_dir_all(&resumable_dir).expect("create resumable run dir");
    std::fs::create_dir_all(&prompt_dir).expect("create prompt run dir");
    std::fs::create_dir_all(&failed_dir).expect("create failed run dir");

    write_events_jsonl(
        &resumable_dir,
        &[
            envelope(
                "run_resumable",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_resumable",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_1".to_string(),
                    profile: "worker".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope_with_actor(
                "run_resumable",
                3,
                EventActor::new(ActorKind::Worker, Some("agent_1".to_string())),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_1".to_string(),
                    provider_id: "openai".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "hello".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                "run_resumable",
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    write_events_jsonl(
        &prompt_dir,
        &[
            envelope(
                "run_prompt_filtered",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "prompt".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_prompt_filtered",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    std::fs::write(
        prompt_dir.join("meta.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&serde_json::json!({
                "run_id": "run_prompt_filtered",
                "run_name": "prompt",
                "workspace_root": "/tmp/workspace",
                "profile_preset": "worker",
                "mode_source": "prompt",
                "created_at": "1710000000000"
            }))
            .expect("serialize prompt catalog metadata")
        ),
    )
    .expect("write prompt catalog metadata");

    write_events_jsonl(
        &failed_dir,
        &[
            envelope(
                "run_failed_filtered",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_failed_filtered",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_2".to_string(),
                    profile: "reviewer".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                "run_failed_filtered",
                3,
                EventV1::RunFailed(RunFailedEvent {
                    error: "boom".to_string(),
                }),
            ),
        ],
    );

    let output = run_harness([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "list",
            "--json",
            "--status",
            "finished",
            "--profile",
            "worker",
            "--resumable",
            "false",
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let rows: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("filtered sessions json should parse");
    assert_eq!(rows.as_array().map(Vec::len), Some(1));
    let row = &rows[0];
    assert_eq!(
        row["run_dir"],
        prompt_dir.to_str().expect("prompt dir utf-8")
    );
    assert_eq!(row["run_id"], "run_prompt_filtered");
    assert_eq!(row["run_name"], "prompt");
    assert_eq!(row["status"], "finished");
    assert_eq!(row["workspace_root"], "/tmp/workspace");
    assert_eq!(row["profile_preset"], "worker");
    assert_eq!(row["provider_model"], serde_json::Value::Null);
    assert_eq!(row["mode_source"], "prompt");
    assert_eq!(row["is_resumable"], false);
    assert_eq!(
        row["resume_disabled_reason"],
        "prompt runs are not resumable"
    );
    assert!(row["last_updated_at"].is_string());
}
#[test]
fn sessions_inspect_cli_accepts_positional_session_selector() {
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("directory_name_differs");
    std::fs::create_dir_all(&run_dir).expect("create run dir");

    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_inspect_positional",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "inspectable".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_inspect_positional",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_1".to_string(),
                    profile: "worker".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                "run_inspect_positional",
                3,
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
            "directory_name_differs",
            "--json",
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let inspected: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("sessions inspect json should parse");
    assert_eq!(inspected["catalog"]["run_id"], "run_inspect_positional");
    assert_eq!(inspected["replay"]["run_name"], "inspectable");
    assert_eq!(
        inspected["run_dir"],
        run_dir.to_str().expect("run dir utf-8")
    );
}
#[test]
fn sessions_replay_cli_resolves_run_id_from_session_catalog() {
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("directory_name_differs");
    std::fs::create_dir_all(&run_dir).expect("create run dir");

    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_resolved",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "resolved".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_resolved",
                2,
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
            "replay",
            "run_resolved",
            "--json",
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let summary: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("replay json output should parse");
    assert_eq!(summary["run_id"], "run_resolved");
    assert_eq!(summary["run_name"], "resolved");
}
#[test]
fn sessions_export_cli_writes_json_bundle() {
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("run_export");
    std::fs::create_dir_all(&run_dir).expect("create run dir");

    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_export",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "exportable".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_export",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let export_path = session_dir.path().join("session-export.json");
    let output = run_harness([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "export",
            "run_export",
            "--output",
            export_path.to_str().expect("export path utf-8"),
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bundle: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&export_path).expect("read exported session bundle"))
            .expect("export bundle should parse");
    assert_eq!(bundle["catalog"]["run_id"], "run_export");
    assert_eq!(bundle["replay"]["run_name"], "exportable");
    assert_eq!(bundle["events"].as_array().map(Vec::len), Some(2));
}
#[test]
fn sessions_list_cli_supports_run_id_sorting() {
    let session_dir = tempdir().expect("tempdir");
    for run_id in ["run_b", "run_c", "run_a"] {
        let run_dir = session_dir.path().join(run_id);
        std::fs::create_dir_all(&run_dir).expect("create run dir");
        write_events_jsonl(
            &run_dir,
            &[
                envelope(
                    run_id,
                    1,
                    EventV1::RunStarted(RunStartedEvent {
                        run_name: format!("{run_id}-name"),
                        workspace_root: "/tmp/workspace".to_string(),
                    }),
                ),
                envelope(
                    run_id,
                    2,
                    EventV1::RunFinished(RunFinishedEvent {
                        summary: "done".to_string(),
                    }),
                ),
            ],
        );
    }

    let output = run_harness([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "list",
            "--json",
            "--sort",
            "run_id_asc",
        ]);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let rows: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("sorted sessions json should parse");
    let run_ids = rows
        .as_array()
        .expect("sessions json array")
        .iter()
        .map(|row| row["run_id"].as_str().expect("run_id string"))
        .collect::<Vec<_>>();
    assert_eq!(run_ids, vec!["run_a", "run_b", "run_c"]);
}
#[test]
fn sessions_tree_prints_lineage_depths() {
    let session_dir = tempdir().expect("tempdir");
    let root_dir = session_dir.path().join("root_session_dir");
    let child_dir = session_dir.path().join("child_session_dir");
    std::fs::create_dir_all(&root_dir).expect("create root run dir");
    std::fs::create_dir_all(&child_dir).expect("create child run dir");

    write_events_jsonl(&root_dir, &resumable_finished_events("run_tree_root"));
    write_events_jsonl(&child_dir, &resumable_finished_events("run_tree_child"));
    write_harness_lineage_meta(&child_dir, "run_tree_child", "run_tree_root");

    let json_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "tree",
            "--json",
        ]);

    assert!(
        json_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&json_output.stderr)
    );
    let tree: serde_json::Value =
        serde_json::from_slice(&json_output.stdout).expect("tree json should parse");
    assert_eq!(tree["session_count"], 2);
    assert_eq!(tree["harness_lineage"][0]["run_id"], "run_tree_root");
    assert_eq!(tree["harness_lineage"][0]["depth"], 0);
    assert_eq!(tree["harness_lineage"][1]["run_id"], "run_tree_child");
    assert_eq!(tree["harness_lineage"][1]["depth"], 1);
    assert_eq!(
        tree["harness_lineage"][1]["parent_session_id"],
        "run_tree_root"
    );

    let rooted_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "tree",
            "--root",
            root_dir.to_str().expect("root dir utf-8"),
            "--filter",
            "child",
        ]);

    assert!(
        rooted_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&rooted_output.stderr)
    );
    let stdout = String::from_utf8_lossy(&rooted_output.stdout);
    assert!(stdout.contains("Harness session lineage"));
    assert!(stdout.contains("root: run_tree_root"));
    assert!(stdout.contains("filter: child"));
    assert!(stdout.contains("run_tree_child"));
    assert!(!stdout.contains("run_tree_root status="));
}
