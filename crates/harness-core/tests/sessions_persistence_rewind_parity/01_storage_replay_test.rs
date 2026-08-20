#[test]
fn session_creation_produces_run_dir_with_events_and_writer_lock() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path();

    let store = JsonlFileEventStore::open(session_dir, "run-create-001", true).unwrap_or_abort();

    // act
    // Run directory exists with events.jsonl and writer lock
    let run_dir = session_dir.join("run-create-001");
    // assert
    assert!(run_dir.is_dir(), "run directory must exist");
    assert!(
        run_dir.join("events.jsonl").is_file(),
        "events.jsonl must be created"
    );
    assert!(
        run_dir.join(".writer.lock").is_file(),
        "writer lock must exist during open"
    );

    // Append an event
    let event = store
        .append(make_envelope_without_seq(
            "run-create-001",
            EventV1::RunStarted(RunStartedEvent {
                run_name: "created".into(),
                workspace_root: "/ws".to_string(),
            }),
        ))
        .unwrap_or_abort();
    assert_eq!(event.seq, 1, "first event must have seq=1");

    // Drop store releases writer lock
    drop(store);
    assert!(
        !run_dir.join(".writer.lock").exists(),
        "writer lock must be released on drop"
    );

    // Events file has one valid line
    let events = read_events_from_jsonl(&run_dir.join("events.jsonl"));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].seq, 1);
    assert_eq!(events[0].run_id.as_str(), "run-create-001");
}

#[test]
fn append_only_sequencing_is_monotonic_and_contiguous() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let store = JsonlFileEventStore::open(root.path(), "run-seq-001", true).unwrap_or_abort();

    // act
    for i in 0..5 {
        let event = store
            .append(make_envelope_without_seq(
                "run-seq-001",
                EventV1::RunStarted(RunStartedEvent {
                    run_name: format!("event-{i}").into(),
                    workspace_root: "/ws".to_string(),
                }),
            ))
            .unwrap_or_abort();
        // assert
        assert_eq!(event.seq, i + 1, "events must be contiguous from 1");
    }

    assert_eq!(store.next_seq().unwrap_or_abort(), 6);
}

// ===========================================================================
// 2. WRITER LOCK ENFORCEMENT
// ===========================================================================

#[test]
fn second_writer_lock_acquisition_is_rejected_while_first_held() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path();

    // First open acquires the lock
    let _store1 = JsonlFileEventStore::open(session_dir, "run-locked-001", true).unwrap_or_abort();

    // act
    // Second open on same run must fail (lock held by same process but different file handle)
    let result = JsonlFileEventStore::open(session_dir, "run-locked-001", true);
    // The second open detects an existing lock with OUR pid — since the process is alive,
    // it cannot reclaim it; the result depends on implementation. The lock file content
    // contains our PID which is alive, so it should fail with AcquireWriterLock.
    // assert
    assert!(
        result.is_err(),
        "second writer must be rejected while first holds lock"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err, EventStoreError::AcquireWriterLock { .. }),
        "expected AcquireWriterLock error, got: {err}"
    );
}

// ===========================================================================
// 3. REPLAY PROJECTIONS: TWO PASSES ARE IDENTICAL + NO SIDE EFFECTS
// ===========================================================================

#[tokio::test]
async fn replay_two_passes_produce_identical_projections() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path();

    // Write events to disk (simulating a completed session)
    let run_id = "run-replay-001";
    let events = full_run_events(run_id);

    // Create store, append events, drop
    {
        let store = JsonlFileEventStore::open(session_dir, run_id, true).unwrap_or_abort();
        for event in &events {
            store
                .append(EventEnvelopeWithoutSeqV1::from(event.clone()))
                .unwrap_or_abort();
        }
    }

    // Replay pass 1
    let store1 = JsonlFileEventStore::open_existing(session_dir, run_id, true).unwrap_or_abort();
    let replay1: Vec<EventEnvelopeV1> = {
        use tokio_stream::StreamExt;
        let mut stream = store1.replay(1).unwrap_or_abort();
        let mut events = Vec::new();
        while let Some(result) = stream.next().await {
            events.push(result.unwrap_or_abort());
        }
        events
    };
    drop(store1);

    // Replay pass 2
    let store2 = JsonlFileEventStore::open_existing(session_dir, run_id, true).unwrap_or_abort();
    let replay2: Vec<EventEnvelopeV1> = {
        use tokio_stream::StreamExt;
        let mut stream = store2.replay(1).unwrap_or_abort();
        let mut events = Vec::new();
        while let Some(result) = stream.next().await {
            events.push(result.unwrap_or_abort());
        }
        events
    };
    drop(store2);

    // act
    // Prove identical projections
    // assert
    assert_eq!(replay1.len(), replay2.len(), "event counts must match");
    assert_eq!(replay1, replay2, "replay passes must be byte-identical");

    // Prove conversation projection is identical
    let conv1 = project_conversation(&replay1, &[]).unwrap_or_abort();
    let conv2 = project_conversation(&replay2, &[]).unwrap_or_abort();
    assert_eq!(conv1, conv2, "conversation projections must be identical");

    // Prove no side effects: events.jsonl unchanged between replays
    let events_path = session_dir.join(run_id).join("events.jsonl");
    let digest_before =
        harness_core::prompt_rewind::event_log_digest(&events_path).unwrap_or_abort();

    // Third replay (no mutation)
    let store3 = JsonlFileEventStore::open_existing(session_dir, run_id, true).unwrap_or_abort();
    {
        use tokio_stream::StreamExt;
        let mut stream = store3.replay(1).unwrap_or_abort();
        while let Some(result) = stream.next().await {
            result.unwrap_or_abort();
        }
    }
    drop(store3);

    let digest_after =
        harness_core::prompt_rewind::event_log_digest(&events_path).unwrap_or_abort();
    assert_eq!(
        digest_before, digest_after,
        "replay must not modify events.jsonl"
    );
}

// ===========================================================================
// 4. SESSION RENAME VIA EVENT REPLAY
// ===========================================================================

#[tokio::test]
async fn session_rename_is_replayed_from_title_event() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path();
    let run_id = "run-rename-001";

    // Create session with title rename event
    {
        let store = JsonlFileEventStore::open(session_dir, run_id, true).unwrap_or_abort();
        store
            .append(make_envelope_without_seq(
                run_id,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "original-name".into(),
                    workspace_root: "/ws".to_string(),
                }),
            ))
            .unwrap_or_abort();
        store
            .append(make_envelope_without_seq(
                run_id,
                EventV1::SessionTitleUpdated(SessionTitleUpdatedEvent {
                    title: "renamed-session".to_string(),
                }),
            ))
            .unwrap_or_abort();
        store
            .append(make_envelope_without_seq(
                run_id,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ))
            .unwrap_or_abort();
    }

    // Replay and verify title is projected
    let store = JsonlFileEventStore::open_existing(session_dir, run_id, true).unwrap_or_abort();
    let events: Vec<EventEnvelopeV1> = {
        use tokio_stream::StreamExt;
        let mut stream = store.replay(1).unwrap_or_abort();
        let mut events = Vec::new();
        while let Some(result) = stream.next().await {
            events.push(result.unwrap_or_abort());
        }
        events
    };

    // act
    // Find the title event and verify
    let title_event = events.iter().find(|event| {
        matches!(&event.payload, EventV1::SessionTitleUpdated(payload) if payload.title == "renamed-session")
    });
    // assert
    assert!(
        title_event.is_some(),
        "SessionTitleUpdated event must be replayed"
    );
    assert_eq!(events.len(), 3, "all events must be preserved");
}

// ===========================================================================
// 5. LINEAGE TREE PROJECTION
// ===========================================================================

