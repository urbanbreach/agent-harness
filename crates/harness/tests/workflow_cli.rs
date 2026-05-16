use std::env;
use std::fs;
use std::process::Command;

use serde_json::Value;
use tempfile::tempdir;

mod common;

use common::repo_root;

fn harness_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_harness"));
    command
        .env_remove("HARNESS_CONFIG")
        .env_remove("HARNESS_CONFIG_CONTENT")
        .env_remove("HARNESS_TUI_CONFIG")
        .env("HOME", "/nonexistent")
        .env("XDG_CONFIG_HOME", "/nonexistent");
    command
}

#[test]
fn workflow_snapshot_write_cli_creates_redacted_artifact_and_workflow_evidence() {
    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");

    let output = harness_command()
        .current_dir(repo_root())
        .args([
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "workflow",
            "snapshot",
            "write",
            "--workflow-id",
            "wf_cli_snapshot",
            "--source-command",
            "/interview",
            "--task",
            "Investigate sk-ABCDE12345ABCDE before workflow run",
            "--desired-outcome",
            "A redacted context snapshot artifact",
            "--ambiguity-score",
            "0.35",
            "--json",
        ])
        .output()
        .expect("run workflow snapshot write");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("snapshot write json");
    let run_dir = report["run_dir"].as_str().expect("run dir");
    let events_path = report["events_path"].as_str().expect("events path");
    let artifact_path = report["snapshot"]["artifact_path"]
        .as_str()
        .expect("artifact path");

    assert!(artifact_path.starts_with("artifacts/context_snapshots/ctx_"));
    assert_eq!(
        report["snapshot"]["artifact_digest"]
            .as_str()
            .expect("digest")
            .len(),
        64
    );

    let artifact_body = fs::read_to_string(std::path::Path::new(run_dir).join(artifact_path))
        .expect("read snapshot artifact");
    assert!(!artifact_body.contains("sk-ABCDE12345ABCDE"));
    assert!(artifact_body.contains("[REDACTED_API_KEY]"));
    assert!(artifact_body.contains("/interview"));

    let events = fs::read_to_string(events_path).expect("read events jsonl");
    assert!(events.contains("evidence.context_snapshot"));
    assert!(events.contains("wf_cli_snapshot"));
    assert!(events.contains(artifact_path));
}
