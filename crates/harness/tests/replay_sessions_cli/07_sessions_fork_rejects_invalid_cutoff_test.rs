use harness::UnwrapOrAbort;
#[test]
fn sessions_fork_rejects_invalid_cutoff() {
    let session_dir = tempdir().unwrap_or_abort();
    let source_dir = session_dir.path().join("unstable_source");
    std::fs::create_dir_all(&source_dir).unwrap_or_abort();
    write_events_jsonl(
        &source_dir,
        &[
            envelope(
                "run_unstable_source",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_unstable_source",
                2,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_in_flight".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:default:gpt-5.4-mini".to_string()),
                }),
            ),
            envelope(
                "run_unstable_source",
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "fork",
            "--source",
            "run_unstable_source",
            "--cutoff",
            "2",
            "--json",
        ]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Harness session fork failed"));
    assert!(stderr.contains("prefix ending at seq 2 is unstable"));
    assert!(stderr.contains("tasks are still in flight: task_in_flight"));
}
#[test]
fn sessions_fork_clone_reject_invalid_source_selector() {
    let session_dir = tempdir().unwrap_or_abort();

    let fork_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "fork",
            "--source",
            "missing_lineage_source",
            "--cutoff",
            "1",
            "--json",
        ]);
    assert!(!fork_output.status.success());
    let fork_stderr = String::from_utf8_lossy(&fork_output.stderr);
    assert!(fork_stderr.contains("Harness session fork failed"));
    assert!(fork_stderr.contains("no saved session matched `missing_lineage_source`"));

    let clone_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "clone",
            "--source",
            "missing_lineage_source",
            "--json",
        ]);
    assert!(!clone_output.status.success());
    let clone_stderr = String::from_utf8_lossy(&clone_output.stderr);
    assert!(clone_stderr.contains("Harness session clone failed"));
    assert!(clone_stderr.contains("no saved session matched `missing_lineage_source`"));
}
#[test]
fn sessions_fork_clone_reject_ambiguous_source_selector() {
    let session_dir = tempdir().unwrap_or_abort();
    for run_dir_name in ["ambiguous_source_a", "ambiguous_source_b"] {
        let run_dir = session_dir.path().join(run_dir_name);
        std::fs::create_dir_all(&run_dir).unwrap_or_abort();
        write_events_jsonl(&run_dir, &resumable_finished_events("run_ambiguous_source"));
    }

    let fork_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "fork",
            "--source",
            "run_ambiguous_source",
            "--cutoff",
            "5",
            "--json",
        ]);
    assert!(!fork_output.status.success());
    let fork_stderr = String::from_utf8_lossy(&fork_output.stderr);
    assert!(fork_stderr.contains("Harness session fork failed"));
    assert!(fork_stderr.contains("multiple saved sessions matched `run_ambiguous_source`"));

    let clone_output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "clone",
            "--source",
            "run_ambiguous_source",
            "--json",
        ]);
    assert!(!clone_output.status.success());
    let clone_stderr = String::from_utf8_lossy(&clone_output.stderr);
    assert!(clone_stderr.contains("Harness session clone failed"));
    assert!(clone_stderr.contains("multiple saved sessions matched `run_ambiguous_source`"));
}
#[test]
fn sessions_fork_rejects_cutoff_beyond_log() {
    let session_dir = tempdir().unwrap_or_abort();
    let source_dir = session_dir.path().join("short_source");
    std::fs::create_dir_all(&source_dir).unwrap_or_abort();
    write_events_jsonl(
        &source_dir,
        &resumable_finished_events("run_short_lineage_source"),
    );

    let output = run_harness([
            "--session-dir",
            session_dir.path().to_str().unwrap_or_abort(),
            "sessions",
            "fork",
            "--source",
            "run_short_lineage_source",
            "--cutoff",
            "99",
            "--json",
        ]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Harness session fork failed"));
    assert!(stderr.contains("stable prefix cutoff seq 99 is outside event log range 0..=5"));
}
#[test]
fn replay_cli_fails_when_events_are_missing() {
    let run_dir = tempdir().unwrap_or_abort();
    let output = run_harness([
            "replay",
            "--session",
            run_dir.path().to_str().unwrap_or_abort(),
        ]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("replay failed:"));
    assert!(stderr.contains("events.jsonl"));
}
