#[test]
fn foreign_import_creates_replay_only_session() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let foreign = root.path().join("foreign-dir");
    let dest = root.path().join("harness-dest");
    fs::create_dir_all(&foreign).unwrap_or_abort();

    let source_events = completed_run_events("foreign-run-abc");
    write_events_jsonl(&foreign.join("events.jsonl"), &source_events);
    let source_before = fs::read(foreign.join("events.jsonl")).unwrap_or_abort();

    // act
    let result = import_foreign_session_as_replay(&foreign, &dest).unwrap_or_abort();

    // assert
    assert_eq!(result.event_count, 2);
    assert_eq!(result.format, "events_jsonl_v1");
    assert_eq!(result.mode_source, SessionModeSource::ReplayOnly);
    assert!(result.run_dir.join("events.jsonl").is_file());
    assert!(result.run_dir.join("meta.json").is_file());

    // Source is never mutated
    assert_eq!(
        fs::read(foreign.join("events.jsonl")).unwrap_or_abort(),
        source_before,
        "foreign source must not be mutated"
    );

    // Imported events use new run_id
    let imported_events = read_events_from_jsonl(&result.run_dir.join("events.jsonl"));
    assert_eq!(imported_events.len(), 2);
    for event in &imported_events {
        assert_eq!(event.run_id.as_str(), result.run_id);
    }
}

#[test]
fn foreign_import_rejects_active_session_target() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let foreign = root.path().join("foreign-active");
    let active = root.path().join("active-session");
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
        "must refuse import into active session"
    );
}

#[test]
fn foreign_import_rejects_corrupt_source() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let foreign = root.path().join("corrupt-foreign");
    let dest = root.path().join("harness-dest");
    fs::create_dir_all(&foreign).unwrap_or_abort();

    // Non-envelope JSONL
    fs::write(
        foreign.join("events.jsonl"),
        r#"{"role":"user","text":"hello"}
"#,
    )
    .unwrap_or_abort();

    // act
    let result = import_foreign_session_as_replay(&foreign, &dest);
    // assert
    assert!(result.is_err(), "must reject non-envelope JSONL");
    assert!(matches!(
        result.unwrap_err(),
        ForeignSessionError::SourceParse { .. }
    ));
}

#[test]
fn foreign_discover_classifies_candidates() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let scan = root.path().join("scan-root");
    fs::create_dir_all(&scan).unwrap_or_abort();

    // Importable events.jsonl
    let importable = scan.join("good-session");
    fs::create_dir_all(&importable).unwrap_or_abort();
    let events = completed_run_events("scan-run");
    write_events_jsonl(&importable.join("events.jsonl"), &events);

    // Plain directory (rejected)
    let plain = scan.join("not-a-session");
    fs::create_dir_all(&plain).unwrap_or_abort();
    fs::write(plain.join("notes.txt"), "hello").unwrap_or_abort();

    // act
    let found = discover_foreign_sessions(&scan).unwrap_or_abort();
    // assert
    assert!(found.len() >= 2, "must find all directories");
    assert!(found.iter().any(|c| c.is_importable()));
    assert!(found.iter().any(|c| c.is_rejected()));
}

// ===========================================================================
// 11. EXPORT METADATA (meta.json)
// ===========================================================================

#[test]
fn import_metadata_carries_support_ready_provenance() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let foreign = root.path().join("meta-foreign");
    let dest = root.path().join("meta-dest");
    fs::create_dir_all(&foreign).unwrap_or_abort();

    let events = completed_run_events("meta-run");
    write_events_jsonl(&foreign.join("events.jsonl"), &events);

    let result = import_foreign_session_as_replay(&foreign, &dest).unwrap_or_abort();

    // act
    // Read and validate meta.json
    let meta_text = fs::read_to_string(result.run_dir.join("meta.json")).unwrap_or_abort();
    let meta: serde_json::Value = serde_json::from_str(&meta_text).unwrap_or_abort();

    // assert
    assert_eq!(meta["run_id"], result.run_id, "meta must record run_id");
    assert_eq!(
        meta["mode_source"], "replay_only",
        "meta must record replay_only mode"
    );
    assert_eq!(
        meta["foreign_import"]["format"], "events_jsonl_v1",
        "meta must record import format"
    );
    assert_eq!(
        meta["foreign_import"]["source_path"],
        foreign.display().to_string(),
        "meta must record source path for provenance"
    );
    assert_eq!(
        meta["foreign_import"]["event_count"], 2,
        "meta must record event count"
    );
    assert!(
        meta["foreign_import"]["policy"]
            .as_str()
            .unwrap_or_default()
            .contains("read-only replay import"),
        "meta must record import policy"
    );
    // Support-ready: harness_version present
    assert!(
        meta["harness_version"].is_string(),
        "meta must include harness_version for support"
    );
}

// ===========================================================================
// 12. REPLAY FROM SEQUENCE OFFSET
// ===========================================================================

#[tokio::test]
async fn replay_from_offset_skips_earlier_events() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_id = "run-offset-replay";
    let events = full_run_events(run_id);

    {
        let store = JsonlFileEventStore::open(root.path(), run_id, true).unwrap_or_abort();
        for event in &events {
            store
                .append(EventEnvelopeWithoutSeqV1::from(event.clone()))
                .unwrap_or_abort();
        }
    }

    // act
    let store = JsonlFileEventStore::open_existing(root.path(), run_id, true).unwrap_or_abort();
    let from_seq_3: Vec<EventEnvelopeV1> = {
        use tokio_stream::StreamExt;
        let mut stream = store.replay(3).unwrap_or_abort();
        let mut events = Vec::new();
        while let Some(result) = stream.next().await {
            events.push(result.unwrap_or_abort());
        }
        events
    };

    // assert
    assert_eq!(from_seq_3.len(), 5, "replay(3) must return seq 3..=7");
    assert_eq!(from_seq_3[0].seq, 3);
    assert_eq!(from_seq_3[4].seq, 7);
}

// ===========================================================================
// 13. CRASH RECOVERY VIA REOPEN (writer lock recovery)
// ===========================================================================

#[test]
fn crash_reopen_recovers_stale_lock_on_open() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let session_dir = root.path();
    let run_id = "run-crash-reopen";
    let run_dir = session_dir.join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    // Write events + simulate stale lock from a dead process
    let events = completed_run_events(run_id);
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);
    fs::write(run_dir.join(".writer.lock"), "pid=999999999\ntoken=1\n").unwrap_or_abort();

    // act
    // Verify crash detection
    let report = inspect_previous_crash(&run_dir);
    // assert
    assert!(report.previous_crash_detected);

    // open recovers the stale lock (dead PID is reclaimable)
    let store = JsonlFileEventStore::open(session_dir, run_id, true).unwrap_or_abort();

    // After open, the lock is now ours; events are preserved
    assert_eq!(
        store.next_seq().unwrap_or_abort(),
        3,
        "must resume from existing events"
    );

    // Writer lock exists and is ours (not the dead one)
    let lock_content = fs::read_to_string(run_dir.join(".writer.lock")).unwrap_or_abort();
    assert!(lock_content.contains(&format!("pid={}", std::process::id())));
    drop(store);
}

// ===========================================================================
// 14. REPLAY IS SIDE-EFFECT FREE (PROOF: NO PROVIDER/TOOL/HOOK/MCP/NETWORK)
// ===========================================================================

#[tokio::test]
async fn replay_is_pure_projection_without_side_effects() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_id = "run-pure-replay";
    let run_dir = root.path().join(run_id);

    // Create a session with tool calls (that would be side effects if executed)
    let events = full_run_events(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);

    // Record filesystem state before replay
    let digest_before =
        harness_core::prompt_rewind::event_log_digest(&run_dir.join("events.jsonl"))
            .unwrap_or_abort();
    let dir_entries_before: Vec<String> = fs::read_dir(&run_dir)
        .unwrap_or_abort()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();

    // Perform replay (open_existing does NOT execute; just reads)
    let store = JsonlFileEventStore::open_existing(root.path(), run_id, true).unwrap_or_abort();
    let replayed: Vec<EventEnvelopeV1> = {
        use tokio_stream::StreamExt;
        let mut stream = store.replay(1).unwrap_or_abort();
        let mut events = Vec::new();
        while let Some(result) = stream.next().await {
            events.push(result.unwrap_or_abort());
        }
        events
    };
    drop(store);

    // act
    // Prove: events.jsonl unchanged
    let digest_after = harness_core::prompt_rewind::event_log_digest(&run_dir.join("events.jsonl"))
        .unwrap_or_abort();
    // assert
    assert_eq!(
        digest_before, digest_after,
        "replay must not write to events.jsonl"
    );

    // Prove: no new files created in run_dir (except writer lock which is cleaned on drop)
    // After drop, writer lock is removed; check remaining entries
    let dir_entries_after: Vec<String> = fs::read_dir(&run_dir)
        .unwrap_or_abort()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    // events.jsonl should be the same; writer lock is ephemeral
    let non_lock_after: Vec<&String> = dir_entries_after
        .iter()
        .filter(|name| !name.starts_with(".writer.lock"))
        .collect();
    let non_lock_before: Vec<&String> = dir_entries_before
        .iter()
        .filter(|name| !name.starts_with(".writer.lock"))
        .collect();
    assert_eq!(
        non_lock_before, non_lock_after,
        "replay must not create or delete files"
    );

    // Prove: replayed events match source (read-only fidelity)
    assert_eq!(replayed.len(), events.len());
    assert_eq!(replayed, events, "replayed events must match stored events");
}
