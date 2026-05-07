use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

mod common;

use common::repo_root;

#[test]
fn stress_harness_script_offline_mode_writes_summary_and_stage_artifacts() {
    let repo_root = repo_root();
    let script_path = repo_root.join("scripts/stress-harness.sh");
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let artifact_dir = temp_dir.path().join("artifacts");

    let output = Command::new("bash")
        .current_dir(&repo_root)
        .arg(script_path)
        .arg("--mode")
        .arg("offline")
        .arg("--artifact-dir")
        .arg(&artifact_dir)
        .arg("--harness-bin")
        .arg(env!("CARGO_BIN_EXE_harness"))
        .output()
        .expect("run stress harness script");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let summary = fs::read_to_string(artifact_dir.join("summary.txt")).expect("read summary");
    assert!(summary.contains("config_validate PASS"));
    assert!(summary.contains("prompt_mock_smoke PASS"));
    assert!(summary.contains("run_golden_path PASS"));
    assert!(summary.contains("run_golden_path_interactive PASS"));

    let prompt_events = fs::read_to_string(
        artifact_dir
            .join("stages")
            .join("prompt_mock_smoke")
            .join("events.jsonl"),
    )
    .expect("read prompt events");
    assert!(prompt_events.contains("\"event_type\":\"run_finished\""));

    let run_events = fs::read_to_string(
        artifact_dir
            .join("stages")
            .join("run_golden_path")
            .join("events.jsonl"),
    )
    .expect("read run events");
    assert!(run_events.contains("\"event_type\":\"tool_call_finished\""));
    assert!(run_events.contains("\"status\":\"succeeded\""));
}

#[test]
fn stress_harness_script_reports_missing_option_values_cleanly() {
    let repo_root = repo_root();
    let script_path = repo_root.join("scripts/stress-harness.sh");

    let output = Command::new("bash")
        .current_dir(&repo_root)
        .arg(script_path)
        .arg("--mode")
        .output()
        .expect("run stress harness script with missing mode value");

    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("Missing value for --mode"),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn stress_harness_script_accepts_relative_artifact_dir_with_missing_parent() {
    let repo_root = repo_root();
    let script_path = repo_root.join("scripts/stress-harness.sh");
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after unix epoch")
        .as_nanos();
    let relative_artifact_dir =
        format!("target/harness-stress/relative-artifact-dir-{unique}/artifacts");
    let absolute_artifact_dir = repo_root.join(&relative_artifact_dir);
    if absolute_artifact_dir.exists() {
        fs::remove_dir_all(&absolute_artifact_dir).expect("remove pre-existing artifact dir");
    }

    let output = Command::new("bash")
        .current_dir(repo_root)
        .arg(script_path)
        .arg("--mode")
        .arg("offline")
        .arg("--artifact-dir")
        .arg(&relative_artifact_dir)
        .arg("--harness-bin")
        .arg(env!("CARGO_BIN_EXE_harness"))
        .output()
        .expect("run stress harness script with relative artifact dir");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(absolute_artifact_dir.join("summary.txt").exists());

    fs::remove_dir_all(
        absolute_artifact_dir
            .parent()
            .expect("artifact dir should have a parent"),
    )
    .expect("clean up artifact dir tree");
}
