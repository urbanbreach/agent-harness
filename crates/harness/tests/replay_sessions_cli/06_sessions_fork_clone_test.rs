use harness::UnwrapOrAbort;
#[test]
fn sessions_fork_and_clone_create_child_sessions() {
    let session_dir = tempdir().unwrap_or_abort();
    let source_dir = session_dir.path().join("source_session");
    std::fs::create_dir_all(&source_dir).unwrap_or_abort();
    write_events_jsonl(
        &source_dir,
        &resumable_finished_events("run_fork_clone_source"),
    );

    let fork_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "fork",
            "--source",
            "run_fork_clone_source",
            "--cutoff",
            "5",
            "--json",
        ]);

    assert!(
        fork_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&fork_output.stderr)
    );
    let forked: serde_json::Value =
        serde_json::from_slice(&fork_output.stdout).unwrap_or_abort();
    assert_eq!(forked["harness_operation"], "fork");
    assert_eq!(forked["source_run_id"], "run_fork_clone_source");
    assert_eq!(forked["source_cutoff_seq"], 5);
    assert_eq!(forked["event_count"], 5);
    assert_eq!(forked["warnings"], serde_json::json!([]));
    assert_eq!(forked["errors"], serde_json::json!([]));
    let fork_child_dir =
        std::path::PathBuf::from(forked["child_run_dir"].as_str().unwrap_or_abort());
    assert!(fork_child_dir.join("events.jsonl").exists());

    let clone_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "clone",
            "--source",
            source_dir.to_str().unwrap_or_abort(),
            "--json",
        ]);

    assert!(
        clone_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&clone_output.stderr)
    );
    let cloned: serde_json::Value =
        serde_json::from_slice(&clone_output.stdout).unwrap_or_abort();
    assert_eq!(cloned["harness_operation"], "clone");
    assert_eq!(cloned["source_run_id"], "run_fork_clone_source");
    assert_eq!(cloned["source_cutoff_seq"], 5);
    assert_eq!(cloned["warnings"], serde_json::json!([]));
    assert_eq!(cloned["errors"], serde_json::json!([]));
    assert_ne!(forked["child_run_id"], cloned["child_run_id"]);

    let human_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "clone",
            "--source",
            "run_fork_clone_source",
        ]);
    assert!(
        human_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&human_output.stderr)
    );
    let stdout = String::from_utf8_lossy(&human_output.stdout);
    assert!(stdout.contains("Harness session clone created"));
    assert!(stdout.contains("child_run_id:"));
    assert!(stdout.contains("child_run_dir:"));
}
#[test]
fn sessions_fork_clone_child_replays() {
    let session_dir = tempdir().unwrap_or_abort();
    let source_dir = session_dir.path().join("replay_source");
    std::fs::create_dir_all(&source_dir).unwrap_or_abort();
    write_events_jsonl(
        &source_dir,
        &resumable_finished_events("run_child_replay_source"),
    );

    let fork_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "fork",
            "--source",
            "run_child_replay_source",
            "--cutoff",
            "5",
            "--json",
        ]);
    assert!(
        fork_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&fork_output.stderr)
    );
    let forked: serde_json::Value =
        serde_json::from_slice(&fork_output.stdout).unwrap_or_abort();
    let child_run_id = forked["child_run_id"].as_str().unwrap_or_abort();

    let inspect_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "inspect",
            child_run_id,
            "--json",
        ]);
    assert!(
        inspect_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&inspect_output.stderr)
    );
    let inspected: serde_json::Value =
        serde_json::from_slice(&inspect_output.stdout).unwrap_or_abort();
    assert_eq!(inspected["catalog"]["run_id"], child_run_id);
    assert_eq!(inspected["replay"]["is_resumable"], true);

    let replay_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "replay",
            child_run_id,
            "--json",
        ]);
    assert!(
        replay_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&replay_output.stderr)
    );
    let replay: serde_json::Value =
        serde_json::from_slice(&replay_output.stdout).unwrap_or_abort();
    assert_eq!(replay["run_id"], child_run_id);
    assert_eq!(replay["total_events"], 5);
    assert_eq!(replay["is_resumable"], true);

    let export_path = session_dir.path().join("child-export.json");
    let export_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "export",
            child_run_id,
            "--output",
            export_path.to_str().unwrap_or_abort(),
        ]);
    assert!(
        export_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&export_output.stderr)
    );
    let exported: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&export_path).unwrap_or_abort())
            .unwrap_or_abort();
    assert_eq!(exported["catalog"]["run_id"], child_run_id);
    assert_eq!(exported["events"].as_array().map(Vec::len), Some(5));

    let tree_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "tree",
            "--root",
            "run_child_replay_source",
            "--json",
        ]);
    assert!(
        tree_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&tree_output.stderr)
    );
    let tree: serde_json::Value =
        serde_json::from_slice(&tree_output.stdout).unwrap_or_abort();
    assert_eq!(
        tree["harness_lineage"][0]["run_id"],
        "run_child_replay_source"
    );
    assert_eq!(tree["harness_lineage"][1]["run_id"], child_run_id);
    assert_eq!(tree["harness_lineage"][1]["depth"], 1);
}
#[test]
fn sessions_fork_clone_reject_active_or_writer_locked_source() {
    let session_dir = tempdir().unwrap_or_abort();
    let active_dir = session_dir.path().join("active_source");
    std::fs::create_dir_all(&active_dir).unwrap_or_abort();
    write_events_jsonl(
        &active_dir,
        &[envelope(
            "run_active_lineage_source",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        )],
    );

    let clone_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "clone",
            "--source",
            "run_active_lineage_source",
            "--json",
        ]);
    assert!(!clone_output.status.success());
    let clone_stderr = String::from_utf8_lossy(&clone_output.stderr);
    assert!(clone_stderr.contains("Harness session clone failed"));
    assert!(clone_stderr.contains("no stable completed prefix"));

    let fork_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "fork",
            "--source",
            "run_active_lineage_source",
            "--cutoff",
            "1",
            "--json",
        ]);
    assert!(!fork_output.status.success());
    let fork_stderr = String::from_utf8_lossy(&fork_output.stderr);
    assert!(fork_stderr.contains("Harness session fork failed"));
    assert!(fork_stderr.contains("run is still active"));

    let locked_dir = session_dir.path().join("locked_source");
    std::fs::create_dir_all(&locked_dir).unwrap_or_abort();
    write_events_jsonl(
        &locked_dir,
        &resumable_finished_events("run_locked_lineage_source"),
    );
    std::fs::write(locked_dir.join(".writer.lock"), "locked").unwrap_or_abort();

    let locked_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "fork",
            "--source",
            "run_locked_lineage_source",
            "--cutoff",
            "5",
            "--json",
        ]);
    assert!(!locked_output.status.success());
    let locked_stderr = String::from_utf8_lossy(&locked_output.stderr);
    assert!(locked_stderr.contains("Harness session fork failed"));
    assert!(locked_stderr.contains("actively writer-locked"));

    let entries = std::fs::read_dir(session_dir.path())
        .unwrap_or_abort()
        .map(|entry| entry.unwrap_or_abort().file_name())
        .collect::<Vec<_>>();
    assert_eq!(
        entries.len(),
        2,
        "no child run should be published on rejection"
    );
}
#[test]
fn sessions_child_replay_and_continue_readiness_survive_parent_movement() {
    let session_dir = tempdir().unwrap_or_abort();
    let source_dir = session_dir.path().join("movable_source");
    std::fs::create_dir_all(&source_dir).unwrap_or_abort();
    write_events_jsonl(
        &source_dir,
        &resumable_finished_events("run_movable_parent"),
    );

    let fork_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "fork",
            "--source",
            "run_movable_parent",
            "--cutoff",
            "5",
            "--json",
        ]);
    assert!(
        fork_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&fork_output.stderr)
    );
    let forked: serde_json::Value =
        serde_json::from_slice(&fork_output.stdout).unwrap_or_abort();
    let child_run_id = forked["child_run_id"].as_str().unwrap_or_abort();

    let moved_parent_dir = tempdir().unwrap_or_abort();
    let moved_parent = moved_parent_dir.path().join("moved_parent");
    std::fs::rename(&source_dir, &moved_parent).unwrap_or_abort();

    let replay_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "replay",
            child_run_id,
            "--json",
        ]);
    assert!(
        replay_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&replay_output.stderr)
    );
    let replay: serde_json::Value =
        serde_json::from_slice(&replay_output.stdout).unwrap_or_abort();
    assert_eq!(replay["run_id"], child_run_id);
    assert_eq!(replay["is_resumable"], true);

    let reopen_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "reopen",
            "--session",
            child_run_id,
            "--json",
        ]);
    assert!(
        reopen_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&reopen_output.stderr)
    );
    let recovery: serde_json::Value =
        serde_json::from_slice(&reopen_output.stdout).unwrap_or_abort();
    assert_eq!(recovery["run_id"], child_run_id);
    assert_eq!(recovery["resumable"], true);
    assert!(recovery["continue_hint"]
        .as_str()
        .unwrap_or_abort()
        .contains(child_run_id));

    std::fs::remove_dir_all(&moved_parent).unwrap_or_abort();
    let tree_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "tree",
            "--json",
        ]);
    assert!(
        tree_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&tree_output.stderr)
    );
    let tree: serde_json::Value =
        serde_json::from_slice(&tree_output.stdout).unwrap_or_abort();
    assert_eq!(tree["session_count"], 1);
    assert_eq!(tree["harness_lineage"][0]["run_id"], child_run_id);
    assert_eq!(tree["harness_lineage"][0]["depth"], 0);
}
#[test]
fn sessions_tree_renders_deep_lineage_deterministically() {
    let session_dir = tempdir().unwrap_or_abort();
    let chain = [
        ("run_deep_root", None),
        ("run_deep_child", Some("run_deep_root")),
        ("run_deep_grandchild", Some("run_deep_child")),
        ("run_deep_great_grandchild", Some("run_deep_grandchild")),
        ("run_deep_leaf", Some("run_deep_great_grandchild")),
    ];
    for (run_id, parent) in chain {
        let run_dir = session_dir.path().join(run_id);
        std::fs::create_dir_all(&run_dir).unwrap_or_abort();
        write_events_jsonl(&run_dir, &resumable_finished_events(run_id));
        if let Some(parent) = parent {
            write_harness_lineage_meta(&run_dir, run_id, parent);
        }
    }

    let json_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
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
        serde_json::from_slice(&json_output.stdout).unwrap_or_abort();
    let rows = tree["harness_lineage"].as_array().unwrap_or_abort();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0]["run_id"], "run_deep_root");
    assert_eq!(rows[1]["run_id"], "run_deep_child");
    assert_eq!(rows[2]["run_id"], "run_deep_grandchild");
    assert_eq!(rows[3]["run_id"], "run_deep_great_grandchild");
    assert_eq!(rows[4]["run_id"], "run_deep_leaf");
    assert_eq!(
        rows.iter()
            .map(|row| row["depth"].as_u64().unwrap_or_abort())
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4]
    );

    let human_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "tree",
        ]);
    assert!(
        human_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&human_output.stderr)
    );
    let stdout = String::from_utf8_lossy(&human_output.stdout);
    assert!(stdout.contains("Harness session lineage"));
    assert!(stdout.contains("        - run_deep_leaf status=finished"));
}
