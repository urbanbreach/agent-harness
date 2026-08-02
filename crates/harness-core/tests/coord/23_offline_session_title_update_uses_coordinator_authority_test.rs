use harness_core::coord::update_session_title_offline;
use harness_core::UnwrapOrAbort;

fn seed_finished_session(session_dir: &Path, run_id: &str) -> PathBuf {
    let run_dir = session_dir.join(run_id);
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    let events = [
        serde_json::json!({
            "schema_version": 1,
            "event_id": "evt-1",
            "seq": 1,
            "run_id": run_id,
            "mono_ms": 1,
            "actor": { "kind": "system" },
            "payload": {
                "event_type": "run_started",
                "data": { "run_name": "test", "workspace_root": "/tmp" }
            }
        }),
        serde_json::json!({
            "schema_version": 1,
            "event_id": "evt-2",
            "seq": 2,
            "run_id": run_id,
            "mono_ms": 2,
            "actor": { "kind": "system" },
            "payload": {
                "event_type": "run_finished",
                "data": { "summary": "done" }
            }
        }),
    ];
    let body = events
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_abort()
        .join("\n")
        + "\n";
    fs::write(run_dir.join("events.jsonl"), body).unwrap_or_abort();
    run_dir
}

#[test]
fn offline_title_update_appends_via_coordinator_authority() {
    // arrange: a finished session with two events
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = seed_finished_session(temp_dir.path(), "run_offline_rename");
    let events_before = fs::read(run_dir.join("events.jsonl")).unwrap_or_abort();
    let clock = FakeClock::new();

    // act: offline title update through coordinator authority
    let envelope =
        update_session_title_offline(&clock, temp_dir.path(), "run_offline_rename", "Offline Title")
            .unwrap_or_abort();

    // assert: event appended with correct seq and payload
    assert_eq!(envelope.seq, 3);
    match &envelope.payload {
        EventV1::SessionTitleUpdated(payload) => assert_eq!(payload.title, "Offline Title"),
        other => panic!("expected SessionTitleUpdated, got {other:?}"),
    }

    // And: existing events are preserved (append-only invariant)
    let events_after = fs::read(run_dir.join("events.jsonl")).unwrap_or_abort();
    assert!(
        events_after.starts_with(&events_before),
        "existing events must be preserved"
    );
    assert!(
        events_after.len() > events_before.len(),
        "events.jsonl must have grown"
    );
}

#[test]
fn offline_title_update_rejects_empty_title() {
    // arrange: a finished session
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    seed_finished_session(temp_dir.path(), "run_offline_empty");
    let clock = FakeClock::new();

    // act: offline title update with whitespace-only title
    let err = update_session_title_offline(&clock, temp_dir.path(), "run_offline_empty", "  ")
        .expect_err("empty title should be rejected");

    // assert: fail-closed with InvalidSessionTitle
    assert!(matches!(err, CoordinatorError::InvalidSessionTitle));
}

#[test]
fn offline_title_update_rejects_when_writer_lock_held() {
    // arrange: a session with an active writer lock
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = seed_finished_session(temp_dir.path(), "run_offline_locked");
    fs::write(run_dir.join(".writer.lock"), "pid=1\ntoken=1\n").unwrap_or_abort();
    let clock = FakeClock::new();

    // act: offline title update while lock is held
    let result =
        update_session_title_offline(&clock, temp_dir.path(), "run_offline_locked", "Title");

    // assert: fail-closed — cannot mutate an active session
    let err = result.expect_err("writer lock held should prevent append");
    assert!(matches!(err, CoordinatorError::EventStore(_)));
    assert!(
        err.to_string().contains("writer lock"),
        "error should mention writer lock: {err}"
    );
}
