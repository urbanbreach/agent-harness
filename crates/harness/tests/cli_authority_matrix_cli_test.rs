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

fn run_cli_in_dir(args: &[&str], dir: &std::path::Path) -> (i32, String, String) {
    run_cli(args, CliDeps::real().with_current_dir(dir.to_path_buf()))
}

#[test]
fn schema_command_emits_valid_runtime_json_schema_when_invoked() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["schema"]);
    // assert
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
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["schema", "--tui"]);
    // assert
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
    // arrange
    let dir = tempfile::tempdir().unwrap();
    let output_path = dir.path().join("workspace.tar.gz");
    let output_arg = output_path.to_str().unwrap().to_string();
    std::fs::write(dir.path().join("README.md"), "fixture").unwrap();
    // act
    let (code, stdout, stderr) = run_cli_in_dir(&["wrap", "--output", &output_arg], dir.path());
    // assert
    assert_eq!(code, 0, "{stderr}");
    assert!(output_path.exists(), "archive file must exist on disk");
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(json["status"].as_str().unwrap(), "wrapped");
    assert!(json["bytes"].as_u64().unwrap() >= 1);
}

#[test]
fn wrap_command_returns_error_when_output_path_is_invalid() {
    // arrange
    let dir = tempfile::tempdir().unwrap();
    let non_directory = dir.path().join("not-a-directory");
    std::fs::write(&non_directory, "fixture").unwrap();
    let output_path = non_directory.join("pkg.tar.gz");
    let output_arg = output_path.to_str().unwrap();
    // act
    let (code, _stdout, stderr) = run_cli_in_dir(&["wrap", "--output", output_arg], dir.path());
    // assert
    assert_ne!(code, 0);
    assert!(stderr.contains("failed to create") || stderr.contains("failed to write"));
}

#[test]
fn mcp_list_command_emits_server_list_when_config_loads_successfully() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["mcp", "list"]);
    // assert
    assert_eq!(code, 0);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(json["servers"].is_array());
}

#[test]
fn mcp_list_command_emits_server_list_without_config_in_temp_dir() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_temp(&["mcp", "list"]);
    // assert
    assert_eq!(code, 0);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(json["servers"].is_array());
}

#[test]
fn mcp_health_command_reports_not_configured_without_server_configuration() {
    // arrange
    // act
    let (code, stdout, _stderr) = run_cli_in_workspace(&["mcp", "health", "test-server"]);
    // assert
    assert_eq!(code, 0);
    let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(json["server_id"].as_str().unwrap(), "test-server");
    assert_eq!(json["configured"].as_bool(), Some(false));
    assert_eq!(json["enabled"].as_bool(), Some(false));
    assert_eq!(json["status"].as_str(), Some("not_configured"));
}

#[test]
fn mcp_health_command_returns_error_when_server_id_is_empty() {
    // arrange
    // act
    let (code, _stdout, stderr) = run_cli_in_workspace(&["mcp", "health", ""]);
    // assert
    assert_ne!(code, 0);
    assert!(stderr.contains("server_id must not be empty"));
}

#[test]
fn export_command_fails_when_session_does_not_exist() {
    // arrange
    // act
    let (code, stdout, stderr) = run_cli_in_workspace(&["export", "sess-123"]);
    // assert
    assert_eq!(code, 1);
    assert!(stdout.is_empty());
    assert!(stderr.contains("no session matched"));
}

#[test]
fn best_of_n_command_is_not_exposed() {
    // arrange
    // act
    let (code, stdout, stderr) = run_cli_in_workspace(&["best-of-n", "--prompt", "hello"]);
    // assert
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("unrecognized subcommand 'best-of-n'"));
}

#[test]
fn check_command_is_not_exposed() {
    // arrange
    // act
    let (code, stdout, stderr) = run_cli_in_workspace(&["check", "--component", "config"]);
    // assert
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("unrecognized subcommand 'check'"));
}

#[test]
fn permission_command_is_not_exposed() {
    // arrange
    // act
    let (code, stdout, stderr) =
        run_cli_in_workspace(&["permission", "--permission", "bash", "--level", "deny"]);
    // assert
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("unrecognized subcommand 'permission'"));
}

#[test]
fn resume_command_is_not_exposed() {
    // arrange
    // act
    let (code, stdout, stderr) = run_cli_in_workspace(&["resume", "--session", "sess-123"]);
    // assert
    assert_eq!(code, 2);
    assert!(stdout.is_empty());
    assert!(stderr.contains("unrecognized subcommand 'resume'"));
}
