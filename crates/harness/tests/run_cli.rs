use std::io::Write;
use std::process::{Command, Stdio};

use tempfile::tempdir;

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
