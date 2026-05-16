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

#[test]
fn workflow_status_json_is_projection_only() {
    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");

    let run_output = harness_command()
        .current_dir(repo_root())
        .args([
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "workflow",
            "run",
            "--workflow-id",
            "wf_status_projection",
            "--title",
            "Projection only status",
            "--json",
        ])
        .output()
        .expect("run workflow run");

    assert!(
        run_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run_output.stdout),
        String::from_utf8_lossy(&run_output.stderr)
    );
    let run_report: Value = serde_json::from_slice(&run_output.stdout).expect("run json");
    let run_dir = run_report["run_dir"].as_str().expect("run dir");
    let events_path = run_report["events_path"].as_str().expect("events path");
    let before = fs::read_to_string(events_path).expect("read events before status");

    let status_output = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "status",
            "--run-dir",
            run_dir,
            "--workflow-id",
            "wf_status_projection",
            "--json",
        ])
        .output()
        .expect("run workflow status");

    assert!(
        status_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&status_output.stdout),
        String::from_utf8_lossy(&status_output.stderr)
    );
    let after = fs::read_to_string(events_path).expect("read events after status");
    assert_eq!(before, after, "workflow status must not append events");

    let status: Value = serde_json::from_slice(&status_output.stdout).expect("status json");
    assert_eq!(status["workflow_count"], 1);
    assert_eq!(status["active_count"], 1);
    assert_eq!(
        status["projection"]["workflows"]["wf_status_projection"]["status"],
        "active"
    );
}

#[test]
fn workflow_dossier_json_reports_signoff_gate_without_appending_events() {
    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");

    let run_output = harness_command()
        .current_dir(repo_root())
        .args([
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "workflow",
            "run",
            "--workflow-id",
            "wf_dossier_signoff",
            "--title",
            "Dossier signoff gate",
            "--json",
        ])
        .output()
        .expect("run workflow run");

    assert!(
        run_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run_output.stdout),
        String::from_utf8_lossy(&run_output.stderr)
    );
    let run_report: Value = serde_json::from_slice(&run_output.stdout).expect("run json");
    let run_dir = run_report["run_dir"].as_str().expect("run dir");
    let events_path = run_report["events_path"].as_str().expect("events path");
    let before = fs::read_to_string(events_path).expect("read events before dossier");

    let dossier_output = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "dossier",
            "export",
            "--run-dir",
            run_dir,
            "--workflow-id",
            "wf_dossier_signoff",
            "--format",
            "json",
        ])
        .output()
        .expect("run workflow dossier export");

    assert!(
        dossier_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&dossier_output.stdout),
        String::from_utf8_lossy(&dossier_output.stderr)
    );
    let after = fs::read_to_string(events_path).expect("read events after dossier");
    assert_eq!(
        before, after,
        "workflow dossier export must not append events"
    );

    let dossier: Value = serde_json::from_slice(&dossier_output.stdout).expect("dossier json");
    let workflow = &dossier["workflows"][0];
    assert_eq!(workflow["workflow_id"], "wf_dossier_signoff");
    assert_eq!(workflow["signoff"]["allowed"], false);
    assert_eq!(workflow["quality_gate"]["passed"], false);
    assert!(workflow["quality_gate"]["missing"]
        .as_array()
        .expect("quality gate missing array")
        .iter()
        .any(|gate| gate.as_str() == Some("prompt_to_artifact_audit")));
    assert!(workflow["signoff"]["missing_evidence_categories"]
        .as_array()
        .expect("missing evidence array")
        .iter()
        .any(|category| category.as_str() == Some("evidence.context_snapshot")));
    assert!(workflow["signoff"]["missing_evidence_categories"]
        .as_array()
        .expect("missing evidence array")
        .iter()
        .any(|category| category.as_str() == Some("evidence.simulated_tool_result")));
}

#[test]
fn workflow_signoff_audit_run_projects_terminal_decision() {
    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");

    let signoff_output = harness_command()
        .current_dir(repo_root())
        .args([
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "workflow",
            "signoff",
            "--workflow-id",
            "wf_signoff_audit",
            "--approve",
            "--json",
        ])
        .output()
        .expect("run workflow signoff");

    assert!(
        signoff_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&signoff_output.stdout),
        String::from_utf8_lossy(&signoff_output.stderr)
    );
    let signoff: Value = serde_json::from_slice(&signoff_output.stdout).expect("signoff json");
    let run_dir = signoff["run_dir"].as_str().expect("run dir");

    let status_output = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "status",
            "--run-dir",
            run_dir,
            "--workflow-id",
            "wf_signoff_audit",
            "--json",
        ])
        .output()
        .expect("run workflow status");

    assert!(
        status_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&status_output.stdout),
        String::from_utf8_lossy(&status_output.stderr)
    );
    let status: Value = serde_json::from_slice(&status_output.stdout).expect("status json");
    let workflow = &status["projection"]["workflows"]["wf_signoff_audit"];
    assert_eq!(status["workflow_count"], 1);
    assert_eq!(workflow["status"], "outcome.finished");
    assert_eq!(workflow["terminal"], true);
    assert_eq!(workflow["operator_decisions"][0], "signoff-approved");
}

#[test]
fn workflow_init_check_writes_nothing_and_apply_is_explicit() {
    let temp = tempdir().expect("tempdir");
    let workflow_dir = temp.path().join(".agent-harness").join("workflows");

    let check_output = harness_command()
        .current_dir(temp.path())
        .args(["workflow", "init", "--check", "--json"])
        .output()
        .expect("run workflow init --check");

    assert!(
        check_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&check_output.stdout),
        String::from_utf8_lossy(&check_output.stderr)
    );
    assert!(
        !workflow_dir.exists(),
        "workflow init --check must not create project files"
    );
    let check_report: Value = serde_json::from_slice(&check_output.stdout).expect("check json");
    assert_eq!(check_report["mode"], "check");
    assert_eq!(check_report["files"][0]["action"], "would_create");

    let apply_output = harness_command()
        .current_dir(temp.path())
        .args(["workflow", "init", "--apply", "--json"])
        .output()
        .expect("run workflow init --apply");

    assert!(
        apply_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&apply_output.stdout),
        String::from_utf8_lossy(&apply_output.stderr)
    );
    assert!(
        workflow_dir.join("README.md").is_file(),
        "workflow init --apply should create the safe bootstrap file"
    );
    let apply_report: Value = serde_json::from_slice(&apply_output.stdout).expect("apply json");
    assert_eq!(apply_report["mode"], "apply");
    assert_eq!(apply_report["files"][0]["action"], "created");
}
