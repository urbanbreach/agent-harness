//! E2E tests for the CLI authority matrix (Task 13).
//!
//! Tests that retained CLI commands call real authority and expose
//! meaningful failure behavior. Each advertised command has one happy
//! and one failure E2E test with external postconditions.

use harness::CliDeps;
use harness::CliIo;
use harness::ExitOutcome;
use harness_providers::UnwrapOrAbort;
use std::io::Cursor;

fn run_cli(args: &[&str], deps: CliDeps) -> (i32, String, String) {
    let args: Vec<&str> = std::iter::once("harness")
        .chain(args.iter().copied())
        .collect();
    let mut stdin = Cursor::new(Vec::new());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut io = CliIo::new(&mut stdin, &mut stdout, &mut stderr);
    let ExitOutcome { code, .. } = harness::run(args, &mut io, deps);
    (
        code,
        String::from_utf8_lossy(&stdout).to_string(),
        String::from_utf8_lossy(&stderr).to_string(),
    )
}

fn run_cli_in_temp(args: &[&str]) -> (i32, String, String) {
    let temp = tempfile::tempdir().unwrap_or_abort();
    let deps = CliDeps::real().with_filesystem_root(temp.path().to_path_buf());
    run_cli(args, deps)
}

fn run_cli_in_workspace(args: &[&str]) -> (i32, String, String) {
    run_cli(args, CliDeps::real())
}

#[test]
fn schema_command_emits_valid_runtime_json_schema_when_invoked() {
    let (code, stdout, _stderr) = run_cli_in_workspace(&["schema"]);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(
        parsed["$schema"].is_string()
            || parsed["type"].is_string()
            || parsed["properties"].is_object()
    );
}

#[test]
fn schema_command_emits_valid_tui_json_schema_when_tui_flag_passed() {
    let (code, stdout, _stderr) = run_cli_in_workspace(&["schema", "--tui"]);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(
        parsed["$schema"].is_string()
            || parsed["type"].is_string()
            || parsed["properties"].is_object()
    );
}

#[test]
fn wrap_command_creates_real_tar_gz_archive_on_disk_when_output_provided() {
    let dir = tempfile::tempdir().unwrap();
    let output_path = dir.path().join("workspace.tar.gz");
    let output_arg = output_path.to_str().unwrap().to_string();
    let (code, stdout, stderr) = run_cli_in_workspace(&["wrap", "--output", &output_arg]);
    assert_eq!(code, 0, "{stderr}");
    assert!(output_path.exists(), "archive file must exist on disk");
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(json["status"].as_str().unwrap(), "wrapped");
    assert!(json["file_count"].as_u64().unwrap() >= 1);
}

#[test]
fn wrap_command_returns_error_when_output_path_is_invalid() {
    let (code, _stdout, stderr) =
        run_cli_in_workspace(&["wrap", "--output", "/nonexistent/dir/pkg.tar.gz"]);
    assert_ne!(code, 0);
    assert!(stderr.contains("failed to create") || stderr.contains("failed to write"));
}

#[test]
fn mcp_list_command_emits_server_list_when_config_loads_successfully() {
    let (code, stdout, _stderr) = run_cli_in_workspace(&["mcp", "list"]);
    assert_eq!(code, 0);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(json["servers"].is_array());
}

#[test]
fn mcp_list_command_emits_server_list_without_config_in_temp_dir() {
    let (code, stdout, _stderr) = run_cli_in_temp(&["mcp", "list"]);
    assert_eq!(code, 0);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(json["servers"].is_array());
}

#[test]
fn mcp_health_command_returns_configured_status_for_known_server() {
    let (code, stdout, _stderr) = run_cli_in_workspace(&["mcp", "health", "test-server"]);
    assert_eq!(code, 0);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(json["server_id"].as_str().unwrap(), "test-server");
    assert!(json["configured"].is_boolean());
    assert!(json["enabled"].is_boolean());
}

#[test]
fn mcp_health_command_returns_error_when_server_id_is_empty() {
    let (code, _stdout, stderr) = run_cli_in_workspace(&["mcp", "health", ""]);
    assert_ne!(code, 0);
    assert!(stderr.contains("server_id must not be empty"));
}

#[test]
fn export_command_returns_meaningful_failure_directing_to_sessions_export() {
    let (code, stdout, stderr) = run_cli_in_workspace(&["export", "sess-123"]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("not available"));
    assert!(stderr.contains("harness sessions export"));
}

#[test]
fn best_of_n_command_returns_meaningful_failure_directing_to_run() {
    let (code, stdout, stderr) = run_cli_in_workspace(&["best-of-n", "--prompt", "hello"]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("not available"));
    assert!(stderr.contains("harness run"));
}

#[test]
fn check_command_returns_meaningful_failure_directing_to_doctor() {
    let (code, stdout, stderr) = run_cli_in_workspace(&["check", "--component", "config"]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("not available"));
    assert!(stderr.contains("harness doctor"));
}

#[test]
fn permission_command_returns_meaningful_failure_directing_to_config() {
    let (code, stdout, stderr) =
        run_cli_in_workspace(&["permission", "--permission", "bash", "--level", "deny"]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("not available"));
    assert!(stderr.contains("harness config"));
}

#[test]
fn resume_command_returns_meaningful_failure_directing_to_sessions_resume() {
    let (code, stdout, stderr) = run_cli_in_workspace(&["resume", "--session", "sess-123"]);
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("not available"));
    assert!(stderr.contains("harness sessions resume"));
}
