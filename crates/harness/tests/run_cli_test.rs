use harness::UnwrapOrAbort;
use std::fs;
use std::path::Path;

use tempfile::tempdir;

mod common;

use common::CliHarness;
use harness_testkit::workspace::TestWorkspace;

fn run_cli_config(session_dir: &Path) -> String {
    serde_json::json!({
        "provider": {
            "default": {
                "type": "openai_compatible",
                "options": {
                    "baseURL": "http://127.0.0.1:9999/v1",
                    "apiKey": "DUMMY",
                    "apiMode": "responses",
                    "timeoutMs": 60000
                },
                "models": {
                    "gpt-5.6-luna": {
                        "name": "GPT 5.6 Luna",
                        "metadata": {"supportsReasoningSummaries": true},
                        "variants": {
                            "max": {"metadata": {"reasoningEffort": "max"}}
                        }
                    }
                }
            }
        },
        "model": "default/gpt-5.6-luna",
        "agent": {
            "default": {
                "tools": [],
                "variant": "max"
            }
        },
        "permission": {
            "*": "allow",
            "shell_allowlist": {
                "executables": ["bash", "git", "cargo", "ls", "rg"],
                "cwd_roots": ["."]
            }
        },
        "runtime": {
            "background_tasks": {
                "default_concurrency": 2,
                "provider_concurrency": 2,
                "model_concurrency": 2,
                "stale_timeout_ms": 30000,
                "message_staleness_timeout_ms": 10000
            },
            "session_dir": session_dir,
            "deterministic": {
                "enabled": false,
                "seed": 42
            }
        }
    })
    .to_string()
}

#[test]
fn run_cli_writes_out_file_and_prints_run_dir() {
    // arrange
    // act
    // assert
    let workspace = TestWorkspace::new().unwrap_or_abort();
    let out_path = workspace.path("events/out.jsonl");
    let session_dir = workspace.sessions_dir();

    let output = CliHarness::new()
        .test_workspace(workspace)
        .args([
            "run",
            "--scenario",
            "golden_path",
            "--deterministic",
            "--session-dir",
            session_dir.to_str().unwrap_or_abort(),
            "--out",
            out_path.to_str().unwrap_or_abort(),
            "--print-run-dir",
        ])
        .output();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let run_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert!(
        run_dir.contains("run_"),
        "expected run dir in stdout, got {run_dir}"
    );
    assert!(out_path.exists(), "expected --out file to be written");
    assert!(
        std::fs::read_to_string(&out_path)
            .unwrap_or_abort()
            .contains("run_finished"),
        "expected copied events jsonl to include run_finished"
    );

    let captured_run = output.single_run();
    assert_eq!(
        captured_run.events_path,
        Path::new(&run_dir).join("events.jsonl")
    );
    assert!(
        captured_run.events.contains("run_finished"),
        "expected captured event log to include run_finished"
    );
    assert!(
        captured_run
            .artifacts
            .iter()
            .any(|artifact| artifact.relative_path.starts_with("artifacts")),
        "expected CliHarness to capture run artifacts under {}",
        captured_run.run_dir.display()
    );
}

#[test]
fn run_cli_golden_path_completes_provider_turn_with_configured_model_settings() {
    // arrange
    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.scenario.jsonc");
    let session_dir = temp.path().join("sessions");
    fs::write(&config_path, run_cli_config(&session_dir)).unwrap_or_abort();

    // act
    let output = CliHarness::new()
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "run",
            "--scenario",
            "golden_path",
            "--deterministic",
            "--session-dir",
            session_dir.to_str().unwrap_or_abort(),
        ])
        .capture_session_dir(&session_dir)
        .output();

    // assert
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let captured = output.single_run();
    let parsed = captured
        .events
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap_or_abort())
        .collect::<Vec<_>>();
    assert!(parsed.iter().any(|event| {
        event["payload"]["event_type"] == "provider_request_started"
            && event["payload"]["data"]["metadata"]["context_budget"].is_object()
    }));
    assert!(parsed.iter().any(|event| {
        event["payload"]["event_type"] == "provider_request_finished"
            && event["payload"]["data"]["finish_reason"] == "done"
    }));
    assert!(!parsed.iter().any(|event| {
        event["payload"]["event_type"] == "provider_request_finished"
            && event["payload"]["data"]["finish_reason"] == "error"
    }));
    assert!(!parsed
        .iter()
        .any(|event| event["payload"]["event_type"] == "task_cancelled"));
}

#[test]
fn run_cli_interactive_permissions_accepts_allow_on_in_memory_stdin() {
    // arrange
    // act
    // assert
    let temp = tempdir().unwrap_or_abort();
    let session_dir = temp.path().join("sessions");

    let output = CliHarness::new()
        .args([
            "run",
            "--scenario",
            "golden_path_interactive",
            "--deterministic",
            "--session-dir",
            session_dir.to_str().unwrap_or_abort(),
        ])
        .capture_session_dir(&session_dir)
        .stdin(b"allow\n".to_vec())
        .output();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("permission requested:"));
    assert!(stdout.contains("scenario golden_path_interactive complete:"));
    assert!(output.single_run().events.contains("run_finished"));
}

#[test]
fn run_cli_creates_durable_run_logs_under_run_dir() {
    // arrange
    // act
    // assert
    let temp = tempdir().unwrap_or_abort();
    let config_path = temp.path().join("harness.logging.jsonc");
    let session_dir = temp.path().join("sessions");

    fs::write(&config_path, run_cli_config(&session_dir)).unwrap_or_abort();

    let output = CliHarness::new()
        .args([
            "--config",
            config_path.to_str().unwrap_or_abort(),
            "run",
            "--scenario",
            "golden_path",
            "--deterministic",
            "--session-dir",
            session_dir.to_str().unwrap_or_abort(),
            "--print-run-dir",
        ])
        .output();

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let run_dir = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let log_path = Path::new(&run_dir).join("logs").join("harness.log");
    assert!(
        log_path.exists(),
        "expected log file at {}",
        log_path.display()
    );

    let log_body = fs::read_to_string(&log_path).unwrap_or_abort();
    assert!(
        log_body.contains("initialized harness file logging"),
        "expected logging init marker in {}\n{}",
        log_path.display(),
        log_body
    );
}
