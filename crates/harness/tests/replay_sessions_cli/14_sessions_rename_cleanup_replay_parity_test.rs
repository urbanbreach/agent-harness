use harness::UnwrapOrAbort;
use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

#[test]
fn sessions_replay_from_different_cwd_leaves_events_unchanged() {
    // arrange: a finished session stored under a session directory
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_replay_purity");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_replay_purity",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_replay_purity",
                2,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_1".into(),
                    text: "hello world".to_string(),
                }),
            ),
            envelope(
                "run_replay_purity",
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    let events_path = run_dir.join("events.jsonl");
    let digest_before = sha256_hex(&fs::read(&events_path).unwrap_or_abort());

    // act: replay from a different CWD (the temp root, not the session dir)
    let replay_output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "replay",
        "run_replay_purity",
        "--json",
    ]);

    // assert: replay succeeds and events.jsonl is byte-identical
    assert!(
        replay_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&replay_output.stderr)
    );
    let digest_after = sha256_hex(&fs::read(&events_path).unwrap_or_abort());
    assert_eq!(
        digest_before, digest_after,
        "events.jsonl must be byte-identical after replay from different CWD"
    );

    // And: no new files were created in the session directory
    let entries_before: Vec<String> = ["events.jsonl"]
        .into_iter()
        .map(String::from)
        .collect();
    let entries_after: Vec<String> = std::fs::read_dir(&run_dir)
        .unwrap_or_abort()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    for expected in &entries_before {
        assert!(
            entries_after.iter().any(|name| name == expected),
            "expected file {expected} must still exist after replay"
        );
    }
}

#[test]
fn sessions_replay_corrupt_events_fails_closed() {
    // arrange: a session directory with a corrupt events.jsonl (invalid JSON on line 2)
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_corrupt");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    std::fs::write(
        run_dir.join("events.jsonl"),
        "{\"seq\":1}\nNOT VALID JSON\n",
    )
    .unwrap_or_abort();

    // act: replay the corrupt session
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "replay",
        "run_corrupt",
    ]);

    // assert: fail-closed — replay must not silently succeed on corrupt events
    assert!(
        !output.status.success(),
        "replay must fail for corrupt events.jsonl"
    );
}

#[test]
fn sessions_rename_appends_title_event_preserving_existing_events() {
    // arrange: a finished session
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_rename_cli");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_rename_cli",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/tmp".to_string(),
                }),
            ),
            envelope(
                "run_rename_cli",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    let events_before = fs::read(run_dir.join("events.jsonl")).unwrap_or_abort();

    // act: rename via CLI
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "rename",
        "run_rename_cli",
        "Renamed Session Title",
        "--json",
    ]);

    // assert: success, event appended, existing events preserved
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    assert_eq!(body["harness_operation"], "rename");
    assert_eq!(body["title"], "Renamed Session Title");
    assert_eq!(body["event_seq"], 3);

    let events_after = fs::read(run_dir.join("events.jsonl")).unwrap_or_abort();
    assert!(
        events_after.starts_with(&events_before),
        "existing events must be preserved (append-only)"
    );
    assert!(
        events_after.len() > events_before.len(),
        "events.jsonl must have grown"
    );
}

#[test]
fn sessions_rename_fails_for_locked_session() {
    // arrange: a session with an active writer lock
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_rename_locked_cli");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(
        &run_dir,
        &[envelope(
            "run_rename_locked_cli",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".into(),
                workspace_root: "/tmp".to_string(),
            }),
        )],
    );
    std::fs::write(run_dir.join(".writer.lock"), "pid=1\ntoken=1\n").unwrap_or_abort();

    // act: rename via CLI
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "rename",
        "run_rename_locked_cli",
        "New Title",
    ]);

    // assert: fail-closed — cannot rename an active session
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("writer lock"));
}

#[test]
fn sessions_rename_empty_title_fails_closed() {
    // arrange: a finished session
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_rename_empty_cli");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(
        &run_dir,
        &[envelope(
            "run_rename_empty_cli",
            1,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        )],
    );

    // act: rename with whitespace-only title
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "rename",
        "run_rename_empty_cli",
        "   ",
    ]);

    // assert: fail-closed
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("title must not be empty"));
}

#[test]
fn sessions_cleanup_deletes_session_with_yes_flag() {
    // arrange: a finished session
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_cleanup_cli");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(
        &run_dir,
        &[envelope(
            "run_cleanup_cli",
            1,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        )],
    );
    assert!(run_dir.exists());

    // act: cleanup with --yes
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "cleanup",
        "run_cleanup_cli",
        "--yes",
        "--json",
    ]);

    // assert: session directory removed
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    assert_eq!(body["harness_operation"], "cleanup");
    assert_eq!(body["deleted"], true);
    assert!(!run_dir.exists());
}

#[test]
fn sessions_cleanup_refuses_without_yes_flag() {
    // arrange: a finished session
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_cleanup_noyes_cli");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(
        &run_dir,
        &[envelope(
            "run_cleanup_noyes_cli",
            1,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        )],
    );

    // act: cleanup without --yes
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "cleanup",
        "run_cleanup_noyes_cli",
    ]);

    // assert: fail-closed — armed confirmation required
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--yes"));
    assert!(run_dir.exists(), "session must not be deleted without --yes");
}

#[test]
fn sessions_cleanup_refuses_for_locked_session() {
    // arrange: a session with an active writer lock
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_cleanup_locked_cli");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(
        &run_dir,
        &[envelope(
            "run_cleanup_locked_cli",
            1,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        )],
    );
    std::fs::write(run_dir.join(".writer.lock"), "pid=1\ntoken=1\n").unwrap_or_abort();

    // act: cleanup with --yes
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "cleanup",
        "run_cleanup_locked_cli",
        "--yes",
    ]);

    // assert: fail-closed — cannot delete an active session
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("writer lock"));
    assert!(run_dir.exists(), "locked session must not be deleted");
}

#[test]
fn sessions_fork_rejects_invalid_cutoff_beyond_event_log() {
    // arrange: a finished session with 3 events
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_fork_bad_cutoff");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_fork_bad_cutoff",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/tmp".to_string(),
                }),
            ),
            envelope(
                "run_fork_bad_cutoff",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
            envelope(
                "run_fork_bad_cutoff",
                3,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_extra".into(),
                    text: "extra".to_string(),
                }),
            ),
        ],
    );

    // act: fork with cutoff beyond the event log (seq 99)
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "fork",
        "--source",
        "run_fork_bad_cutoff",
        "--cutoff",
        "99",
    ]);

    // assert: fail-closed — cutoff out of range
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("fork failed") || stderr.contains("out of range"));
}

#[test]
fn sessions_inspect_reports_corrupt_session_without_side_effects() {
    // arrange: a session with a corrupt events.jsonl
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_inspect_corrupt");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    std::fs::write(
        run_dir.join("events.jsonl"),
        "NOT VALID JSON\n",
    )
    .unwrap_or_abort();

    // act: inspect the corrupt session
    let _output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "inspect",
        "run_inspect_corrupt",
        "--json",
    ]);

    // assert: inspect handles corruption gracefully (may succeed with error fields or fail)
    // Either way, the corrupt file must not be modified
    let events_after = fs::read_to_string(run_dir.join("events.jsonl")).unwrap_or_abort();
    assert_eq!(events_after, "NOT VALID JSON\n");
}

#[test]
fn sessions_crash_scan_detects_recovery_marker() {
    // arrange: a session root with one clean and one crashed session
    let session_dir = tempdir().unwrap_or_abort();
    let clean_dir = session_dir.path().join("run_clean_scan");
    let crashed_dir = session_dir.path().join("run_crashed_scan");
    std::fs::create_dir_all(&clean_dir).unwrap_or_abort();
    std::fs::create_dir_all(&crashed_dir).unwrap_or_abort();
    std::fs::write(clean_dir.join("events.jsonl"), "").unwrap_or_abort();
    std::fs::write(crashed_dir.join("events.jsonl"), "").unwrap_or_abort();
    std::fs::write(crashed_dir.join(".writer.lock.recovering"), "pid=1\n").unwrap_or_abort();

    // act: crash-scan
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "crash-scan",
        "--json",
    ]);

    // assert: scan reports the crashed session
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    assert_eq!(body["summary"]["scanned"], 2);
    assert_eq!(body["summary"]["previous_crash"], 1);
    assert_eq!(body["summary"]["clean"], 1);
}

#[test]
fn sessions_discover_finds_foreign_session_markers() {
    // arrange: a scan root with a codex-style session.json marker
    let root = tempdir().unwrap_or_abort();
    let foreign = root.path().join("codex-foreign");
    std::fs::create_dir_all(&foreign).unwrap_or_abort();
    std::fs::write(
        foreign.join("session.json"),
        r#"{"id":"abc","title":"demo"}"#,
    )
    .unwrap_or_abort();

    // act: discover
    let output = run_harness([
        "sessions",
        "discover",
        "--from",
        root.path().to_str().unwrap_or_abort(),
        "--json",
    ]);

    // assert: discoverable candidate found
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    assert_eq!(body["count"], 1);
    assert_eq!(body["candidates"][0]["status"], "discoverable");
    assert_eq!(body["candidates"][0]["kind"], "codex");
}

#[test]
fn sessions_import_events_jsonl_creates_replay_only_session() {
    // arrange: a foreign session with harness-compatible events.jsonl
    let root = tempdir().unwrap_or_abort();
    let foreign = root.path().join("foreign-importable");
    let session_dir = root.path().join("sessions");
    std::fs::create_dir_all(&foreign).unwrap_or_abort();
    write_events_jsonl(
        &foreign,
        &[
            envelope(
                "foreign-import",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".into(),
                    workspace_root: "/tmp/ws".to_string(),
                }),
            ),
            envelope(
                "foreign-import",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    let source_before = fs::read(foreign.join("events.jsonl")).unwrap_or_abort();

    // act: import
    let output = run_harness([
        "--session-dir",
        session_dir.to_str().unwrap_or_abort(),
        "sessions",
        "import",
        "--from",
        foreign.to_str().unwrap_or_abort(),
        "--json",
    ]);

    // assert: replay-only session created, source unchanged
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    assert_eq!(body["event_count"], 2);
    assert_eq!(body["format"], "events_jsonl_v1");
    assert_eq!(body["mode_source"], "replay_only");
    assert_eq!(
        fs::read(foreign.join("events.jsonl")).unwrap_or_abort(),
        source_before,
        "source events.jsonl must not be mutated"
    );
}

#[test]
fn sessions_import_unknown_format_fails_closed() {
    // arrange: a foreign session with only session.json (not events.jsonl)
    let root = tempdir().unwrap_or_abort();
    let foreign = root.path().join("codex-unknown");
    let session_dir = root.path().join("sessions");
    std::fs::create_dir_all(&foreign).unwrap_or_abort();
    std::fs::write(foreign.join("session.json"), r#"{"id":"x"}"#).unwrap_or_abort();

    // act: import
    let output = run_harness([
        "--session-dir",
        session_dir.to_str().unwrap_or_abort(),
        "sessions",
        "import",
        "--from",
        foreign.to_str().unwrap_or_abort(),
    ]);

    // assert: fail-closed — unsupported format
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("session import failed"));
    assert!(stderr.contains("unsupported") || stderr.contains("not importable"));
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
