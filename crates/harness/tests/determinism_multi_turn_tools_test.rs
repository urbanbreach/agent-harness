use harness::UnwrapOrAbort;
use std::fs;

use sha2::{Digest, Sha256};

mod common;

use common::CliHarness;

#[test]
fn deterministic_multi_turn_tools_twice_produces_identical_sha256_digest() {
    // arrange
    // act
    // assert
    let temp_dir = tempfile::tempdir().unwrap_or_abort();
    let session_dir = temp_dir.path().join("sessions");
    let output_a = temp_dir.path().join("run-a.jsonl");
    let output_b = temp_dir.path().join("run-b.jsonl");

    run_scenario(session_dir.as_path(), output_a.as_path());
    run_scenario(session_dir.as_path(), output_b.as_path());

    let bytes_a = fs::read(&output_a).unwrap_or_abort();
    let bytes_b = fs::read(&output_b).unwrap_or_abort();

    assert_eq!(sha256_hex(&bytes_a), sha256_hex(&bytes_b));
    assert!(has_event_type(&bytes_a, "tool_call_requested"));
    assert!(has_event_type(&bytes_a, "tool_call_finished"));
}

fn run_scenario(session_dir: &std::path::Path, out_path: &std::path::Path) {
    let output = CliHarness::new()
        .args([
            "run".into(),
            "--scenario".into(),
            "golden_path".into(),
            "--deterministic".into(),
            "--session-dir".into(),
            session_dir.as_os_str().to_owned(),
            "--out".into(),
            out_path.as_os_str().to_owned(),
        ])
        .output();

    assert!(
        output.status.success(),
        "harness run failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn has_event_type(bytes: &[u8], event_type: &str) -> bool {
    let body = String::from_utf8_lossy(bytes);
    body.lines().any(|line| {
        serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|value| {
                value
                    .get("payload")
                    .and_then(|payload| payload.get("event_type"))
                    .and_then(serde_json::Value::as_str)
                    .map(|found| found == event_type)
            })
            .unwrap_or(false)
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
