#[test]
fn rewind_plan_projects_conversation_through_cutoff() {
    // arrange
    let events = full_run_events("run-rewind-001");

    // act
    let plan = plan_prompt_rewind(&events, 4).unwrap_or_abort();
    // assert
    assert_eq!(plan.cutoff_seq, 4);
    assert_eq!(plan.retained_event_count, 4);
    assert_eq!(plan.discarded_event_count, 3);
    assert!(plan.events_append_only, "events must stay append-only");

    // Conversation should contain the user message up to cutoff
    assert!(
        !plan.conversation.messages.is_empty(),
        "conversation projection must be non-empty"
    );
}

#[test]
fn rewind_plan_fails_on_empty_log() {
    // arrange
    let result = plan_prompt_rewind(&[], 1);
    // act
    // assert
    assert!(matches!(result, Err(PromptRewindError::EmptyEventLog)));
}

#[test]
fn rewind_plan_fails_on_out_of_range_cutoff() {
    // arrange
    let events = full_run_events("run-rewind-oob");
    // act
    let result = plan_prompt_rewind(&events, 99);
    // assert
    assert!(matches!(
        result,
        Err(PromptRewindError::CutoffOutOfRange {
            cutoff_seq: 99,
            max_seq: 7
        })
    ));
}

#[test]
fn rewind_does_not_rewrite_events_jsonl() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_dir = root.path().join("run-rewind-appendonly");
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    let events = full_run_events("run-rewind-appendonly");
    write_events_jsonl(&run_dir.join("events.jsonl"), &events);

    let digest_before =
        harness_core::prompt_rewind::event_log_digest(&run_dir.join("events.jsonl"))
            .unwrap_or_abort();

    // Plan rewind (read-only operation)
    let _plan = plan_prompt_rewind(&events, 3).unwrap_or_abort();

    // act
    let digest_after = harness_core::prompt_rewind::event_log_digest(&run_dir.join("events.jsonl"))
        .unwrap_or_abort();
    // assert
    assert_eq!(
        digest_before, digest_after,
        "plan_prompt_rewind must not modify events.jsonl"
    );
}

#[test]
fn atomic_rewind_restores_workspace_files_atomically() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let workspace = root.path().join("workspace");
    fs::create_dir_all(workspace.join("src")).unwrap_or_abort();

    // Setup workspace files
    fs::write(
        workspace.join("src/main.rs"),
        "fn main() { println!(\"v2\"); }",
    )
    .unwrap_or_abort();
    fs::write(workspace.join("README.md"), "# Version 2").unwrap_or_abort();

    let events = full_run_events("run-atomic-rewind");

    // File snapshot: restore to "version 1" content
    let snapshot = vec![
        FileSnapshotEntry {
            path: "src/main.rs".to_string(),
            content: "fn main() { println!(\"v1\"); }".to_string(),
        },
        FileSnapshotEntry {
            path: "README.md".to_string(),
            content: "# Version 1".to_string(),
        },
    ];

    // act
    let result = atomic_prompt_rewind(&events, 4, &workspace, &snapshot).unwrap_or_abort();

    // assert
    assert_eq!(result.files_restored, 2, "both files must be restored");
    assert_eq!(result.files_unchanged, 0);
    assert!(result.events_append_only);
    assert_eq!(result.conversation.cutoff_seq, 4);

    // Verify files were restored
    assert_eq!(
        fs::read_to_string(workspace.join("src/main.rs")).unwrap_or_abort(),
        "fn main() { println!(\"v1\"); }"
    );
    assert_eq!(
        fs::read_to_string(workspace.join("README.md")).unwrap_or_abort(),
        "# Version 1"
    );
}

#[test]
fn atomic_rewind_with_empty_snapshot_is_conversation_only() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let workspace = root.path();

    // act
    let events = full_run_events("run-atomic-empty-snap");
    let result = atomic_prompt_rewind(&events, 3, workspace, &[]).unwrap_or_abort();

    // assert
    assert_eq!(result.files_restored, 0);
    assert_eq!(result.files_unchanged, 0);
    assert!(result.events_append_only);
}

// ===========================================================================
// 9. CRASH SCAN + RECOVERY
// ===========================================================================

#[test]
fn crash_scan_detects_stale_writer_lock() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let sessions_root = root.path().join("sessions");
    let run_dir = sessions_root.join("run-crashed-001");
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    // Simulate crash: stale lock with dead PID + events.jsonl present
    fs::write(run_dir.join(".writer.lock"), "pid=999999999\ntoken=1\n").unwrap_or_abort();
    fs::write(run_dir.join("events.jsonl"), "").unwrap_or_abort();

    // act
    let report = inspect_previous_crash(&run_dir);
    // assert
    assert!(report.previous_crash_detected, "must detect previous crash");
    assert!(report.stale_writer_lock, "must flag stale writer lock");
    assert!(report.events_log_present, "events.jsonl is present");
    assert!(
        report.recovery_message.is_some(),
        "must provide recovery message"
    );
    assert_eq!(
        report.recovery_action,
        Some(CrashRecoveryAction::OpenRecovers),
        "recovery action must be open_recovers"
    );
}

#[test]
fn crash_scan_detects_recovery_marker() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_dir = root.path().join("run-recovery-marker");
    fs::create_dir_all(&run_dir).unwrap_or_abort();

    // Recovery marker present (no writer lock)
    fs::write(run_dir.join(".writer.lock.recovering"), "").unwrap_or_abort();
    fs::write(run_dir.join("events.jsonl"), "").unwrap_or_abort();

    // act
    let report = inspect_previous_crash(&run_dir);
    // assert
    assert!(report.previous_crash_detected);
    assert!(report.recovery_marker_present);
    assert!(!report.stale_writer_lock);
}

#[test]
fn crash_scan_reports_clean_for_healthy_session() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let run_dir = root.path().join("run-healthy");
    fs::create_dir_all(&run_dir).unwrap_or_abort();
    fs::write(run_dir.join("events.jsonl"), "{}\n").unwrap_or_abort();

    // act
    let report = inspect_previous_crash(&run_dir);
    // assert
    assert!(
        !report.previous_crash_detected,
        "healthy session must not flag crash"
    );
    assert!(!report.stale_writer_lock);
    assert!(!report.recovery_marker_present);
    assert!(report.recovery_message.is_none());
}

#[test]
fn crash_scan_multi_directory_counts_correctly() {
    // arrange
    let root = tempdir().unwrap_or_abort();
    let sessions_root = root.path().join("sessions");

    // Healthy session
    let healthy = sessions_root.join("run-healthy");
    fs::create_dir_all(&healthy).unwrap_or_abort();
    fs::write(healthy.join("events.jsonl"), "").unwrap_or_abort();

    // Crashed session (stale lock)
    let crashed = sessions_root.join("run-crashed");
    fs::create_dir_all(&crashed).unwrap_or_abort();
    fs::write(crashed.join(".writer.lock"), "pid=999999999\ntoken=1\n").unwrap_or_abort();
    fs::write(crashed.join("events.jsonl"), "").unwrap_or_abort();

    // act
    let reports = scan_previous_crashes(&sessions_root);
    // assert
    assert_eq!(reports.len(), 2, "must scan all session directories");

    let crashed_reports: Vec<_> = reports
        .iter()
        .filter(|r| r.previous_crash_detected)
        .collect();
    assert_eq!(crashed_reports.len(), 1, "exactly one crashed session");
}

// ===========================================================================
// 10. FOREIGN-SESSION IMPORT
// ===========================================================================

