#[test]
fn lineage_tree_projects_parent_child_relationships() {
    // arrange
    use harness_core::proj::SessionCatalogEntry;

    let parent = SessionCatalogEntry {
        run_id: "run-parent".to_string(),
        run_name: Some("parent".to_string()),
        status: Some(RunStatus::Finished),
        last_updated_at: Some("2026-01-01T00:00:00Z".to_string()),
        workspace_root: Some("/workspace".to_string()),
        profile_preset: None,
        provider_model: None,
        mode_source: SessionModeSource::InteractiveLive,
        is_resumable: true,
        resume_disabled_reason: None,
        artifact_count: 0,
        child_session_count: 1,
        parent_session_id: None,
    };

    let child = SessionCatalogEntry {
        run_id: "run-child-001".to_string(),
        run_name: Some("child".to_string()),
        status: Some(RunStatus::Finished),
        last_updated_at: Some("2026-01-02T00:00:00Z".to_string()),
        workspace_root: Some("/workspace".to_string()),
        profile_preset: None,
        provider_model: None,
        mode_source: SessionModeSource::InteractiveLive,
        is_resumable: true,
        resume_disabled_reason: None,
        artifact_count: 0,
        child_session_count: 0,
        parent_session_id: Some("run-parent".to_string()),
    };

    // act
    let tree = project_lineage_tree(vec![parent.clone(), child.clone()]);

    // assert
    assert_eq!(tree.len(), 2, "tree must contain both entries");
    assert_eq!(tree.roots.len(), 1, "one root (parent)");
    assert_eq!(tree.roots[0].entry.run_id, "run-parent");
    assert_eq!(tree.roots[0].children.len(), 1, "parent has one child");
    assert_eq!(tree.roots[0].children[0].entry.run_id, "run-child-001");

    // Flatten produces correct depth ordering
    let rows = tree.flatten();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].depth, 0);
    assert_eq!(rows[0].entry.run_id, "run-parent");
    assert_eq!(rows[1].depth, 1);
    assert_eq!(rows[1].entry.run_id, "run-child-001");
}

#[test]
fn lineage_tree_handles_orphan_and_cyclic_entries_as_roots() {
    // arrange
    use harness_core::proj::SessionCatalogEntry;

    let orphan = SessionCatalogEntry {
        run_id: "run-orphan".to_string(),
        run_name: None,
        status: None,
        last_updated_at: None,
        workspace_root: None,
        profile_preset: None,
        provider_model: None,
        mode_source: SessionModeSource::Unknown,
        is_resumable: false,
        resume_disabled_reason: None,
        artifact_count: 0,
        child_session_count: 0,
        parent_session_id: Some("nonexistent-parent".to_string()),
    };

    let self_ref = SessionCatalogEntry {
        run_id: "run-selfref".to_string(),
        run_name: None,
        status: None,
        last_updated_at: None,
        workspace_root: None,
        profile_preset: None,
        provider_model: None,
        mode_source: SessionModeSource::Unknown,
        is_resumable: false,
        resume_disabled_reason: None,
        artifact_count: 0,
        child_session_count: 0,
        parent_session_id: Some("run-selfref".to_string()),
    };

    // act
    let tree = project_lineage_tree(vec![orphan, self_ref]);
    // assert
    assert_eq!(tree.roots.len(), 2, "orphans/cycles become roots");
    assert_eq!(tree.len(), 2);
}

// ===========================================================================
// 6. FORK AT STABLE CUTOFF
// ===========================================================================

#[test]
fn fork_validates_stable_prefix_at_completed_cutoff() {
    // arrange
    let events = full_run_events("run-fork-src");

    // act
    // Stable at seq=7 (finished with all in-flight resolved)
    let prefix = validate_fork_stable_prefix(&events, 7).unwrap_or_abort();
    // assert
    assert_eq!(prefix.cutoff_seq, 7);
    assert_eq!(prefix.event_count, 7);
    assert_eq!(prefix.status, Some(RunStatus::Finished));
}

#[test]
fn fork_rejects_unstable_prefix_with_open_tool_call() {
    // arrange
    let events = full_run_events("run-fork-unstable");

    // act
    // Cutoff at seq=4: tool call tc-001 was requested but not finished — unstable
    let result = validate_fork_stable_prefix(&events, 4);
    // assert
    assert!(
        result.is_err(),
        "prefix with in-flight tool call must be unstable"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, SessionLineageError::UnstablePrefix { .. }),
        "expected UnstablePrefix, got: {err}"
    );
}

#[test]
fn fork_rejects_out_of_range_cutoff() {
    // arrange
    let events = full_run_events("run-fork-range");

    // act
    let result = validate_fork_stable_prefix(&events, 999);
    // assert
    assert!(matches!(
        result,
        Err(SessionLineageError::CutoffOutOfRange { .. })
    ));
}

#[test]
fn fork_materializes_child_session_atomically() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path();
    let run_id = "run-fork-parent";
    let run_dir = session_dir.join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = full_run_events(run_id);
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);

    let prefix = validate_fork_stable_prefix(&events, 7).unwrap_or_abort();
    let request = ChildSessionMaterializationRequest {
        source_run_dir: &run_dir,
        events: &events,
        stable_prefix: &prefix,
        source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
    };

    // act
    let result = materialize_child_session(request).unwrap_or_abort();
    // assert
    assert_eq!(result.source_cutoff_seq, 7);
    assert_eq!(result.event_count, 7);
    assert!(result.child_run_dir.is_dir(), "child run dir must exist");
    assert!(
        result.child_run_dir.join("events.jsonl").is_file(),
        "child must have events.jsonl"
    );

    // Child events use child run_id
    let child_events = read_events_from_jsonl(&result.child_run_dir.join("events.jsonl"));
    assert_eq!(child_events.len(), 7);
    for event in &child_events {
        assert_eq!(
            event.run_id.as_str(),
            result.child_run_id,
            "child events must be rewritten to child run_id"
        );
    }

    // Source is untouched
    let source_events = read_events_from_jsonl(&run_dir.join("events.jsonl"));
    assert_eq!(source_events.len(), 7);
    assert_eq!(source_events[0].run_id.as_str(), run_id);
}

#[test]
fn fork_rejects_writer_locked_source() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path();
    let run_id = "run-fork-locked";
    let run_dir = session_dir.join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = full_run_events(run_id);
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);

    // Create a writer lock file to simulate active session
    fs::write(
        run_dir.join(".writer.lock"),
        format!("pid={}\ntoken=999\n", std::process::id()),
    )
    .unwrap_or_abort();

    let prefix = validate_fork_stable_prefix(&events, 7).unwrap_or_abort();
    let request = ChildSessionMaterializationRequest {
        source_run_dir: &run_dir,
        events: &events,
        stable_prefix: &prefix,
        source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
    };

    // act
    let result = materialize_child_session(request);
    // assert
    assert!(result.is_err(), "fork must reject writer-locked source");
}

// ===========================================================================
// 7. CLONE LATEST STABLE PREFIX
// ===========================================================================

#[test]
fn clone_selects_latest_stable_prefix() {
    // arrange
    let events = full_run_events("run-clone-src");

    // act
    let prefix = latest_clone_stable_prefix(&events).unwrap_or_abort();
    // assert
    assert_eq!(prefix.cutoff_seq, 7, "latest stable is at the final event");
    assert_eq!(prefix.status, Some(RunStatus::Finished));
}

#[test]
fn clone_fails_when_no_stable_prefix_exists() {
    // arrange
    // Run with active lifecycle and no finished/failed
    let events = active_run_events("run-clone-nostable");

    // act
    let result = latest_clone_stable_prefix(&events);
    // assert
    assert!(
        result.is_err(),
        "clone must fail when no stable prefix exists"
    );
}

#[test]
fn clone_materializes_child_from_latest_stable() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path();
    let run_id = "run-clone-parent";
    let run_dir = session_dir.join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = full_run_events(run_id);
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);

    let prefix = latest_clone_stable_prefix(&events).unwrap_or_abort();
    let request = ChildSessionMaterializationRequest {
        source_run_dir: &run_dir,
        events: &events,
        stable_prefix: &prefix,
        source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
    };

    // act
    let result = materialize_child_session(request).unwrap_or_abort();
    // assert
    assert_eq!(result.source_cutoff_seq, 7);
    assert_eq!(result.event_count, 7);
    assert!(result.child_run_dir.is_dir());
}

// ===========================================================================
// 8. PROMPT REWIND: PLAN + ATOMIC WORKSPACE RESTORE
// ===========================================================================

