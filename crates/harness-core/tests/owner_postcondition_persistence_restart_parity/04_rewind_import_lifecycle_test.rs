// ===========================================================================
// P2 OWNER: prompt rewind is append-only (plan) and atomic (workspace restore).
// ===========================================================================

#[test]
fn p2_prompt_rewind_plan_is_append_only() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_dir = root.path().join("t36-rewind-plan");
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = full_run_events("t36-rewind-plan");
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);
    let digest_before = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();

    // act
    let plan = plan_prompt_rewind(&events, 4).unwrap_or_abort();
    // assert
    assert_eq!(plan.cutoff_seq, 4);
    assert!(plan.events_append_only);

    let digest_after = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();
    assert_eq!(
        digest_before, digest_after,
        "plan_prompt_rewind must not mutate events.jsonl"
    );
}

#[test]
fn p2_prompt_rewind_atomic_restore_rolls_back_files_and_keeps_events() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let workspace = root.path().join("workspace");
    fs::create_dir_all(workspace.join("src")).unwrap_or_abort();

    fs::write(
        workspace.join("src/main.rs"),
        "fn main() { println!(\"v2-corrupted\"); }",
    )
    .unwrap_or_abort();
    fs::write(workspace.join("README.md"), "# Version 2 (broken)").unwrap_or_abort();

    let events = full_run_events("t36-rewind-atomic");
    let snapshot = vec![
        FileSnapshotEntry {
            path: "src/main.rs".to_string(),
            content: "fn main() { println!(\"v1-good\"); }".to_string(),
        },
        FileSnapshotEntry {
            path: "README.md".to_string(),
            content: "# Version 1 (known good)".to_string(),
        },
    ];

    // act
    let result = atomic_prompt_rewind(&events, 4, &workspace, &snapshot).unwrap_or_abort();
    // assert
    assert_eq!(result.files_restored, 2, "both files must be restored");
    assert!(
        result.events_append_only,
        "atomic rewind must keep events append-only"
    );

    // Files actually rolled back to the snapshot content.
    assert_eq!(
        fs::read_to_string(workspace.join("src/main.rs")).unwrap_or_abort(),
        "fn main() { println!(\"v1-good\"); }"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("README.md")).unwrap_or_abort(),
        "# Version 1 (known good)"
    );
}

// ===========================================================================
// P2 OWNER: foreign-session import creates a replay-only session and never
// mutates the source.
// ===========================================================================

#[test]
fn p2_foreign_import_owner_postcondition_creates_replay_only_session() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let foreign = root.path().join("foreign-source");
    let dest = root.path().join("harness-import");
    fs::create_dir_all(&foreign).unwrap_or_abort();

    let source_events = completed_run_events("foreign-t36");
    write_events_jsonl(&foreign.join("events.jsonl"), &source_events);
    let source_bytes_before = fs::read(foreign.join("events.jsonl")).unwrap_or_abort();

    // act
    let result = import_foreign_session_as_replay(&foreign, &dest).unwrap_or_abort();

    // assert
    assert_eq!(result.event_count, 2);
    assert_eq!(result.mode_source, SessionModeSource::ReplayOnly);
    assert!(result.run_dir.join("events.jsonl").is_file());
    assert!(result.run_dir.join("meta.json").is_file());

    // Owner postcondition: source is never mutated.
    let source_bytes_after = fs::read(foreign.join("events.jsonl")).unwrap_or_abort();
    assert_eq!(
        source_bytes_before, source_bytes_after,
        "foreign source events.jsonl must not be mutated by import"
    );

    // Imported events use the new child run_id.
    let imported = read_events_from_jsonl(&result.run_dir.join("events.jsonl"));
    for event in &imported {
        assert_eq!(event.run_id.as_str(), result.run_id);
    }
}

// ===========================================================================
// P7 ERROR: foreign import refuses an active target session.
// ===========================================================================

#[test]
fn p7_foreign_import_refuses_active_target() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let foreign = root.path().join("foreign-active-target");
    let active = root.path().join("active-target");
    fs::create_dir_all(&foreign).unwrap_or_abort();
    fs::create_dir_all(&active).unwrap_or_abort();
    fs::write(active.join("events.jsonl"), "").unwrap_or_abort();

    // act
    let result = refuse_import_into_active_session(&foreign, &active);
    // assert
    assert!(
        matches!(
            result,
            Err(ForeignSessionError::ImportIntoActiveForbidden { .. })
        ),
        "import into an active session must be forbidden"
    );
}

// ===========================================================================
// P7 LIFECYCLE: teardown releases writer lock; reopened session resumes cleanly.
// ===========================================================================

#[test]
fn p7_teardown_releases_writer_lock_and_clean_reopen_resumes() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-teardown";
    let run_dir = root.path().join(run_id);

    // act
    let store = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
    store
        .append(make_envelope_without_seq(
            run_id,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "teardown-check".into(),
                workspace_root: "/ws".to_string(),
            }),
        ))
        .unwrap_or_abort();
    // assert
    assert!(
        run_dir.join(".writer.lock").is_file(),
        "writer lock must be held while store is open"
    );

    // Teardown = drop. After drop the lock file is gone.
    drop(store);
    assert!(
        !run_dir.join(".writer.lock").exists(),
        "writer lock must be released after drop"
    );

    // Clean reopen succeeds and sees the persisted event.
    let reopened = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
    assert_eq!(
        reopened.next_seq().unwrap_or_abort(),
        2,
        "clean reopen must resume after teardown"
    );
}

// ===========================================================================
// P7 LIFECYCLE: replay across two separate store handles yields identical
// events, proving there is no hidden shared mutable state.
// ===========================================================================

#[tokio::test]
async fn p7_replay_across_independent_handles_is_identical() {
    // arrange
    use tokio_stream::StreamExt;
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-independent-handles";
    let events = full_run_events(run_id);
    {
        let store = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
        for event in &events {
            store
                .append(EventEnvelopeWithoutSeqV1::from(event.clone()))
                .unwrap_or_abort();
        }
    }

    let mut collected = Vec::new();
    for _ in 0..2 {
        let store = JsonlFileEventStore::open_existing(root.path(), run_id, true).unwrap_or_abort();
        let mut stream = store.replay(1).unwrap_or_abort();
        let mut local = Vec::new();
        while let Some(result) = stream.next().await {
            local.push(result.unwrap_or_abort());
        }
        drop(store);
        collected.push(local);
    }

    // act
    // Mutation: introduce shared mutable state between store handles and
    // these two vectors will diverge.
    // assert
    assert_eq!(
        collected[0], collected[1],
        "independent replay handles must produce identical events"
    );
    assert_eq!(
        collected[0], events,
        "replayed events must match stored events"
    );
}

// ===========================================================================
// P7 LIFECYCLE: concurrent-looking back-to-back writes produce contiguous seq.
// ===========================================================================

#[test]
fn p7_rapid_appends_produce_contiguous_monotonic_seq() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-rapid-append";
    let store = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();

    let mut seqs = Vec::new();
    for i in 0..10 {
        let event = store
            .append(make_envelope_without_seq(
                run_id,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: format!("rapid-{i}").into(),
                    workspace_root: "/ws".to_string(),
                }),
            ))
            .unwrap_or_abort();
        seqs.push(event.seq);
    }

    // act
    // Mutation: any gap or duplication in seq would break this assertion.
    let expected: Vec<u64> = (1..=10).collect();
    // assert
    assert_eq!(seqs, expected, "appends must produce contiguous seq from 1");
    assert_eq!(store.next_seq().unwrap_or_abort(), 11);
}
