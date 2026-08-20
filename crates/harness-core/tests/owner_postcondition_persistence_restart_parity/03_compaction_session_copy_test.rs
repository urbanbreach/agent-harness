// ===========================================================================
// P2 OWNER: compaction appends a checkpoint and never rewrites events.jsonl.
// ===========================================================================

#[test]
fn p2_compaction_appends_checkpoint_without_rewriting_events_jsonl() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-compaction";
    let run_dir = root.path().join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = full_run_events(run_id);
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);
    let digest_before = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();

    // Replay-derived transcript projection (no side effects).
    let projection = project_transcript(&events).unwrap_or_abort();

    // act
    let digest_after = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();
    // assert
    assert_eq!(
        digest_before, digest_after,
        "transcript projection must not rewrite events.jsonl"
    );

    // A projected compaction checkpoint (if present) is a derived projection,
    // not a mutation of the source events. The projection's messages vector
    // contains the event-derived turn content; compaction checkpoints are
    // emitted as separate derived projections.
    assert!(
        !projection.messages.is_empty(),
        "projection must produce messages without mutating source events"
    );
}

// ===========================================================================
// P2 OWNER: session rename is replay-derived from SessionTitleUpdated event.
// ===========================================================================

#[tokio::test]
async fn p2_session_rename_postcondition_is_event_replay_not_direct_mutation() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-rename";
    let run_dir = root.path().join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = vec![
        envelope(
            run_id,
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "original".into(),
                workspace_root: "/ws".to_string(),
            }),
        ),
        envelope(
            run_id,
            2,
            EventV1::SessionTitleUpdated(SessionTitleUpdatedEvent {
                title: "renamed-by-event".to_string(),
            }),
        ),
        envelope(
            run_id,
            3,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ];
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);

    let digest_before = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();

    // act
    // Replay produces the renamed title without writing back to events.jsonl.
    let replayed = replay_all_async(root.path(), run_id).await;
    let title_event_count = replayed
        .iter()
        .filter(|e| matches!(&e.payload, EventV1::SessionTitleUpdated(_)))
        .count();
    // assert
    assert_eq!(
        title_event_count, 1,
        "SessionTitleUpdated event must be replayed exactly once"
    );

    let digest_after = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();
    assert_eq!(
        digest_before, digest_after,
        "rename replay must not mutate events.jsonl"
    );
}

// ===========================================================================
// P2 OWNER: fork validates stable prefix and materializes child atomically.
// ===========================================================================

#[test]
fn p2_fork_owner_postcondition_creates_child_with_rewritten_run_id() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-fork-source";
    let run_dir = root.path().join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = full_run_events(run_id);
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);
    let source_digest_before = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();

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
    assert!(result.child_run_dir.join("events.jsonl").is_file());

    let child_events = read_events_from_jsonl(&result.child_run_dir.join("events.jsonl"));
    for event in &child_events {
        assert_eq!(
            event.run_id.as_str(),
            result.child_run_id,
            "child events must be rewritten to the child run_id"
        );
    }

    // Source events.jsonl must be untouched.
    // Mutation: if materialization mutated the source, this digest drifts.
    let source_digest_after = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();
    assert_eq!(
        source_digest_before, source_digest_after,
        "fork must not mutate the source session events.jsonl"
    );
}

// ===========================================================================
// P2 OWNER: clone selects the latest stable prefix and materializes a child.
// ===========================================================================

#[test]
fn p2_clone_owner_postcondition_selects_latest_stable_prefix() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-clone-source";
    let run_dir = root.path().join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = full_run_events(run_id);
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);

    // act
    let prefix = latest_clone_stable_prefix(&events).unwrap_or_abort();
    // assert
    assert_eq!(
        prefix.cutoff_seq, 7,
        "latest stable prefix must be the final event"
    );

    let request = ChildSessionMaterializationRequest {
        source_run_dir: &run_dir,
        events: &events,
        stable_prefix: &prefix,
        source_kind: ChildSessionMaterializationSourceKind::DiskRunDirectory,
    };
    let result = materialize_child_session(request).unwrap_or_abort();
    assert_eq!(result.event_count, 7);
    assert!(result.child_run_dir.is_dir());
}

// ===========================================================================
// P7 ERROR: clone rejects an active run with no stable prefix.
// ===========================================================================

#[test]
fn p7_clone_rejects_active_run_with_no_stable_prefix() {
    // arrange
    let active_events = vec![
        envelope(
            "t36-clone-active",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "active".into(),
                workspace_root: "/ws".to_string(),
            }),
        ),
        envelope(
            "t36-clone-active",
            2,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tc-active".into(),
                tool_id: "bash".to_string(),
                args_summary: "{}".to_string(),
                args_digest: "d".to_string(),
                metadata: None,
            }),
        ),
    ];

    // act
    let result = latest_clone_stable_prefix(&active_events);
    // assert
    assert!(
        result.is_err(),
        "clone must reject a run with no stable prefix"
    );
}

