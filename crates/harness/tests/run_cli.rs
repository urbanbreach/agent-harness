use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use tempfile::tempdir;

fn run_cli_config(session_dir: &Path) -> String {
    serde_json::json!({
        "providers": {
            "default": {
                "type": "openai_compatible",
                "base_url": "http://127.0.0.1:9999/v1",
                "api_key": "DUMMY",
                "api_mode": "responses",
                "timeout_ms": 60000,
                "models": {
                    "gpt-4o-mini": {
                        "display_name": "GPT-4o mini"
                    }
                }
            }
        },
        "agents": {
            "deep": {
                "description": "Deep profile",
                "model_ref": "default:gpt-4o-mini",
                "tools": []
            }
        },
        "permissions": {
            "defaults": {
                "edit": "allow",
                "shell": "allow",
                "network": "allow"
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
        },
        "integrations": {
            "remote_search": {
                "endpoint": "https://mcp.exa.ai/mcp"
            }
        },
        "ui": {
            "default_profile": "deep"
        }
    })
    .to_string()
}

#[test]
fn run_cli_writes_out_file_and_prints_run_dir() {
    let temp = tempdir().expect("tempdir");
    let out_path = temp.path().join("events/out.jsonl");
    let session_dir = temp.path().join("sessions");

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "run",
            "--scenario",
            "golden_path",
            "--deterministic",
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "--out",
            out_path.to_str().expect("out path utf-8"),
            "--print-run-dir",
        ])
        .output()
        .expect("run harness run command");

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
            .expect("read out file")
            .contains("run_finished"),
        "expected copied events jsonl to include run_finished"
    );
}

#[test]
fn run_cli_interactive_permissions_accepts_allow_on_stdin() {
    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");

    let mut child = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "run",
            "--scenario",
            "golden_path_interactive",
            "--deterministic",
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn interactive run command");

    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(b"allow\n")
        .expect("write allow decision");

    let output = child.wait_with_output().expect("wait for interactive run");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("permission requested:"));
    assert!(stdout.contains("scenario golden_path_interactive complete:"));
}

#[test]
fn run_cli_creates_durable_run_logs_under_run_dir() {
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.logging.jsonc");
    let session_dir = temp.path().join("sessions");

    fs::write(&config_path, run_cli_config(&session_dir)).expect("write config");

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            config_path.to_str().expect("config path utf-8"),
            "run",
            "--scenario",
            "golden_path",
            "--deterministic",
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "--print-run-dir",
        ])
        .output()
        .expect("run harness run command with logging");

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

    let log_body = fs::read_to_string(&log_path).expect("read harness log file");
    assert!(
        log_body.contains("initialized harness file logging"),
        "expected logging init marker in {}\n{}",
        log_path.display(),
        log_body
    );
}
