// ===========================================================================
// P2 OWNER: prompt queue persists across restart; send-now selects FIFO head.
// ===========================================================================

#[test]
fn p2_prompt_queue_persists_across_restart_and_drains_post_turn() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path().join("session");
    fs::create_dir_all(&session_dir).unwrap_or_abort();
    fs::write(session_dir.join("events.jsonl"), b"{\"seq\":1}\n").unwrap_or_abort();

    let events_before = event_log_digest(&session_dir.join("events.jsonl")).unwrap_or_abort();

    // act
    let queue = DurablePromptQueue::for_session(&session_dir);
    queue.enqueue("q-1", "after turn", 1).unwrap_or_abort();
    queue.enqueue("q-2", "after next turn", 2).unwrap_or_abort();
    let interjection = queue
        .interject_mid_turn("urgent", "send now", 3, true)
        .unwrap_or_abort();
    // assert
    assert!(interjection.turn_was_running);
    assert!(!interjection.mutates_conversation_events);
    drop(queue);

    // Restart: a fresh queue handle must see the persisted entries.
    let resumed = DurablePromptQueue::for_session(&session_dir);
    let reconciled = resumed.drain_interjections().unwrap_or_abort();
    assert_eq!(
        reconciled.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        ["urgent"],
        "interjection must reconcile on restart"
    );

    let send_now = resumed.dequeue().unwrap_or_abort().unwrap();
    assert_eq!(send_now.id, "q-1", "send-now must select FIFO head");

    let drained = resumed.drain().unwrap_or_abort();
    assert_eq!(
        drained.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        ["q-2"],
        "post-turn drain must clear remaining entries"
    );

    let after = DurablePromptQueue::for_session(&session_dir);
    assert!(
        after.is_empty().unwrap_or_abort(),
        "queue must be empty after drain + restart"
    );

    // Owner postcondition: queue ops never touch events.jsonl.
    // Mutation: route queue persistence through the event store and this
    // digest drifts, violating the append-only owner contract.
    let events_after = event_log_digest(&session_dir.join("events.jsonl")).unwrap_or_abort();
    assert_eq!(
        events_before, events_after,
        "queue ops must not mutate events.jsonl"
    );
}

// ===========================================================================
// P2 OWNER: durable memory persists across restart with deterministic search.
// ===========================================================================

#[test]
fn p2_durable_memory_persists_across_restart_with_deterministic_search() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let workspace = root.path().join("workspace");
    fs::create_dir_all(&workspace).unwrap_or_abort();

    let store = DurableMemoryStore::for_workspace(&workspace);
    store
        .put("alpha-rule", "always prefer leftmost derivation")
        .unwrap_or_abort();
    store
        .put("beta-rule", "secondary fallback path")
        .unwrap_or_abort();
    drop(store);

    // act
    // Restart: fresh handle sees the persisted entries.
    let resumed = DurableMemoryStore::for_workspace(&workspace);
    let search_alpha = resumed.search("alpha").unwrap_or_abort();
    let search_beta = resumed.search("beta").unwrap_or_abort();
    let search_both = resumed.search("rule").unwrap_or_abort();

    // assert
    assert!(
        search_alpha.iter().any(|e| e.key == "alpha-rule"),
        "alpha entry must survive restart"
    );
    assert!(
        search_beta.iter().any(|e| e.key == "beta-rule"),
        "beta entry must survive restart"
    );
    assert_eq!(
        search_both.len(),
        2,
        "deterministic search ordering must return both matches"
    );

    // Release-scope is the owner postcondition for clearing entries.
    resumed
        .release_scope(MemoryScope::Workspace)
        .unwrap_or_abort();
    let resumed_after_flush = DurableMemoryStore::for_workspace(&workspace);
    let post_flush = resumed_after_flush.search("rule").unwrap_or_abort();
    assert!(
        post_flush.is_empty(),
        "release_scope(Workspace) must clear persisted memory entries"
    );
}

// ===========================================================================
// P7 ERROR: corrupt events.jsonl is detected by replay and never silently
// repaired. The owner surfaces a parse error rather than mutating the log.
// ===========================================================================

#[tokio::test]
async fn p7_corrupt_events_jsonl_is_detected_and_never_silently_repaired() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-corrupt";
    let run_dir = root.path().join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    // Write a line that is not a valid envelope but is newline-terminated.
    fs::write(run_dir.join("events.jsonl"), b"not-an-envelope\n").unwrap_or_abort();
    let digest_before = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();

    // act
    // The store surfaces corruption at open_existing time as a typed
    // InvalidJsonLine error — it never silently accepts garbage lines.
    let open_result = JsonlFileEventStore::open_existing(root.path(), run_id, true);
    // assert
    assert!(
        open_result.is_err(),
        "open_existing must reject a corrupt events.jsonl line"
    );
    let err = open_result.unwrap_err();
    assert!(
        matches!(err, EventStoreError::InvalidJsonLine { .. }),
        "expected InvalidJsonLine, got: {err}"
    );

    // Mutation: if a future "auto-repair" path rewrites events.jsonl, this
    // digest comparison catches it.
    let digest_after = event_log_digest(&run_dir.join("events.jsonl")).unwrap_or_abort();
    assert_eq!(
        digest_before, digest_after,
        "corrupt events.jsonl must not be silently rewritten"
    );
}

// ===========================================================================
// P7 ERROR: writer lock contention produces a typed AcquireWriterLock error,
// never a silent overwrite of the active writer.
// ===========================================================================

#[test]
fn p7_writer_lock_contention_produces_typed_error_without_overwrite() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-lock-contention";

    let first = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
    first
        .append(make_envelope_without_seq(
            run_id,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "first-writer".into(),
                workspace_root: "/ws".to_string(),
            }),
        ))
        .unwrap_or_abort();

    // act
    // A second open while the first holds the lock must fail with a typed
    // error and must NOT overwrite the first writer's event.
    let result = JsonlFileEventStore::open(root.path(), run_id, true);
    // assert
    assert!(
        result.is_err(),
        "second writer must be rejected while first holds lock"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, EventStoreError::AcquireWriterLock { .. }),
        "expected AcquireWriterLock, got: {err}"
    );

    // The first writer's event survives the rejected second open.
    let events = read_events_from_jsonl(&root.path().join(run_id).join("events.jsonl"));
    assert_eq!(
        events.len(),
        1,
        "first writer's event must survive contention"
    );
    assert_eq!(events[0].seq, 1);
}

// ===========================================================================
// P7 CRASH RECOVERY: stale writer lock from a dead PID is recoverable on
// reopen; the lock is reclaimed and the existing events are preserved.
// ===========================================================================

#[test]
fn p7_crash_recovery_reclaims_stale_lock_and_preserves_events() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_id = "t36-crash-recover";
    let run_dir = root.path().join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = completed_run_events(run_id);
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);

    // Simulate a crashed previous process: dead PID holds the lock.
    fs::write(run_dir.join(".writer.lock"), "pid=999999999\ntoken=1\n").unwrap_or_abort();

    // act
    let report = inspect_previous_crash(&run_dir);
    // assert
    assert!(report.previous_crash_detected);
    assert_eq!(
        report.recovery_action,
        Some(CrashRecoveryAction::OpenRecovers),
        "stale writer lock must map to open-recovers action"
    );

    // Open reclaims the stale lock and preserves existing events.
    let store = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
    assert_eq!(
        store.next_seq().unwrap_or_abort(),
        3,
        "recovered store must resume seq counter after existing events"
    );

    let lock_content = fs::read_to_string(run_dir.join(".writer.lock")).unwrap_or_abort();
    assert!(
        lock_content.contains(&format!("pid={}", std::process::id())),
        "recovered lock must be reclaimed by the live process"
    );

    // The events on disk are the original ones — recovery never appends.
    // Mutation: if recovery appended a synthetic marker event, this count
    // would drift to 3.
    let events_after = read_events_from_jsonl(&run_dir.join("events.jsonl"));
    assert_eq!(
        events_after.len(),
        events.len(),
        "crash recovery must not synthesize events"
    );
}

