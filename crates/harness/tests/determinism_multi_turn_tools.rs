use std::fs;
use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};

mod common;

use common::repo_root;

#[test]
fn deterministic_multi_turn_tools_twice_produces_identical_sha256_digest() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let session_dir = temp_dir.path().join("sessions");
    let output_a = temp_dir.path().join("run-a.jsonl");
    let output_b = temp_dir.path().join("run-b.jsonl");

    run_scenario(&session_dir, &output_a);
    run_scenario(&session_dir, &output_b);

    let bytes_a = fs::read(&output_a).expect("read first jsonl output");
    let bytes_b = fs::read(&output_b).expect("read second jsonl output");

    assert_eq!(sha256_hex(&bytes_a), sha256_hex(&bytes_b));
    assert!(has_event_type(&bytes_a, "tool_call_requested"));
    assert!(has_event_type(&bytes_a, "tool_call_finished"));
}

fn run_scenario(session_dir: &Path, out_path: &Path) {
    let repo_root = repo_root();

    let status = Command::new("cargo")
        .arg("run")
        .arg("-p")
        .arg("harness")
        .arg("--")
        .arg("run")
        .arg("--scenario")
        .arg("golden_path")
        .arg("--deterministic")
        .arg("--session-dir")
        .arg(session_dir)
        .arg("--out")
        .arg(out_path)
        .env("HARNESS_DETERMINISTIC", "1")
        .current_dir(&repo_root)
        .status()
        .expect("spawn harness run command");

    assert!(status.success(), "harness run failed with status {status}");
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
