use harness::UnwrapOrAbort;
use std::fs::FileTimes;
use std::time::{Duration, UNIX_EPOCH};

const INDEX_FILE_NAME: &str = ".session-history-index-v1.json";

#[test]
fn session_history_cli_builds_paginates_searches_and_rebuilds_index() {
    // arrange
    let session_dir = tempdir().unwrap_or_abort();
    for run_id in ["run_alpha", "run_beta", "run_gamma"] {
        let run_dir = session_dir.path().join(run_id);
        std::fs::create_dir_all(&run_dir).unwrap_or_abort();
        write_events_jsonl(&run_dir, &resumable_finished_events(run_id));
    }

    // act
    let first_page = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "list",
        "--json",
        "--limit",
        "2",
        "--offset",
        "0",
    ]);

    // assert
    assert!(
        first_page.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&first_page.stderr)
    );
    assert!(
        session_dir.path().join(INDEX_FILE_NAME).is_file(),
        "listing must persist the rebuildable history index"
    );
    let first_rows: serde_json::Value =
        serde_json::from_slice(&first_page.stdout).unwrap_or_abort();
    assert_eq!(first_rows.as_array().map(Vec::len), Some(2));
    let cursor = first_rows[1]["cursor"].as_str().unwrap_or_abort();

    let second_page = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "list",
        "--json",
        "--limit",
        "2",
        "--cursor",
        cursor,
    ]);
    assert!(second_page.status.success());
    let second_rows: serde_json::Value =
        serde_json::from_slice(&second_page.stdout).unwrap_or_abort();
    assert_eq!(second_rows.as_array().map(Vec::len), Some(1));
    assert_ne!(first_rows[0]["run_id"], second_rows[0]["run_id"]);

    let search = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "search",
        "beta",
        "--json",
    ]);
    assert!(
        search.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&search.stderr)
    );
    let search_rows: serde_json::Value =
        serde_json::from_slice(&search.stdout).unwrap_or_abort();
    assert_eq!(search_rows.as_array().map(Vec::len), Some(1));
    assert_eq!(search_rows[0]["run_id"], "run_beta");

    std::fs::write(
        session_dir.path().join(INDEX_FILE_NAME),
        "{corrupt index",
    )
    .unwrap_or_abort();
    let rebuild = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "rebuild-index",
        "--json",
    ]);
    assert!(
        rebuild.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&rebuild.stderr)
    );
    let rebuilt: serde_json::Value =
        serde_json::from_slice(&rebuild.stdout).unwrap_or_abort();
    assert_eq!(rebuilt["entry_count"], 3);
}

#[test]
fn session_history_index_reuses_rows_and_recovers_deleted_or_corrupt_state() {
    // arrange
    let session_dir = tempdir().unwrap_or_abort();
    for run_id in ["run_one", "run_two"] {
        let run_dir = session_dir.path().join(run_id);
        std::fs::create_dir_all(&run_dir).unwrap_or_abort();
        write_events_jsonl(&run_dir, &resumable_finished_events(run_id));
    }

    // act
    let first =
        harness::inspect_session_catalog_indexed(session_dir.path()).unwrap_or_abort();
    let second =
        harness::inspect_session_catalog_indexed(session_dir.path()).unwrap_or_abort();

    // assert
    assert!(first.rebuilt);
    assert_eq!(first.journals_scanned, 2);
    assert_eq!(first.recovery_reason.as_deref(), Some("missing"));
    assert!(!second.rebuilt);
    assert_eq!(
        second.journals_scanned, 0,
        "unchanged listing must not full-scan durable journals"
    );
    assert_eq!(
        first
            .entries
            .iter()
            .map(|entry| entry.catalog.run_id.as_str())
            .collect::<Vec<_>>(),
        second
            .entries
            .iter()
            .map(|entry| entry.catalog.run_id.as_str())
            .collect::<Vec<_>>()
    );

    std::fs::remove_file(session_dir.path().join(INDEX_FILE_NAME)).unwrap_or_abort();
    let rebuilt_deleted =
        harness::inspect_session_catalog_indexed(session_dir.path()).unwrap_or_abort();
    assert!(rebuilt_deleted.rebuilt);
    assert_eq!(rebuilt_deleted.journals_scanned, 2);
    assert_eq!(
        rebuilt_deleted.recovery_reason.as_deref(),
        Some("missing")
    );

    std::fs::write(
        session_dir.path().join(INDEX_FILE_NAME),
        "not valid json",
    )
    .unwrap_or_abort();
    let rebuilt_corrupt =
        harness::inspect_session_catalog_indexed(session_dir.path()).unwrap_or_abort();
    assert!(rebuilt_corrupt.rebuilt);
    assert_eq!(rebuilt_corrupt.journals_scanned, 2);
    assert_eq!(
        rebuilt_corrupt.recovery_reason.as_deref(),
        Some("corrupt")
    );

    std::fs::write(session_dir.path().join(INDEX_FILE_NAME), "{").unwrap_or_abort();
    let rebuilt_truncated =
        harness::inspect_session_catalog_indexed(session_dir.path()).unwrap_or_abort();
    assert_eq!(
        rebuilt_truncated.recovery_reason.as_deref(),
        Some("truncated")
    );

    std::fs::write(
        session_dir.path().join(INDEX_FILE_NAME),
        r#"{"schema_version":999,"entries":{}}"#,
    )
    .unwrap_or_abort();
    let rebuilt_unsupported =
        harness::inspect_session_catalog_indexed(session_dir.path()).unwrap_or_abort();
    assert_eq!(
        rebuilt_unsupported.recovery_reason.as_deref(),
        Some("unsupported_version")
    );

    let malformed_dir = session_dir.path().join("run_malformed");
    std::fs::create_dir_all(&malformed_dir).unwrap_or_abort();
    write_events_lines(&malformed_dir, &["{invalid journal"]);
    let with_malformed =
        harness::inspect_session_catalog_indexed(session_dir.path()).unwrap_or_abort();
    assert_eq!(with_malformed.entries.len(), 3);
    assert!(with_malformed
        .entries
        .iter()
        .any(|entry| entry.catalog.run_id == "run_one"));
}

#[test]
fn session_history_index_rejects_nested_run_dir_poisoning() {
    // arrange
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_safe");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(&run_dir, &resumable_finished_events("run_safe"));
    harness::inspect_session_catalog_indexed(session_dir.path()).unwrap_or_abort();
    let index_path = session_dir.path().join(INDEX_FILE_NAME);
    let mut index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&index_path).unwrap_or_abort()).unwrap_or_abort();
    let key = run_dir.to_str().unwrap_or_abort();
    index["entries"][key]["entry"]["run_dir"] =
        serde_json::Value::String("/tmp/poisoned-session".to_string());
    std::fs::write(
        &index_path,
        serde_json::to_vec_pretty(&index).unwrap_or_abort(),
    )
    .unwrap_or_abort();

    // act
    let report =
        harness::inspect_session_catalog_indexed(session_dir.path()).unwrap_or_abort();

    // assert
    assert_eq!(report.entries.len(), 1);
    assert_eq!(report.entries[0].run_dir, run_dir);
    assert_eq!(report.journals_scanned, 1);
}

#[cfg(unix)]
#[test]
fn session_history_index_temp_symlink_cannot_clobber_another_file() {
    // arrange
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_safe");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(&run_dir, &resumable_finished_events("run_safe"));
    let victim = session_dir.path().join("victim.txt");
    std::fs::write(&victim, "preserve me").unwrap_or_abort();
    std::os::unix::fs::symlink(
        &victim,
        session_dir
            .path()
            .join(".session-history-index-v1.json.tmp"),
    )
    .unwrap_or_abort();

    // act
    let rebuild = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "rebuild-index",
        "--json",
    ]);

    // assert
    assert!(
        rebuild.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&rebuild.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&victim).unwrap_or_abort(),
        "preserve me"
    );
    assert!(
        !std::fs::symlink_metadata(session_dir.path().join(INDEX_FILE_NAME))
            .unwrap_or_abort()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn cursor_distinguishes_equal_timestamp_and_run_id_by_run_dir() {
    // arrange
    let session_dir = tempdir().unwrap_or_abort();
    let modified = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    for directory in ["copy-a", "copy-b"] {
        let run_dir = session_dir.path().join(directory);
        std::fs::create_dir_all(&run_dir).unwrap_or_abort();
        write_events_jsonl(&run_dir, &resumable_finished_events("run_same"));
        std::fs::File::options()
            .write(true)
            .open(run_dir.join("events.jsonl"))
            .unwrap_or_abort()
            .set_times(FileTimes::new().set_modified(modified))
            .unwrap_or_abort();
    }

    // act
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "list",
        "--json",
    ]);

    // assert
    assert!(output.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap_or_abort();
    assert_ne!(rows[0]["cursor"], rows[1]["cursor"]);
}

#[test]
fn invalid_or_stale_cursor_fails_closed() {
    // arrange
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_cursor");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(&run_dir, &resumable_finished_events("run_cursor"));

    // act
    let output = run_harness([
        "--session-dir",
        session_dir.path().to_str().unwrap_or_abort(),
        "sessions",
        "list",
        "--json",
        "--cursor",
        "not-a-current-cursor",
    ]);

    // assert
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cursor"));
}

#[test]
fn warm_list_opens_zero_journals_through_counted_source() {
    // arrange
    let session_dir = tempdir().unwrap_or_abort();
    let run_dir = session_dir.path().join("run_warm");
    std::fs::create_dir_all(&run_dir).unwrap_or_abort();
    write_events_jsonl(&run_dir, &resumable_finished_events("run_warm"));
    let cold = harness::inspect_session_catalog_indexed(session_dir.path()).unwrap_or_abort();

    // act
    let warm = harness::inspect_session_catalog_indexed(session_dir.path()).unwrap_or_abort();

    // assert
    assert_eq!(cold.journals_opened, 1);
    assert_eq!(warm.journals_opened, 0);
}
