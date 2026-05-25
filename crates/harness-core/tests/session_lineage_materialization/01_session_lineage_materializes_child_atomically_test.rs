#[test]
fn session_lineage_materializes_child_atomically() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let session_dir = temp_dir.path();
    let source_run_id = "run_parent_materialize";
    let source_run_dir = session_dir.join(source_run_id);
    fs::create_dir_all(&source_run_dir).expect("create source run dir");

    let artifact_path = "artifacts/toolcalls/toolcall_000001/output.txt";
    let artifact_body = b"child materialization artifact\n".as_slice();
    write_source_artifact(&source_run_dir, artifact_path, artifact_body);
    let artifact_digest = blake3::hash(artifact_body).to_hex().to_string();
    fs::write(
        source_run_dir.join("meta.json"),
        serde_json::json!({
            "run_id": source_run_id,
            "run_name": "Parent run",
            "workspace_root": "/workspace/source",
            "config_digest": "cfg-parent",
            "harness_version": "test-version"
        })
        .to_string(),
    )
    .expect("write source meta");

    let events = stable_events(
        source_run_id,
        artifact_path,
        &artifact_digest,
        artifact_body.len(),
    );
    let prefix = validate_fork_stable_prefix(&events, events.len() as u64).expect("stable prefix");

    let lock_path = source_run_dir.join(".writer.lock");
    fs::write(&lock_path, b"locked").expect("write source writer lock");
    let locked_err = materialize_child_session(ChildSessionMaterializationRequest {
        source_run_dir: &source_run_dir,
        events: &events,
        stable_prefix: &prefix,
        source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
    })
    .expect_err("disk source must reject active writer lock");
    assert!(matches!(
        locked_err,
        ChildSessionMaterializationError::SourceWriterLocked { .. }
    ));

    let result = materialize_child_session(ChildSessionMaterializationRequest {
        source_run_dir: &source_run_dir,
        events: &events,
        stable_prefix: &prefix,
        source_kind: ChildSessionMaterializationSourceKind::TuiStableInMemorySnapshot,
    })
    .expect("TUI stable in-memory snapshot may materialize while source writer is locked");

    assert_ne!(result.child_run_id, source_run_id);
    assert_eq!(result.source_run_id.as_deref(), Some(source_run_id));
    assert_eq!(result.source_cutoff_seq, events.len() as u64);
    assert_eq!(result.event_count, events.len());
    assert_eq!(result.artifact_count, 1);
    assert!(result.child_run_dir.is_dir());
    assert_eq!(
        fs::read(result.child_run_dir.join(artifact_path)).expect("read child artifact"),
        artifact_body
    );

    let child_events = read_events(&result.child_run_dir);
    assert_eq!(child_events.len(), events.len());
    for (index, (source, child)) in events.iter().zip(&child_events).enumerate() {
        assert_eq!(child.seq, index as u64 + 1);
        assert_eq!(child.run_id, result.child_run_id);
        assert_ne!(child.event_id, source.event_id);
        assert!(child.event_id.contains(&result.child_run_id));
        assert_eq!(child.correlation_id, None);
        assert_eq!(child.causation_id, None);
        assert_eq!(
            child.stream_key.as_deref(),
            Some(format!("run:{}", result.child_run_id).as_str())
        );
    }

    let meta: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(result.child_run_dir.join("meta.json")).expect("read child meta"),
    )
    .expect("parse child meta");
    let created_at = meta["created_at"]
        .as_str()
        .expect("created_at should be populated");
    assert!(
        created_at.starts_with("unix_ms:"),
        "created_at should use deterministic harness materialization timestamp shape"
    );
    assert_eq!(meta["run_id"], result.child_run_id);
    assert_eq!(
        meta["run_name"],
        format!("Harness child of {source_run_id}")
    );
    assert_eq!(meta["workspace_root"], "/workspace/source");
    assert_eq!(meta["config_digest"], "cfg-parent");
    assert_eq!(meta["harness_version"], "test-version");
    assert_eq!(
        meta["harness_lineage"]["relationship"],
        "child_session_materialization"
    );
    assert_eq!(
        meta["harness_lineage"]["harness_operation"],
        "child_session_materialization"
    );
    assert_eq!(
        meta["harness_lineage"]["harness_source_run_id"],
        source_run_id
    );
    assert_eq!(
        meta["harness_lineage"]["harness_source_cutoff_seq"],
        events.len() as u64
    );
    assert_eq!(
        meta["harness_lineage"]["harness_source_cutoff_event_id"],
        events.last().expect("cutoff event").event_id
    );
    assert_eq!(
        meta["harness_lineage"]["harness_source_digest"],
        source_prefix_digest(&events)
    );
    assert_eq!(meta["harness_lineage"]["harness_created_at"], created_at);
    assert_eq!(meta["harness_lineage"]["parent_run_id"], source_run_id);
    assert_eq!(
        meta["harness_lineage"]["source_cutoff_seq"],
        events.len() as u64
    );
    assert_eq!(
        meta["harness_lineage"]["source_cutoff_event_id"],
        events.last().expect("cutoff event").event_id
    );
    assert_eq!(
        meta["harness_lineage"]["source_digest"],
        source_prefix_digest(&events)
    );
    assert!(meta["harness_lineage"]["event_rewrite_policy"]
        .as_str()
        .expect("event policy")
        .contains("clears correlation_id and causation_id"));

    assert_no_unpublished_temp_dirs(session_dir);
}
#[test]
fn session_lineage_tui_live_snapshot_terminalizes_open_state_for_resume() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let session_dir = temp_dir.path();
    let source_run_id = "run_parent_live_snapshot";
    let source_run_dir = session_dir.join(source_run_id);
    fs::create_dir_all(&source_run_dir).expect("create source run dir");
    fs::write(
        source_run_dir.join("meta.json"),
        serde_json::json!({
            "run_id": source_run_id,
            "run_name": "Parent live run",
            "workspace_root": "/workspace/source",
            "config_digest": "cfg-parent",
            "harness_version": "test-version"
        })
        .to_string(),
    )
    .expect("write source meta");

    let events = live_snapshot_with_open_state(source_run_id);
    let prefix = validate_tui_fork_stable_prefix(&events, events.len() as u64)
        .expect("TUI live snapshot prefix accepts reference-style snapshots");

    let result = materialize_child_session(ChildSessionMaterializationRequest {
        source_run_dir: &source_run_dir,
        events: &events,
        stable_prefix: &prefix,
        source_kind: ChildSessionMaterializationSourceKind::TuiStableInMemorySnapshot,
    })
    .expect("TUI live snapshot materializes");

    let child_events = read_events(&result.child_run_dir);
    assert!(child_events.iter().any(|event| matches!(
        &event.payload,
        EventV1::TaskCancelled(payload)
            if payload.task_id == "task_000001"
                && payload.reason.contains("terminalized copied live task state")
    )));
    assert!(child_events.iter().any(|event| matches!(
        &event.payload,
        EventV1::PermissionResolved(payload)
            if payload.permission_id == "perm_000001"
                && payload.decision == PermissionDecision::Deny
    )));
    assert!(matches!(
        child_events.last().map(|event| &event.payload),
        Some(EventV1::RunFinished(_))
    ));

    let resume_plan = inspect_resume_plan(&result.child_run_dir);
    assert!(
        resume_plan.is_resumable,
        "child fork should resume after live-state terminalization: {:?}",
        resume_plan.resume_disabled_reason
    );
    assert!(resume_plan.tasks_in_flight.is_empty());
    assert!(resume_plan.pending_permissions.is_empty());
}
#[test]
fn session_lineage_missing_artifact_rolls_back() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let session_dir = temp_dir.path();
    let source_run_id = "run_parent_missing_artifact";
    let source_run_dir = session_dir.join(source_run_id);
    fs::create_dir_all(&source_run_dir).expect("create source run dir");

    let events = stable_events(
        source_run_id,
        "artifacts/toolcalls/toolcall_000001/missing.txt",
        blake3::hash(b"missing").to_hex().as_ref(),
        7,
    );
    write_source_events(&source_run_dir, &events);
    let prefix = validate_fork_stable_prefix(&events, events.len() as u64).expect("stable prefix");

    let err = materialize_child_session(ChildSessionMaterializationRequest {
        source_run_dir: &source_run_dir,
        events: &events,
        stable_prefix: &prefix,
        source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
    })
    .expect_err("missing artifact rejects before publish");
    assert!(matches!(
        err,
        ChildSessionMaterializationError::MissingArtifact { .. }
    ));

    let run_dirs = fs::read_dir(session_dir)
        .expect("read session dir")
        .map(|entry| entry.expect("dir entry").file_name())
        .collect::<Vec<_>>();
    assert_eq!(run_dirs, vec![source_run_id]);
    assert_no_unpublished_temp_dirs(session_dir);
}
#[test]
fn session_lineage_concurrent_fork_clone_from_same_source_create_unique_children() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let session_dir = temp_dir.path();
    let source_run_id = "run_parent_concurrent";
    let source_run_dir = session_dir.join(source_run_id);
    fs::create_dir_all(&source_run_dir).expect("create source run dir");

    let artifact_path = "artifacts/toolcalls/toolcall_000001/output.txt";
    let artifact_body = b"shared concurrent materialization artifact\n".as_slice();
    write_source_artifact(&source_run_dir, artifact_path, artifact_body);
    let artifact_digest = blake3::hash(artifact_body).to_hex().to_string();
    let events = stable_events(
        source_run_id,
        artifact_path,
        &artifact_digest,
        artifact_body.len(),
    );
    write_source_events(&source_run_dir, &events);
    let prefix = validate_fork_stable_prefix(&events, events.len() as u64).expect("stable prefix");

    let source_run_dir = Arc::new(source_run_dir);
    let events = Arc::new(events);
    let prefix = Arc::new(prefix);
    let handles = (0..6)
        .map(|_| {
            let source_run_dir = Arc::clone(&source_run_dir);
            let events = Arc::clone(&events);
            let prefix = Arc::clone(&prefix);
            thread::spawn(move || {
                materialize_child_session(ChildSessionMaterializationRequest {
                    source_run_dir: source_run_dir.as_ref(),
                    events: events.as_slice(),
                    stable_prefix: prefix.as_ref(),
                    source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
                })
                .expect("concurrent child materialization succeeds")
            })
        })
        .collect::<Vec<_>>();

    let results = handles
        .into_iter()
        .map(|handle| handle.join().expect("join materialization thread"))
        .collect::<Vec<_>>();
    let child_ids = results
        .iter()
        .map(|result| result.child_run_id.clone())
        .collect::<BTreeSet<_>>();

    assert_eq!(child_ids.len(), results.len());
    for result in &results {
        assert_ne!(result.child_run_id, source_run_id);
        assert!(result.child_run_dir.join("events.jsonl").exists());
        assert!(result.child_run_dir.join("meta.json").exists());
        assert_eq!(
            fs::read(result.child_run_dir.join(artifact_path)).expect("read child artifact"),
            artifact_body
        );
    }
    assert_no_unpublished_temp_dirs(session_dir);
}
#[test]
fn session_lineage_missing_meta_uses_harness_fallback_metadata() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let session_dir = temp_dir.path();
    let source_run_id = "run_parent_missing_meta";
    let source_run_dir = session_dir.join(source_run_id);
    fs::create_dir_all(&source_run_dir).expect("create source run dir");

    let artifact_path = "artifacts/toolcalls/toolcall_000001/output.txt";
    let artifact_body = b"metadata fallback artifact\n".as_slice();
    write_source_artifact(&source_run_dir, artifact_path, artifact_body);
    let artifact_digest = blake3::hash(artifact_body).to_hex().to_string();
    let events = stable_events(
        source_run_id,
        artifact_path,
        &artifact_digest,
        artifact_body.len(),
    );
    write_source_events(&source_run_dir, &events);
    let prefix = validate_fork_stable_prefix(&events, events.len() as u64).expect("stable prefix");

    let result = materialize_child_session(ChildSessionMaterializationRequest {
        source_run_dir: &source_run_dir,
        events: &events,
        stable_prefix: &prefix,
        source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
    })
    .expect("missing source metadata falls back to event-derived fields");

    let meta: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(result.child_run_dir.join("meta.json")).expect("read child meta"),
    )
    .expect("parse child meta");
    assert_eq!(
        meta["run_name"],
        format!("Harness child of {source_run_id}")
    );
    assert_eq!(meta["workspace_root"], "/workspace/source");
    assert_eq!(meta["config_digest"], "harness-lineage-materialized");
    assert_eq!(
        meta["harness_lineage"]["harness_source_run_id"],
        source_run_id
    );
    assert_no_unpublished_temp_dirs(session_dir);
}
#[test]
fn session_lineage_invalid_artifact_path_rolls_back() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let session_dir = temp_dir.path();
    let source_run_id = "run_parent_bad_artifact_path";
    let source_run_dir = session_dir.join(source_run_id);
    fs::create_dir_all(&source_run_dir).expect("create source run dir");

    let events = stable_events(
        source_run_id,
        "artifacts/../meta.json",
        blake3::hash(b"bad").to_hex().as_ref(),
        3,
    );
    write_source_events(&source_run_dir, &events);
    let prefix = validate_fork_stable_prefix(&events, events.len() as u64).expect("stable prefix");

    let err = materialize_child_session(ChildSessionMaterializationRequest {
        source_run_dir: &source_run_dir,
        events: &events,
        stable_prefix: &prefix,
        source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
    })
    .expect_err("path traversal artifact reference rejects before publish");

    assert!(matches!(
        err,
        ChildSessionMaterializationError::InvalidArtifactPath { .. }
    ));
    assert_eq!(session_dir_entries(session_dir), vec![source_run_id]);
    assert_no_unpublished_temp_dirs(session_dir);
}
#[cfg(unix)]
#[test]
fn session_lineage_rejects_artifact_symlink_without_copying_target() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let session_dir = temp_dir.path();
    let source_run_id = "run_parent_symlink_artifact";
    let source_run_dir = session_dir.join(source_run_id);
    fs::create_dir_all(&source_run_dir).expect("create source run dir");

    let artifact_path = "artifacts/toolcalls/toolcall_000001/output.txt";
    let artifact_body = b"outside symlink target\n".as_slice();
    let outside_dir = tempfile::tempdir().expect("outside tempdir");
    let outside_target = outside_dir.path().join("outside-target.txt");
    fs::write(&outside_target, artifact_body).expect("write outside target");
    let link_path = source_run_dir.join(artifact_path);
    fs::create_dir_all(link_path.parent().expect("artifact parent"))
        .expect("create artifact parent");
    std::os::unix::fs::symlink(&outside_target, &link_path).expect("create artifact symlink");

    let artifact_digest = blake3::hash(artifact_body).to_hex().to_string();
    let events = stable_events(
        source_run_id,
        artifact_path,
        &artifact_digest,
        artifact_body.len(),
    );
    write_source_events(&source_run_dir, &events);
    let prefix = validate_fork_stable_prefix(&events, events.len() as u64).expect("stable prefix");

    let err = materialize_child_session(ChildSessionMaterializationRequest {
        source_run_dir: &source_run_dir,
        events: &events,
        stable_prefix: &prefix,
        source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
    })
    .expect_err("symlinked artifact must not be copied");

    assert!(matches!(
        err,
        ChildSessionMaterializationError::ArtifactSymlink { .. }
    ));
    assert_eq!(session_dir_entries(session_dir), vec![source_run_id]);
    assert_no_unpublished_temp_dirs(session_dir);
}
#[test]
fn session_lineage_source_event_log_mismatch_rolls_back_before_publish() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let session_dir = temp_dir.path();
    let source_run_id = "run_parent_modified_source";
    let source_run_dir = session_dir.join(source_run_id);
    fs::create_dir_all(&source_run_dir).expect("create source run dir");

    let events = stable_events(
        source_run_id,
        "artifacts/toolcalls/toolcall_000001/output.txt",
        blake3::hash(b"unchanged").to_hex().as_ref(),
        9,
    );
    let mut changed_events = events.clone();
    changed_events.push(envelope(
        source_run_id,
        6,
        EventV1::RunStarted(RunStartedEvent {
            run_name: "modified after load".to_string(),
            workspace_root: "/workspace/source".to_string(),
        }),
    ));
    write_source_events(&source_run_dir, &changed_events);
    let prefix = validate_fork_stable_prefix(&events, events.len() as u64).expect("stable prefix");

    let err = materialize_child_session(ChildSessionMaterializationRequest {
        source_run_dir: &source_run_dir,
        events: &events,
        stable_prefix: &prefix,
        source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
    })
    .expect_err("changed source event log must reject before temp publish");

    assert!(matches!(
        err,
        ChildSessionMaterializationError::SourceEventLogChanged { .. }
    ));
    assert_eq!(session_dir_entries(session_dir), vec![source_run_id]);
    assert_no_unpublished_temp_dirs(session_dir);
}
