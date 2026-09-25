#[tokio::test]
async fn conversation_rewind_preserves_journal_and_files_and_survives_resume() {
    let sessions = tempfile::tempdir().unwrap_or_abort();
    let workspace = tempfile::tempdir().unwrap_or_abort();
    let provider = CapturingProvider::new(vec!["kept answer", "discarded answer", "replacement answer"]);
    let target = drift_target("rewind", "low", "low", "auto", json!({}));
    let live = coordinator_for_target(sessions.path(), Arc::new(provider.clone()), target.clone());
    let run = live.start_run("rewind", workspace.path().to_path_buf()).await.unwrap_or_abort();
    let agent = live.spawn_agent_idle(supervisor_actor(), "default", None).await.unwrap_or_abort();
    let store = live.event_store().await.unwrap_or_abort();
    let mut events = store.subscribe(1).unwrap_or_abort();
    let mut ids = Vec::new();
    for prompt in ["kept prompt", "discarded prompt"] {
        let id = live.request_agent_turn(supervisor_actor(), agent.clone(), prompt).await.unwrap_or_abort();
        await_turn_terminal(&mut events, &id).await;
        ids.push(id);
    }
    std::fs::write(workspace.path().join("edited.txt"), "keep workspace changes").unwrap_or_abort();
    let saved_file = |content: &str| json!({"digest": blake3::hash(content.as_bytes()).to_hex().chars().take(12).collect::<String>(), "content": content});
    let checkpoints = run.artifacts_dir.join("rewind");
    std::fs::create_dir_all(&checkpoints).unwrap_or_abort();
    std::fs::write(workspace.path().join("created.txt"), "new").unwrap_or_abort();
    for (id, before, after, created_before, created_after) in [
        (&ids[0], "original", "first edit", serde_json::Value::Null, serde_json::Value::Null),
        (&ids[1], "first edit", "keep workspace changes", serde_json::Value::Null, saved_file("new")),
    ] {
        let checkpoint = json!({"request_id": id, "before": {"edited.txt": saved_file(before), "created.txt": created_before}, "after": {"edited.txt": saved_file(after), "created.txt": created_after}});
        std::fs::write(checkpoints.join(format!("{id}.json")), serde_json::to_vec(&checkpoint).unwrap_or_abort()).unwrap_or_abort();
    }
    let before = std::fs::read(&run.events_path).unwrap_or_abort();
    assert_eq!(live.rewind_points().await.unwrap_or_abort().unwrap_or_abort().len(), 2);
    let point = live.rewind_conversation(ids[1].clone()).await.unwrap_or_abort();
    assert_eq!(point.text, "discarded prompt");
    assert!(std::fs::read(&run.events_path).unwrap_or_abort().starts_with(&before));
    assert_eq!(std::fs::read_to_string(workspace.path().join("edited.txt")).unwrap_or_abort(), "keep workspace changes");
    assert!(live.rewind_conversation(ids[1].clone()).await.is_err(), "discarded points cannot be selected again");
    let projected = harness_core::transcript_projection::project_transcript(&load_events(&run.events_path)).unwrap_or_abort();
    let projected_text = format!("{projected:?}");
    assert!(projected_text.contains("kept prompt"));
    assert!(!projected_text.contains("discarded prompt"));
    assert!(!projected_text.contains("discarded answer"));

    let replacement = live.request_agent_turn(supervisor_actor(), agent.clone(), "replacement prompt").await.unwrap_or_abort();
    await_turn_terminal(&mut events, &replacement).await;
    let request = provider.requests().last().cloned().unwrap_or_abort();
    let text = serde_json::to_string(&request.messages).unwrap_or_abort();
    assert!(text.contains("kept answer"));
    assert!(!text.contains("discarded"));

    let restarted_sessions = tempfile::tempdir().unwrap_or_abort();
    clone_persisted_run(&run.run_dir, &restarted_sessions.path().join(run.run_id.as_str()), run.run_id.as_str());
    let copied = restarted_sessions.path().join(run.run_id.as_str()).join("artifacts/rewind");
    std::fs::create_dir_all(&copied).unwrap_or_abort();
    for entry in std::fs::read_dir(&checkpoints).unwrap_or_abort() {
        let entry = entry.unwrap_or_abort();
        std::fs::copy(entry.path(), copied.join(entry.file_name())).unwrap_or_abort();
    }
    live.stop_run().await.unwrap_or_abort();
    let resumed_provider = CapturingProvider::new(vec!["resumed answer", "fresh answer"]);
    let resumed = coordinator_for_target(restarted_sessions.path(), Arc::new(resumed_provider.clone()), target);
    resumed.resume_run(run.run_id.to_string(), "interactive").await.unwrap_or_else(|error| panic!("rewind resume: {error}"));
    assert_eq!(resumed.rewind_points().await.unwrap_or_abort().unwrap_or_abort().iter().map(|point| point.text.as_str()).collect::<Vec<_>>(), ["kept prompt", "replacement prompt"]);
    let resumed_store = resumed.event_store().await.unwrap_or_abort();
    let mut resumed_events = resumed_store.subscribe(1).unwrap_or_abort();
    let id = resumed.request_agent_turn(supervisor_actor(), agent.clone(), "continue").await.unwrap_or_abort();
    await_turn_terminal(&mut resumed_events, &id).await;
    let text = serde_json::to_string(&resumed_provider.requests()[0].messages).unwrap_or_abort();
    assert!(text.contains("replacement answer"));
    assert!(!text.contains("discarded"));
    // Saved tracker reloads after restart and folds discarded file edits into the kept prompt.
    std::fs::write(workspace.path().join("edited.txt"), "external edit").unwrap_or_abort();
    std::fs::write(workspace.path().join("untouched.txt"), "user file").unwrap_or_abort();
    let restored = resumed.revert_workspace(ids[0].clone()).await.unwrap_or_abort();
    assert!(restored.failed_paths.is_empty(), "{restored:?}");
    assert_eq!(restored.conflicts, [("edited.txt".into(), "modified externally".into())]);
    assert_eq!(std::fs::read_to_string(workspace.path().join("edited.txt")).unwrap_or_abort(), "original");
    assert!(!workspace.path().join("created.txt").exists());
    assert_eq!(std::fs::read_to_string(workspace.path().join("untouched.txt")).unwrap_or_abort(), "user file");
    assert!(resumed.revert_workspace(ids[0].clone()).await.is_err(), "restored checkpoints are truncated");
    resumed.rewind_conversation(ids[0].clone()).await.unwrap_or_abort();
    assert!(resumed.rewind_points().await.unwrap_or_abort().unwrap_or_abort().is_empty());
    let id = resumed.request_agent_turn(supervisor_actor(), agent, "fresh prompt").await.unwrap_or_abort();
    await_turn_terminal(&mut resumed_events, &id).await;
    let text = serde_json::to_string(&resumed_provider.requests()[1].messages).unwrap_or_abort();
    assert!(!text.contains("kept answer") && !text.contains("replacement answer"));
    resumed.stop_run().await.unwrap_or_abort();
}
