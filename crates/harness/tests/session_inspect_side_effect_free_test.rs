use harness::UnwrapOrAbort;
use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

mod common;

use common::CliHarness;

/// S7 / T-session-inspect: list + inspect must not mutate events.jsonl.
///
/// `sessions list` hides scenario-fixture runs from operator history, so list
/// may return an empty array for golden_path; exit 0 still proves the path is
/// side-effect free. Inspect remains the authoritative read surface.
#[test]
fn session_list_and_inspect_leave_events_jsonl_bytes_unchanged() {
    // arrange
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let session_dir = temp_dir.path().join("sessions");
    let session_dir_str = session_dir.to_str().unwrap_or_abort();

    let run_output = CliHarness::new()
        .args([
            "run",
            "--scenario",
            "golden_path",
            "--deterministic",
            "--session-dir",
            session_dir_str,
        ])
        .capture_session_dir(&session_dir)
        .output();

    assert!(
        run_output.status.success(),
        "golden_path run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run_output.stdout),
        String::from_utf8_lossy(&run_output.stderr)
    );

    let captured = run_output.single_run();
    let events_path = &captured.events_path;
    let run_id = run_id_from_events_path(events_path);
    let digest_before = sha256_hex(&fs::read(events_path).unwrap_or_abort());

    // act
    let list_output = CliHarness::new()
        .args([
            "--session-dir",
            session_dir_str,
            "sessions",
            "list",
            "--json",
        ])
        .output();

    let inspect_output = CliHarness::new()
        .args([
            "--session-dir",
            session_dir_str,
            "sessions",
            "inspect",
            "--json",
            &run_id,
        ])
        .output();

    // assert
    assert!(
        list_output.status.success(),
        "sessions list failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&list_output.stdout),
        String::from_utf8_lossy(&list_output.stderr)
    );
    assert!(
        inspect_output.status.success(),
        "sessions inspect failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&inspect_output.stdout),
        String::from_utf8_lossy(&inspect_output.stderr)
    );

    let digest_after = sha256_hex(&fs::read(events_path).unwrap_or_abort());
    assert_eq!(
        digest_before, digest_after,
        "events.jsonl must be byte-identical after sessions list/inspect"
    );

    let list_rows: serde_json::Value =
        serde_json::from_slice(&list_output.stdout).unwrap_or_abort();
    assert!(
        list_rows.is_array(),
        "sessions list --json must emit a JSON array, got {list_rows}"
    );

    let inspect_summary: serde_json::Value =
        serde_json::from_slice(&inspect_output.stdout).unwrap_or_abort();
    assert_eq!(inspect_summary["catalog"]["run_id"], run_id);
    assert!(
        inspect_summary.get("replay").is_some(),
        "sessions inspect --json must include replay summary"
    );
}

fn run_id_from_events_path(events_path: &Path) -> String {
    events_path
        .parent()
        .and_then(|run_dir| run_dir.file_name())
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_abort()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
