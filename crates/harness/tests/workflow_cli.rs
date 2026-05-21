use std::env;
use std::fs;
use std::path::{Path, PathBuf};
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

fn run_json_command<const N: usize>(current_dir: &Path, args: &[&str; N], context: &str) -> Value {
    let output = harness_command()
        .current_dir(current_dir)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("failed to run {context}: {err}"));
    assert!(
        output.status.success(),
        "{context} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|err| {
        panic!(
            "{context} stdout should be JSON: {err}\nstdout:\n{}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn run_command_expect_failure<const N: usize>(
    current_dir: &Path,
    args: &[&str; N],
    context: &str,
) -> std::process::Output {
    let output = harness_command()
        .current_dir(current_dir)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("failed to run {context}: {err}"));
    assert!(
        !output.status.success(),
        "{context} should fail\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn closeout_dimension<'a>(closeout: &'a Value, dimension_id: &str) -> &'a Value {
    closeout["dimensions"]
        .as_array()
        .expect("closeout dimensions")
        .iter()
        .find(|dimension| dimension["id"] == dimension_id)
        .unwrap_or_else(|| panic!("missing closeout dimension `{dimension_id}`"))
}

fn json_array_contains_str(value: &Value, expected: &str) -> bool {
    value
        .as_array()
        .expect("JSON array")
        .iter()
        .any(|item| item.as_str() == Some(expected))
}

fn deny_edit_config_content() -> &'static str {
    r#"{
      provider: {
        default: {
          type: "openai_compatible",
          options: {
            baseURL: "http://127.0.0.1:8317/v1",
            apiKey: "test-key",
          },
          models: {
            "gpt-4o-mini": { name: "GPT-4o mini" },
          },
        },
      },
      model: "default/gpt-4o-mini",
      agent: {
        operator: { system_prompt: "Operate workflows" },
      },
      default_agent: "operator",
      permission: { edit: "deny" },
    }"#
}

fn latest_run_dir(session_dir: &Path) -> PathBuf {
    let mut runs = fs::read_dir(session_dir)
        .expect("read session dir")
        .map(|entry| entry.expect("session entry").path())
        .filter(|path| path.join("events.jsonl").is_file())
        .collect::<Vec<_>>();
    runs.sort();
    runs.pop().expect("latest run dir")
}

fn assert_permission_denial_projection(
    run_dir: &Path,
    workflow_id: &str,
    decision: &str,
    selector: &str,
) {
    let status_output = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "status",
            "--run-dir",
            run_dir.to_str().expect("run dir utf-8"),
            "--workflow-id",
            workflow_id,
            "--json",
        ])
        .output()
        .expect("run workflow status for permission denial");
    assert!(
        status_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&status_output.stdout),
        String::from_utf8_lossy(&status_output.stderr)
    );
    let status: Value =
        serde_json::from_slice(&status_output.stdout).expect("permission denial status JSON");
    let workflow = &status["projection"]["workflows"][workflow_id];
    assert_eq!(workflow["terminal"], false);
    assert!(workflow["operator_decisions"]
        .as_array()
        .expect("operator decisions")
        .iter()
        .any(|value| value.as_str() == Some(decision)));
    assert!(workflow["evidence_categories"]
        .as_array()
        .expect("evidence categories")
        .iter()
        .any(|value| value.as_str() == Some("evidence.permission_decision")));
    let evidence = &status["projection"]["evidence"][workflow_id][0];
    assert_eq!(evidence["metadata"]["status"], "denied");
    assert_eq!(evidence["metadata"]["selector"], selector);
    assert_eq!(
        status["closeout"][workflow_id]["closeout"]["overall_allowed"],
        false
    );
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
    let closeout = &status["closeout"]["wf_status_projection"]["closeout"];
    assert_eq!(closeout["policy_id"], "workflow.closeout.default");
    assert_eq!(closeout["schema_version"], 1);
    assert_eq!(closeout["overall_allowed"], false);
    assert!(closeout["legal_next_actions"]
        .as_array()
        .expect("legal next actions")
        .iter()
        .any(|action| action["action"] == "request_evidence"));
    assert!(closeout["dimensions"]
        .as_array()
        .expect("closeout dimensions")
        .iter()
        .any(|dimension| dimension["id"] == "evidence"));
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
    assert_eq!(
        workflow["closeout"]["policy_id"],
        "workflow.closeout.default"
    );
    assert_eq!(workflow["closeout"]["schema_version"], 1);
    assert_eq!(workflow["closeout"]["overall_allowed"], false);
    assert_eq!(workflow["closeout"]["stale_export"], false);
    assert!(workflow["closeout"]["matrix"]
        .as_array()
        .expect("closeout matrix")
        .iter()
        .any(|dimension| dimension["id"] == "evidence"));
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
fn workflow_omx_closeout_oracle_drives_plan_dossier_signoff_and_replay() {
    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");
    let session_dir_arg = session_dir.to_str().expect("session dir utf-8");
    let workflow_id = "wf_omx_closeout_oracle";
    let plan_id = "plan_omx_closeout_oracle";

    let plan_report = run_json_command(
        &repo_root(),
        &[
            "--session-dir",
            session_dir_arg,
            "workflow",
            "plan-consensus",
            "--workflow-id",
            workflow_id,
            "--plan-id",
            plan_id,
            "--task",
            "Close out an OMX-style workflow with replay-derived evidence",
            "--option",
            "ship=Ship the deterministic closeout oracle",
            "--chosen-option",
            "ship",
            "--adr",
            "Use replay-derived workflow status and dossier as the closeout authority.",
            "--work",
            "Add a black-box workflow closeout oracle",
            "--risk",
            "Closeout projections can drift from persisted events",
            "--test-plan",
            "cargo test -p harness --test workflow_cli workflow_omx_closeout_oracle_drives_plan_dossier_signoff_and_replay",
            "--manual-qa",
            "Run workflow status, dossier export, signoff, and replay through the CLI",
            "--staffing",
            "planner/executor/reviewer",
            "--handoff",
            "workflow signoff with explicit closeout waiver and approval",
            "--acceptance",
            "status, dossier, signoff, and replay agree on terminal closeout",
            "--evidence-ref",
            "context:intake-snapshot",
            "--json",
        ],
        "workflow plan-consensus closeout oracle",
    );
    let run_dir = plan_report["run_dir"].as_str().expect("run dir");
    let events_path = plan_report["events_path"].as_str().expect("events path");
    let plan_artifact_path = plan_report["artifact_path"]
        .as_str()
        .expect("artifact path");
    assert!(plan_artifact_path.starts_with("artifacts/workflows/plan_consensus/"));
    let plan_artifact_body = fs::read_to_string(Path::new(run_dir).join(plan_artifact_path))
        .expect("read plan consensus artifact");
    assert!(plan_artifact_body.contains("deterministic closeout oracle"));
    assert!(plan_artifact_body.contains("replay-derived workflow status"));

    let initial_events = fs::read_to_string(events_path).expect("read initial workflow events");
    assert!(initial_events.contains("workflow_started"));
    assert!(initial_events.contains("evidence.plan_consensus"));
    assert!(initial_events.contains(plan_artifact_path));

    let status_before = run_json_command(
        &repo_root(),
        &[
            "workflow",
            "status",
            "--run-dir",
            run_dir,
            "--workflow-id",
            workflow_id,
            "--json",
        ],
        "workflow status before closeout",
    );
    assert_eq!(
        fs::read_to_string(events_path).expect("read events after status"),
        initial_events,
        "workflow status must be projection-only"
    );
    let workflow_before = &status_before["projection"]["workflows"][workflow_id];
    assert_eq!(workflow_before["mode"], "workflow.plan_consensus");
    assert_eq!(workflow_before["status"], "active");
    assert_eq!(workflow_before["terminal"], false);
    assert!(json_array_contains_str(
        &workflow_before["evidence_categories"],
        "evidence.plan_consensus"
    ));
    assert_eq!(
        status_before["projection"]["plan_consensus"][plan_id]["status"],
        "approved"
    );
    let closeout_before = &status_before["closeout"][workflow_id]["closeout"];
    assert_eq!(closeout_before["overall_allowed"], false);
    assert!(closeout_before["legal_next_actions"]
        .as_array()
        .expect("legal next actions")
        .iter()
        .any(|action| action["action"] == "request_evidence"));
    let evidence_before = closeout_dimension(closeout_before, "evidence");
    assert_eq!(evidence_before["allowed"], false);
    assert!(json_array_contains_str(
        &evidence_before["missing_categories"],
        "evidence.context_snapshot"
    ));
    assert!(json_array_contains_str(
        &evidence_before["missing_categories"],
        "evidence.simulated_tool_result"
    ));
    let plan_dimension = closeout_dimension(closeout_before, "plan");
    assert_eq!(plan_dimension["allowed"], true);

    let dossier_output = temp.path().join("omx-closeout-dossier.json");
    let dossier_output_arg = dossier_output.to_str().expect("dossier path utf-8");
    let dossier_before = run_json_command(
        &repo_root(),
        &[
            "workflow",
            "dossier",
            "export",
            "--run-dir",
            run_dir,
            "--workflow-id",
            workflow_id,
            "--format",
            "json",
            "--output",
            dossier_output_arg,
        ],
        "workflow dossier export before closeout",
    );
    assert_eq!(
        fs::read_to_string(events_path).expect("read events after dossier"),
        initial_events,
        "workflow dossier export must be projection-only"
    );
    assert!(
        dossier_output.is_file(),
        "dossier export should write the requested file"
    );
    let exported_dossier: Value =
        serde_json::from_str(&fs::read_to_string(&dossier_output).expect("read exported dossier"))
            .expect("exported dossier JSON");
    assert_eq!(dossier_before, exported_dossier);
    let dossier_workflow = &dossier_before["workflows"][0];
    assert_eq!(dossier_workflow["workflow_id"], workflow_id);
    assert_eq!(dossier_workflow["signoff"]["allowed"], false);
    assert_eq!(dossier_workflow["closeout"]["overall_allowed"], false);
    assert!(dossier_workflow["evidence"]
        .as_array()
        .expect("dossier evidence")
        .iter()
        .any(|evidence| evidence["category"] == "evidence.plan_consensus"
            && evidence["artifact_path"] == plan_artifact_path));

    let premature = run_command_expect_failure(
        &repo_root(),
        &[
            "workflow",
            "signoff",
            "--run-dir",
            run_dir,
            "--workflow-id",
            workflow_id,
            "--approve",
            "--json",
        ],
        "premature workflow closeout approval",
    );
    assert!(String::from_utf8_lossy(&premature.stderr).contains("closeout"));
    let after_premature = fs::read_to_string(events_path).expect("read events after denial");
    assert!(after_premature.contains("workflow_transition_denied"));

    let waiver = run_json_command(
        &repo_root(),
        &[
            "workflow",
            "signoff",
            "--run-dir",
            run_dir,
            "--workflow-id",
            workflow_id,
            "--waive",
            "--scope",
            "dimension:evidence",
            "--reason",
            "operator accepts deterministic oracle evidence for this closeout slice",
            "--json",
        ],
        "workflow closeout evidence waiver",
    );
    assert_eq!(waiver["run_dir"], run_dir);
    assert_eq!(waiver["decision"], "waive:dimension:evidence");
    assert_eq!(waiver["terminal_outcome"], Value::Null);
    assert_eq!(waiver["signoff"]["closeout"]["overall_allowed"], true);

    let status_after_waiver = run_json_command(
        &repo_root(),
        &[
            "workflow",
            "status",
            "--run-dir",
            run_dir,
            "--workflow-id",
            workflow_id,
            "--json",
        ],
        "workflow status after evidence waiver",
    );
    let closeout_after_waiver = &status_after_waiver["closeout"][workflow_id]["closeout"];
    assert_eq!(closeout_after_waiver["overall_allowed"], true);
    let waived_evidence = closeout_dimension(closeout_after_waiver, "evidence");
    assert_eq!(waived_evidence["allowed"], true);
    assert_eq!(waived_evidence["waived"], true);
    assert!(closeout_after_waiver["legal_next_actions"]
        .as_array()
        .expect("legal next actions after waiver")
        .iter()
        .any(|action| action["action"] == "approve"));

    let approval = run_json_command(
        &repo_root(),
        &[
            "workflow",
            "signoff",
            "--run-dir",
            run_dir,
            "--workflow-id",
            workflow_id,
            "--approve",
            "--json",
        ],
        "workflow closeout approval after waiver",
    );
    assert_eq!(approval["run_dir"], run_dir);
    assert_eq!(approval["decision"], "signoff-approved");
    assert_eq!(approval["terminal_outcome"], "outcome.finished");
    assert_eq!(approval["signoff"]["closeout"]["overall_allowed"], true);

    let final_status = run_json_command(
        &repo_root(),
        &[
            "workflow",
            "status",
            "--run-dir",
            run_dir,
            "--workflow-id",
            workflow_id,
            "--json",
        ],
        "workflow final status after approval",
    );
    assert_eq!(final_status["active_count"], 0);
    let final_workflow = &final_status["projection"]["workflows"][workflow_id];
    assert_eq!(final_workflow["status"], "outcome.finished");
    assert_eq!(final_workflow["terminal"], true);
    assert!(json_array_contains_str(
        &final_workflow["operator_decisions"],
        "waive:dimension:evidence"
    ));
    assert!(json_array_contains_str(
        &final_workflow["operator_decisions"],
        "signoff-approved"
    ));
    assert_eq!(
        final_status["closeout"][workflow_id]["closeout"]["overall_allowed"],
        true
    );

    let before_replay = fs::read_to_string(events_path).expect("read events before replay");
    let replay = run_json_command(
        &repo_root(),
        &["replay", "--session", run_dir, "--json"],
        "workflow closeout replay JSON",
    );
    assert_eq!(
        fs::read_to_string(events_path).expect("read events after replay"),
        before_replay,
        "workflow replay must not append closeout/status/dossier events"
    );
    assert_eq!(
        replay["workflow_projection"]["workflows"][workflow_id]["status"],
        "outcome.finished"
    );
    assert_eq!(
        replay["workflow_projection"]["workflows"][workflow_id]["terminal"],
        true
    );
    assert_eq!(
        replay["workflow_projection"]["plan_consensus"][plan_id]["status"],
        "approved"
    );
    assert_eq!(
        replay["workflow_projection"]["evidence"][workflow_id][0]["artifact_path"],
        plan_artifact_path
    );
    assert_eq!(
        replay["workflow_closeout"][workflow_id]["overall_allowed"],
        true
    );
    assert_eq!(
        closeout_dimension(&replay["workflow_closeout"][workflow_id], "evidence")["waived"],
        true
    );
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
            "--audit-only",
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
    assert_eq!(signoff["audit_only"], true);

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
fn workflow_signoff_default_targets_existing_run_and_blocks_premature_approval() {
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
            "wf_signoff_target",
            "--title",
            "Target signoff",
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

    let signoff_output = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "signoff",
            "--run-dir",
            run_dir,
            "--workflow-id",
            "wf_signoff_target",
            "--approve",
            "--json",
        ])
        .output()
        .expect("run workflow signoff");
    assert!(
        !signoff_output.status.success(),
        "premature target approval should fail stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&signoff_output.stdout),
        String::from_utf8_lossy(&signoff_output.stderr)
    );

    let status_output = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "status",
            "--run-dir",
            run_dir,
            "--workflow-id",
            "wf_signoff_target",
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
    let workflow = &status["projection"]["workflows"]["wf_signoff_target"];
    assert_eq!(workflow["terminal"], false);
    assert_eq!(workflow["status"], "active");
    assert_eq!(workflow["denied_transition_count"], 1);
}

#[test]
fn workflow_signoff_waiver_requires_reason_and_scope_without_mutating_target() {
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
            "wf_waiver_guard",
            "--title",
            "Waiver guard",
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
    let before = fs::read_to_string(events_path).expect("read events before invalid waiver");

    let missing_reason = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "signoff",
            "--run-dir",
            run_dir,
            "--workflow-id",
            "wf_waiver_guard",
            "--waive",
            "--scope",
            "dimension:evidence",
            "--json",
        ])
        .output()
        .expect("run waiver without reason");
    assert!(!missing_reason.status.success());
    assert!(String::from_utf8_lossy(&missing_reason.stderr).contains("`--reason` is required"));

    let missing_scope = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "signoff",
            "--run-dir",
            run_dir,
            "--workflow-id",
            "wf_waiver_guard",
            "--waive",
            "--reason",
            "operator accepts missing evidence for this dry run",
            "--json",
        ])
        .output()
        .expect("run waiver without scope");
    assert!(!missing_scope.status.success());
    assert!(String::from_utf8_lossy(&missing_scope.stderr).contains("`--scope` is required"));

    let after = fs::read_to_string(events_path).expect("read events after invalid waiver");
    assert_eq!(
        before, after,
        "invalid waiver attempts must not append target workflow events"
    );
}

#[test]
fn workflow_signoff_valid_waiver_allows_later_target_approval() {
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
            "wf_waiver_approval",
            "--title",
            "Waiver approval",
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

    let waiver_output = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "signoff",
            "--run-dir",
            run_dir,
            "--workflow-id",
            "wf_waiver_approval",
            "--waive",
            "--scope",
            "dimension:evidence",
            "--reason",
            "operator accepts missing dry-run evidence",
            "--json",
        ])
        .output()
        .expect("record valid waiver");
    assert!(
        waiver_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&waiver_output.stdout),
        String::from_utf8_lossy(&waiver_output.stderr)
    );

    let approve_output = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "signoff",
            "--run-dir",
            run_dir,
            "--workflow-id",
            "wf_waiver_approval",
            "--approve",
            "--json",
        ])
        .output()
        .expect("approve after valid waiver");
    assert!(
        approve_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&approve_output.stdout),
        String::from_utf8_lossy(&approve_output.stderr)
    );
    let approval: Value = serde_json::from_slice(&approve_output.stdout).expect("approval json");
    assert_eq!(approval["terminal_outcome"], "outcome.finished");

    let status_output = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "status",
            "--run-dir",
            run_dir,
            "--workflow-id",
            "wf_waiver_approval",
            "--json",
        ])
        .output()
        .expect("status after waiver approval");
    assert!(
        status_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&status_output.stdout),
        String::from_utf8_lossy(&status_output.stderr)
    );
    let status: Value = serde_json::from_slice(&status_output.stdout).expect("status json");
    let workflow = &status["projection"]["workflows"]["wf_waiver_approval"];
    assert_eq!(workflow["terminal"], true);
    assert_eq!(workflow["status"], "outcome.finished");
    let evidence_dimension = status["closeout"]["wf_waiver_approval"]["closeout"]["dimensions"]
        .as_array()
        .expect("dimensions")
        .iter()
        .find(|dimension| dimension["id"] == "evidence")
        .expect("evidence dimension");
    assert_eq!(evidence_dimension["allowed"], true);
    assert_eq!(evidence_dimension["waived"], true);
}

#[test]
fn workflow_signoff_audit_only_ignores_target_run_and_leaves_it_open() {
    let temp = tempdir().expect("tempdir");
    let target_session_dir = temp.path().join("target-sessions");
    let audit_session_dir = temp.path().join("audit-sessions");

    let run_output = harness_command()
        .current_dir(repo_root())
        .args([
            "--session-dir",
            target_session_dir
                .to_str()
                .expect("target session dir utf-8"),
            "workflow",
            "run",
            "--workflow-id",
            "wf_audit_target",
            "--title",
            "Audit target",
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
    let target_run_dir = run_report["run_dir"].as_str().expect("target run dir");
    let target_events_path = run_report["events_path"]
        .as_str()
        .expect("target events path");
    let before = fs::read_to_string(target_events_path).expect("read target events");

    let audit_output = harness_command()
        .current_dir(repo_root())
        .args([
            "--session-dir",
            audit_session_dir.to_str().expect("audit session dir utf-8"),
            "workflow",
            "signoff",
            "--run-dir",
            target_run_dir,
            "--workflow-id",
            "wf_audit_target",
            "--approve",
            "--audit-only",
            "--json",
        ])
        .output()
        .expect("run audit-only signoff");
    assert!(
        audit_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&audit_output.stdout),
        String::from_utf8_lossy(&audit_output.stderr)
    );
    let audit: Value = serde_json::from_slice(&audit_output.stdout).expect("audit json");
    assert_eq!(audit["audit_only"], true);
    assert_ne!(audit["run_dir"], target_run_dir);
    let after = fs::read_to_string(target_events_path).expect("read target events after audit");
    assert_eq!(
        before, after,
        "audit-only signoff must not mutate the target run event log"
    );

    let status_output = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "status",
            "--run-dir",
            target_run_dir,
            "--workflow-id",
            "wf_audit_target",
            "--json",
        ])
        .output()
        .expect("run target status");
    assert!(status_output.status.success());
    let status: Value = serde_json::from_slice(&status_output.stdout).expect("status json");
    let workflow = &status["projection"]["workflows"]["wf_audit_target"];
    assert_eq!(workflow["terminal"], false);
    assert_eq!(workflow["status"], "active");
}

#[test]
fn workflow_replay_summary_is_read_only_after_denied_signoff() {
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
            "wf_replay_guard",
            "--title",
            "Replay guard",
            "--json",
        ])
        .output()
        .expect("run workflow run");
    assert!(run_output.status.success());
    let run_report: Value = serde_json::from_slice(&run_output.stdout).expect("run json");
    let run_dir = run_report["run_dir"].as_str().expect("run dir");
    let events_path = run_report["events_path"].as_str().expect("events path");

    let denied = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "signoff",
            "--run-dir",
            run_dir,
            "--workflow-id",
            "wf_replay_guard",
            "--approve",
            "--json",
        ])
        .output()
        .expect("run denied signoff");
    assert!(!denied.status.success());

    let before = fs::read_to_string(events_path).expect("read events before replay");
    let replay_output = harness_command()
        .current_dir(repo_root())
        .args(["replay", "--session", run_dir, "--json"])
        .output()
        .expect("run replay summary");
    assert!(
        replay_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&replay_output.stdout),
        String::from_utf8_lossy(&replay_output.stderr)
    );
    let after = fs::read_to_string(events_path).expect("read events after replay");
    assert_eq!(
        before, after,
        "replay must not append signoff or status events"
    );

    let replay: Value = serde_json::from_slice(&replay_output.stdout).expect("replay json");
    assert_eq!(
        replay["total_events"],
        before.lines().count() as u64,
        "replay event count should match the target event log"
    );

    let status_output = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "status",
            "--run-dir",
            run_dir,
            "--workflow-id",
            "wf_replay_guard",
            "--json",
        ])
        .output()
        .expect("run status after replay");
    assert!(status_output.status.success());
    let status: Value = serde_json::from_slice(&status_output.stdout).expect("status json");
    let workflow = &status["projection"]["workflows"]["wf_replay_guard"];
    assert_eq!(workflow["terminal"], false);
    assert_eq!(workflow["denied_transition_count"], 1);
}

#[test]
fn workflow_signoff_request_evidence_targets_existing_run() {
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
            "wf_request_evidence",
            "--title",
            "Request evidence target",
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

    let signoff_output = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "signoff",
            "--run-dir",
            run_dir,
            "--workflow-id",
            "wf_request_evidence",
            "--request-evidence",
            "--reason",
            "need deterministic verification evidence",
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
    assert_eq!(signoff["run_dir"], run_dir);
    assert_eq!(signoff["audit_only"], false);
    assert_eq!(signoff["terminal_outcome"], Value::Null);
    assert_eq!(signoff["signoff"]["decision"], "request_evidence");
    assert_eq!(
        signoff["signoff"]["closeout"]["policy_id"],
        "workflow.closeout.default"
    );
    assert!(signoff["signoff"]["closeout"]["legal_next_actions"]
        .as_array()
        .expect("signoff legal next actions")
        .iter()
        .any(|action| action["action"] == "request_evidence"));

    let status_output = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "status",
            "--run-dir",
            run_dir,
            "--workflow-id",
            "wf_request_evidence",
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
    let workflow = &status["projection"]["workflows"]["wf_request_evidence"];
    assert_eq!(workflow["terminal"], false);
    assert_eq!(workflow["operator_decisions"][0], "request-evidence");
}

#[test]
fn workflow_plan_consensus_cli_writes_artifact_and_projection() {
    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");

    let output = harness_command()
        .current_dir(repo_root())
        .args([
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "workflow",
            "plan-consensus",
            "--workflow-id",
            "wf_plan_cli",
            "--plan-id",
            "plan_cli",
            "--task",
            "Plan a replay-derived workflow ledger",
            "--option",
            "event-metadata=Record workflow evidence metadata",
            "--chosen-option",
            "event-metadata",
            "--adr",
            "Use workflow evidence as the replay source for consensus plans.",
            "--risk",
            "Metadata drift can hide plan status.",
            "--test-plan",
            "cargo test -p harness-core plan_consensus",
            "--staffing",
            "planner/architect/critic",
            "--handoff",
            "workflow goal create",
            "--evidence-ref",
            "ctx_plan_cli",
            "--json",
        ])
        .output()
        .expect("run workflow plan-consensus");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("plan json");
    let run_dir = report["run_dir"].as_str().expect("run dir");
    let events_path = report["events_path"].as_str().expect("events path");
    let artifact_path = report["artifact_path"].as_str().expect("artifact path");
    assert!(artifact_path.starts_with("artifacts/workflows/plan_consensus/plan_cli"));
    assert_eq!(report["lanes"].as_array().expect("lanes").len(), 3);

    let artifact_body = fs::read_to_string(std::path::Path::new(run_dir).join(artifact_path))
        .expect("read plan artifact");
    assert!(artifact_body.contains("\"adr\""));
    assert!(artifact_body.contains("event-metadata"));
    assert!(artifact_body.contains("Metadata drift"));
    assert!(artifact_body.contains("planner/architect/critic"));

    let before = fs::read_to_string(events_path).expect("read events before status");
    let status_output = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "status",
            "--run-dir",
            run_dir,
            "--workflow-id",
            "wf_plan_cli",
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
    assert_eq!(
        before, after,
        "plan projection status must not append events"
    );
    let status: Value = serde_json::from_slice(&status_output.stdout).expect("status json");
    let plan = &status["projection"]["plan_consensus"]["plan_cli"];
    assert_eq!(plan["status"], "approved");
    assert_eq!(plan["critic_iterations"], 1);
    assert_eq!(plan["lanes"].as_array().expect("plan lanes").len(), 3);
}

#[test]
fn workflow_goal_cli_creates_and_checkpoints_replay_derived_ledger() {
    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");

    let create_output = harness_command()
        .current_dir(repo_root())
        .args([
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "workflow",
            "goal",
            "create",
            "--workflow-id",
            "wf_goal_cli",
            "--goal-id",
            "goal_cli",
            "--objective",
            "Complete a replay-derived goal ledger",
            "--story",
            "G001=Checkpoint with evidence",
            "--acceptance",
            "final gate recorded",
            "--evidence-ref",
            "plan_cli",
            "--json",
        ])
        .output()
        .expect("run workflow goal create");
    assert!(
        create_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&create_output.stdout),
        String::from_utf8_lossy(&create_output.stderr)
    );
    let create_report: Value = serde_json::from_slice(&create_output.stdout).expect("create json");
    let create_run_dir = create_report["run_dir"].as_str().expect("create run dir");
    let create_status = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "goal",
            "status",
            "--run-dir",
            create_run_dir,
            "--goal-id",
            "goal_cli",
            "--json",
        ])
        .output()
        .expect("run goal create status");
    assert!(
        create_status.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&create_status.stdout),
        String::from_utf8_lossy(&create_status.stderr)
    );
    let create_status: Value =
        serde_json::from_slice(&create_status.stdout).expect("create status json");
    assert_eq!(
        create_status["projection"]["goals"]["goal_cli"]["status"],
        "active"
    );

    let checkpoint_session_dir = temp.path().join("checkpoint-sessions");
    let checkpoint_output = harness_command()
        .current_dir(repo_root())
        .args([
            "--session-dir",
            checkpoint_session_dir.to_str().expect("session dir utf-8"),
            "workflow",
            "goal",
            "checkpoint",
            "--workflow-id",
            "wf_goal_cli_checkpoint",
            "--goal-id",
            "goal_cli",
            "--story-id",
            "G001",
            "--status",
            "complete",
            "--summary",
            "G001 complete with test and review evidence",
            "--evidence-ref",
            "tests-pass",
            "--final-goal",
            "--verification-ref",
            "cargo test -p harness-core goal_ledger",
            "--review-ref",
            "code-review approve",
            "--json",
        ])
        .output()
        .expect("run workflow goal checkpoint");
    assert!(
        checkpoint_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&checkpoint_output.stdout),
        String::from_utf8_lossy(&checkpoint_output.stderr)
    );
    let checkpoint_report: Value =
        serde_json::from_slice(&checkpoint_output.stdout).expect("checkpoint json");
    let run_dir = checkpoint_report["run_dir"].as_str().expect("run dir");
    let events_path = checkpoint_report["events_path"]
        .as_str()
        .expect("events path");
    let before = fs::read_to_string(events_path).expect("read events before goal status");

    let status_output = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "goal",
            "status",
            "--run-dir",
            run_dir,
            "--goal-id",
            "goal_cli",
            "--json",
        ])
        .output()
        .expect("run workflow goal status");
    assert!(
        status_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&status_output.stdout),
        String::from_utf8_lossy(&status_output.stderr)
    );
    let after = fs::read_to_string(events_path).expect("read events after goal status");
    assert_eq!(
        before, after,
        "workflow goal status must be projection-only"
    );
    let status: Value = serde_json::from_slice(&status_output.stdout).expect("goal status json");
    let goal = &status["projection"]["goals"]["goal_cli"];
    assert_eq!(goal["status"], "complete");
    assert_eq!(goal["ready_for_completion"], true);
    assert_eq!(goal["stories"]["G001"]["status"], "complete");
    assert!(goal["final_quality_gate"]["passed"]
        .as_bool()
        .expect("quality gate passed"));
}

#[test]
fn workflow_mission_cli_requires_validator_artifact_and_projects_status() {
    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");

    let init_output = harness_command()
        .current_dir(repo_root())
        .args([
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "workflow",
            "mission",
            "init",
            "--workflow-id",
            "wf_mission_cli",
            "--mission-id",
            "mission_cli",
            "--objective",
            "Compare workflow research options",
            "--question",
            "Which validator path is safer?",
            "--validator-mode",
            "prompt-architect-artifact",
            "--sandbox",
            "No network; cite local artifacts.",
            "--json",
        ])
        .output()
        .expect("run workflow mission init");
    assert!(
        init_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&init_output.stdout),
        String::from_utf8_lossy(&init_output.stderr)
    );
    let init: Value = serde_json::from_slice(&init_output.stdout).expect("mission init json");
    let init_run_dir = init["run_dir"].as_str().expect("mission init run dir");
    let init_status = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "mission",
            "status",
            "--run-dir",
            init_run_dir,
            "--mission-id",
            "mission_cli",
            "--json",
        ])
        .output()
        .expect("run workflow mission status");
    assert!(
        init_status.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&init_status.stdout),
        String::from_utf8_lossy(&init_status.stderr)
    );
    let init_status: Value =
        serde_json::from_slice(&init_status.stdout).expect("mission status json");
    assert_eq!(
        init_status["projection"]["missions"]["mission_cli"]["status"],
        "active"
    );

    let run_session_dir = temp.path().join("mission-run-sessions");
    let run_output = harness_command()
        .current_dir(repo_root())
        .args([
            "--session-dir",
            run_session_dir.to_str().expect("run session dir utf-8"),
            "workflow",
            "mission",
            "run",
            "--workflow-id",
            "wf_mission_cli_run",
            "--mission-id",
            "mission_cli",
            "--iteration",
            "1",
            "--status",
            "complete",
            "--summary",
            "Architect review approved the research result.",
            "--candidate-ref",
            "candidate.json",
            "--validator-mode",
            "prompt-architect-artifact",
            "--review-ref",
            "architect-review.json",
            "--evidence-ref",
            "architect-review.json",
            "--json",
        ])
        .output()
        .expect("run workflow mission result");
    assert!(
        run_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run_output.stdout),
        String::from_utf8_lossy(&run_output.stderr)
    );
    let run: Value = serde_json::from_slice(&run_output.stdout).expect("mission run json");
    let run_dir = run["run_dir"].as_str().expect("mission run dir");
    let events_path = run["events_path"].as_str().expect("mission events path");
    let before = fs::read_to_string(events_path).expect("read mission events before status");
    let status_output = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "mission",
            "status",
            "--run-dir",
            run_dir,
            "--mission-id",
            "mission_cli",
            "--json",
        ])
        .output()
        .expect("run workflow mission status");
    assert!(
        status_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&status_output.stdout),
        String::from_utf8_lossy(&status_output.stderr)
    );
    let after = fs::read_to_string(events_path).expect("read mission events after status");
    assert_eq!(
        before, after,
        "workflow mission status must be projection-only"
    );
    let status: Value = serde_json::from_slice(&status_output.stdout).expect("mission status json");
    let mission = &status["projection"]["missions"]["mission_cli"];
    assert_eq!(mission["status"], "complete");
    assert_eq!(mission["ready_for_completion"], true);
    assert_eq!(
        mission["iterations"][0]["validator_ref"],
        "architect-review.json"
    );
}

#[test]
fn workflow_mission_cli_executes_permissioned_validator_command() {
    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");

    let output = harness_command()
        .current_dir(repo_root())
        .env(
            "HARNESS_CONFIG_CONTENT",
            r#"{ permission: { bash: { "printf validator-ok": "allow", "*": "deny" } } }"#,
        )
        .args([
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "workflow",
            "mission",
            "run",
            "--workflow-id",
            "wf_mission_validator",
            "--mission-id",
            "mission_validator",
            "--iteration",
            "1",
            "--status",
            "complete",
            "--summary",
            "Validator command accepted the candidate.",
            "--candidate-ref",
            "candidate.json",
            "--validator-mode",
            "mission-validator-script",
            "--validator-command",
            "printf validator-ok",
            "--evidence-ref",
            "candidate.json",
            "--json",
        ])
        .output()
        .expect("run workflow mission validator command");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("mission run json");
    let run_dir = std::path::Path::new(report["run_dir"].as_str().expect("run dir"));
    let result_path = report["artifact_path"].as_str().expect("result path");
    let result_body =
        fs::read_to_string(run_dir.join(result_path)).expect("read mission result artifact");
    let result: Value = serde_json::from_str(&result_body).expect("mission result artifact JSON");
    let validator_ref = result["validator"]["result_ref"]
        .as_str()
        .expect("validator result ref");
    assert!(validator_ref.contains("validators/mission_validator-1.json"));
    assert_eq!(result["validator"]["status"], "passed");

    let validator_body =
        fs::read_to_string(run_dir.join(validator_ref)).expect("read validator result artifact");
    assert!(validator_body.contains("validator-ok"));
    let validator: Value =
        serde_json::from_str(&validator_body).expect("validator result artifact JSON");
    assert_eq!(validator["tool_result"]["structured_json"]["success"], true);

    let events_path = report["events_path"].as_str().expect("events path");
    let events = fs::read_to_string(events_path).expect("read events");
    assert!(events.contains("tool_call_finished"));
    assert!(events.contains("printf validator-ok"));
}

#[test]
fn workflow_mission_validator_permission_denial_has_no_process_or_artifact_side_effect() {
    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");
    let marker = temp.path().join("validator-ran.txt");
    let command = format!("printf denied > {}", marker.display());

    let output = harness_command()
        .current_dir(repo_root())
        .env(
            "HARNESS_CONFIG_CONTENT",
            r#"{ permission: { bash: { "*": "deny" } } }"#,
        )
        .args([
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "workflow",
            "mission",
            "run",
            "--workflow-id",
            "wf_mission_validator_denied",
            "--mission-id",
            "mission_validator_denied",
            "--iteration",
            "1",
            "--status",
            "complete",
            "--summary",
            "Validator command must not run without permission.",
            "--candidate-ref",
            "candidate.json",
            "--validator-mode",
            "mission-validator-script",
            "--validator-command",
            &command,
            "--evidence-ref",
            "candidate.json",
            "--json",
        ])
        .output()
        .expect("run denied workflow mission validator command");

    assert!(
        !output.status.success(),
        "permission denial should fail stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("requires `bash` permission"),
        "stderr should explain bash permission denial:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !marker.exists(),
        "denied validator command must not spawn a process or create marker file"
    );

    let run_dir = latest_run_dir(&session_dir);
    let events = fs::read_to_string(run_dir.join("events.jsonl")).expect("read denial events");
    assert!(events.contains("evidence.permission_decision"));
    assert!(events.contains("permission-denied:bash"));
    assert!(!events.contains("tool_call_started"));
    assert!(!events.contains("tool_call_finished"));
    assert!(!run_dir.join("artifacts/workflows/research").exists());
    assert_permission_denial_projection(
        &run_dir,
        "wf_mission_validator_denied",
        "permission-denied:bash",
        &command,
    );
}

#[test]
fn workflow_evidence_record_cli_projects_family_closeout_blockers() {
    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");

    let output = harness_command()
        .current_dir(repo_root())
        .args([
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "workflow",
            "evidence",
            "record",
            "--workflow-id",
            "wf_review_family",
            "--mode",
            "workflow.review",
            "--category",
            "evidence.review",
            "--summary",
            "code review found unresolved blockers",
            "--acceptance-ref",
            "review-blockers",
            "--artifact-path",
            "artifacts/workflows/review/review-blockers.json",
            "--artifact-digest",
            "0123456789abcdef",
            "--status-key",
            "review_status",
            "--status",
            "failed",
            "--metadata",
            "severity=high",
            "--json",
        ])
        .output()
        .expect("run workflow evidence record");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("evidence json");
    let run_dir = report["run_dir"].as_str().expect("run dir");
    assert_eq!(report["category"], "evidence.review");
    assert_eq!(report["metadata"]["review_status"], "failed");
    assert_eq!(report["metadata"]["severity"], "high");

    let status_output = harness_command()
        .current_dir(repo_root())
        .args([
            "workflow",
            "status",
            "--run-dir",
            run_dir,
            "--workflow-id",
            "wf_review_family",
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
    let workflow = &status["projection"]["workflows"]["wf_review_family"];
    assert_eq!(workflow["mode"], "workflow.review");
    assert!(workflow["evidence_categories"]
        .as_array()
        .expect("evidence categories")
        .iter()
        .any(|category| category.as_str() == Some("evidence.review")));
    let review = status["closeout"]["wf_review_family"]["closeout"]["dimensions"]
        .as_array()
        .expect("dimensions")
        .iter()
        .find(|dimension| dimension["id"] == "review")
        .expect("review dimension");
    assert_eq!(review["allowed"], false);
    assert!(review["blocking_refs"]
        .as_array()
        .expect("review blockers")
        .iter()
        .any(|reference| reference
            .as_str()
            .is_some_and(|value| value.contains("review-blockers"))));
}

#[test]
fn workflow_wiki_cli_writes_digested_markdown_and_reads_without_events() {
    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");

    let add_output = harness_command()
        .current_dir(temp.path())
        .args([
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "workflow",
            "wiki",
            "add",
            "--workflow-id",
            "wf_wiki_cli",
            "--slug",
            "workflow-evidence",
            "--title",
            "Workflow Evidence",
            "--category",
            "architecture",
            "--tag",
            "workflow",
            "--tag",
            "evidence",
            "--body",
            "Replay queries use metadata and page digests.",
            "--json",
        ])
        .output()
        .expect("run workflow wiki add");
    assert!(
        add_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&add_output.stdout),
        String::from_utf8_lossy(&add_output.stderr)
    );
    let add: Value = serde_json::from_slice(&add_output.stdout).expect("wiki add json");
    let events_path = add["events_path"].as_str().expect("wiki events path");
    let digest = add["page"]["digest"].as_str().expect("wiki digest");
    assert_eq!(digest.len(), 64);
    let page_path = temp.path().join(".agent-harness/wiki/workflow-evidence.md");
    let page_body = fs::read_to_string(&page_path).expect("read wiki page");
    assert!(page_body.contains("Workflow Evidence"));
    let events = fs::read_to_string(events_path).expect("read wiki events");
    let intent_index = events
        .find("wiki-add-intent")
        .expect("wiki add should record coordinator-owned intent before mutation evidence");
    let evidence_index = events
        .find("evidence.wiki")
        .expect("wiki add should record evidence after mutation");
    assert!(
        intent_index < evidence_index,
        "wiki mutation intent must be recorded before project-visible mutation evidence"
    );
    assert!(events.contains("evidence.wiki"));
    assert!(events.contains(digest));

    let before = fs::read_to_string(events_path).expect("read wiki events before reads");
    let read_output = harness_command()
        .current_dir(temp.path())
        .args([
            "workflow",
            "wiki",
            "read",
            "--slug",
            "workflow-evidence",
            "--json",
        ])
        .output()
        .expect("run workflow wiki read");
    assert!(
        read_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&read_output.stdout),
        String::from_utf8_lossy(&read_output.stderr)
    );
    let read: Value = serde_json::from_slice(&read_output.stdout).expect("wiki read json");
    assert_eq!(read["title"], "Workflow Evidence");

    let query_output = harness_command()
        .current_dir(temp.path())
        .args([
            "workflow",
            "wiki",
            "query",
            "--term",
            "metadata",
            "--tag",
            "workflow",
            "--category",
            "architecture",
            "--json",
        ])
        .output()
        .expect("run workflow wiki query");
    assert!(
        query_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&query_output.stdout),
        String::from_utf8_lossy(&query_output.stderr)
    );
    let query: Value = serde_json::from_slice(&query_output.stdout).expect("wiki query json");
    assert_eq!(query["matches"].as_array().expect("matches").len(), 1);

    let list_output = harness_command()
        .current_dir(temp.path())
        .args(["workflow", "wiki", "list", "--json"])
        .output()
        .expect("run workflow wiki list");
    assert!(
        list_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&list_output.stdout),
        String::from_utf8_lossy(&list_output.stderr)
    );
    let lint_output = harness_command()
        .current_dir(temp.path())
        .args(["workflow", "wiki", "lint", "--json"])
        .output()
        .expect("run workflow wiki lint");
    assert!(
        lint_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&lint_output.stdout),
        String::from_utf8_lossy(&lint_output.stderr)
    );
    let lint: Value = serde_json::from_slice(&lint_output.stdout).expect("wiki lint json");
    assert_eq!(lint["findings"].as_array().expect("findings").len(), 0);
    let after = fs::read_to_string(events_path).expect("read wiki events after reads");
    assert_eq!(
        before, after,
        "wiki read/list/query/lint must not append events"
    );

    let delete_session_dir = temp.path().join("delete-sessions");
    let delete_output = harness_command()
        .current_dir(temp.path())
        .args([
            "--session-dir",
            delete_session_dir
                .to_str()
                .expect("delete session dir utf-8"),
            "workflow",
            "wiki",
            "delete",
            "--workflow-id",
            "wf_wiki_cli_delete",
            "--slug",
            "workflow-evidence",
            "--json",
        ])
        .output()
        .expect("run workflow wiki delete");
    assert!(
        delete_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&delete_output.stdout),
        String::from_utf8_lossy(&delete_output.stderr)
    );
    assert!(!page_path.exists(), "wiki delete should remove the page");
}

#[test]
fn workflow_wiki_permission_denial_has_no_project_file_side_effect() {
    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");
    let page_path = temp.path().join(".agent-harness/wiki/permission-denied.md");

    let output = harness_command()
        .current_dir(temp.path())
        .env("HARNESS_CONFIG_CONTENT", deny_edit_config_content())
        .args([
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "workflow",
            "wiki",
            "add",
            "--workflow-id",
            "wf_wiki_permission_denied",
            "--slug",
            "permission-denied",
            "--title",
            "Permission Denied",
            "--category",
            "security",
            "--body",
            "This page must not be written when edit permission is denied.",
            "--json",
        ])
        .output()
        .expect("run denied workflow wiki add");

    assert!(
        !output.status.success(),
        "permission denial should fail stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("requires `edit` permission"),
        "stderr should explain edit permission denial:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !page_path.exists(),
        "denied wiki add must not write a project-visible wiki page"
    );
    assert!(
        !temp.path().join(".agent-harness/wiki").exists(),
        "denied wiki add must not create the wiki directory"
    );

    let run_dir = latest_run_dir(&session_dir);
    let events = fs::read_to_string(run_dir.join("events.jsonl")).expect("read denial events");
    assert!(events.contains("evidence.permission_decision"));
    assert!(events.contains("permission-denied:edit"));
    assert!(!events.contains("evidence.wiki"));
    assert_permission_denial_projection(
        &run_dir,
        "wf_wiki_permission_denied",
        "permission-denied:edit",
        ".agent-harness/wiki/permission-denied.md",
    );
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

#[test]
fn workflow_init_apply_permission_denial_writes_no_project_files() {
    let temp = tempdir().expect("tempdir");
    let session_dir = temp.path().join("sessions");
    let workflow_dir = temp.path().join(".agent-harness").join("workflows");

    let output = harness_command()
        .current_dir(temp.path())
        .env("HARNESS_CONFIG_CONTENT", deny_edit_config_content())
        .args([
            "--session-dir",
            session_dir.to_str().expect("session dir utf-8"),
            "workflow",
            "init",
            "--apply",
            "--json",
        ])
        .output()
        .expect("run denied workflow init --apply");

    assert!(
        !output.status.success(),
        "permission denial should fail stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("requires `edit` permission"),
        "stderr should explain edit permission denial:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !workflow_dir.exists(),
        "denied workflow init --apply must not create workflow bootstrap files"
    );

    let run_dir = latest_run_dir(&session_dir);
    let events = fs::read_to_string(run_dir.join("events.jsonl")).expect("read denial events");
    assert!(events.contains("evidence.permission_decision"));
    assert!(events.contains("permission-denied:edit"));
    assert!(!events.contains("created"));
    assert_permission_denial_projection(
        &run_dir,
        "wf_workflow_init",
        "permission-denied:edit",
        ".agent-harness/workflows/README.md",
    );
}
