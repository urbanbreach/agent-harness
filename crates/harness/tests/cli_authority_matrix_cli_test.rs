//! E2E tests for the CLI authority matrix (Task 13).
//!
//! Tests that retained CLI commands call real authority and expose
//! meaningful failure behavior. Each advertised command has one happy
//! and one failure E2E test with external postconditions.

use harness::CliDeps;
use harness::CliIo;
use harness::ExitOutcome;
use harness_providers::UnwrapOrAbort;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

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
    let deps = CliDeps::real()
        .with_current_dir(dir.to_path_buf())
        .with_env("XDG_CONFIG_HOME", dir.join("absent-xdg").to_string_lossy())
        .without_env("HOME")
        .without_env("HARNESS_CONFIG")
        .without_env("HARNESS_CONFIG_CONTENT");
    run_cli(args, deps)
}

fn archive_members(path: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let file = std::fs::File::open(path).unwrap_or_abort();
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    archive
        .entries()
        .unwrap_or_abort()
        .map(|entry| {
            let mut entry = entry.unwrap_or_abort();
            let path = entry.path().unwrap_or_abort().into_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap_or_abort();
            (path, bytes)
        })
        .collect()
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
fn wrap_command_archives_only_selected_members_in_stable_order() {
    for source in ["default", "relative", "absolute", "config"] {
        let dir = tempfile::tempdir().unwrap_or_abort();
        let session_path = if source == "default" {
            Path::new(".agent-harness/sessions")
        } else {
            Path::new("state/sessions")
        };
        std::fs::create_dir_all(dir.path().join(session_path)).unwrap_or_abort();
        std::fs::write(
            dir.path().join(session_path).join("raw.bin"),
            b"raw\0session",
        )
        .unwrap_or_abort();
        // Create ordinary files out of order to check stable archive ordering.
        std::fs::write(dir.path().join("z.txt"), b"last").unwrap_or_abort();
        std::fs::write(dir.path().join("README.md"), b"fixture").unwrap_or_abort();
        let mut ordinary = vec![
            (PathBuf::from("README.md"), b"fixture".to_vec()),
            (PathBuf::from("z.txt"), b"last".to_vec()),
        ];
        let session_arg = if source == "absolute" {
            dir.path().join(session_path)
        } else {
            PathBuf::from("state/../state/sessions")
        };
        if source == "config" {
            let config = br#"{
                "provider": {"default": {
                    "type": "openai_compatible",
                    "options": {"baseURL": "http://127.0.0.1:9999/v1", "apiKey": "DUMMY"},
                    "models": {"mock-model": {"name": "Mock Model"}}
                }},
                "model": "default/mock-model",
                "agent": {"default": {"model": "default/mock-model"}},
                "runtime": {"session_dir": "state/sessions"}
            }"#;
            std::fs::write(dir.path().join("harness.jsonc"), config).unwrap_or_abort();
            ordinary.push((PathBuf::from("harness.jsonc"), config.to_vec()));
        }
        for with_sessions in [false, true] {
            let output_path = dir.path().join("workspace.wrap.tar.gz");
            let output_arg = output_path.to_str().unwrap_or_abort();
            let mut args = vec!["wrap"];
            if source == "relative" {
                args.extend(["--output", "output/../workspace.wrap.tar.gz"]);
            } else if source != "default" {
                args.extend(["--output", output_arg]);
            }
            if matches!(source, "relative" | "absolute") {
                args.extend(["--session-dir", session_arg.to_str().unwrap_or_abort()]);
            }
            if with_sessions {
                args.push("--with-sessions");
            }
            let mut expected = ordinary.clone();
            if with_sessions {
                expected.push((session_path.join("raw.bin"), b"raw\0session".to_vec()));
            }
            expected.sort_by(|left, right| left.0.cmp(&right.0));
            for _ in 0..2 {
                let (code, stdout, stderr) = run_cli_in_dir(&args, dir.path());
                assert_eq!(code, 0, "{source}: {stderr}");
                let json: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_abort();
                assert_eq!(json["status"], "wrapped");
                assert_eq!(archive_members(&output_path), expected, "{source}");
            }
        }
    }
}

#[test]
fn wrap_command_does_not_include_external_session_directories() {
    let dir = tempfile::tempdir().unwrap_or_abort();
    let workspace = dir.path().join("workspace");
    let sessions = dir.path().join("sessions");
    std::fs::create_dir_all(&workspace).unwrap_or_abort();
    std::fs::create_dir_all(&sessions).unwrap_or_abort();
    std::fs::write(workspace.join("README.md"), b"fixture").unwrap_or_abort();
    std::fs::write(sessions.join("raw.bin"), b"external").unwrap_or_abort();
    let output = workspace.join("workspace.tar.gz");
    for session_arg in ["../sessions", sessions.to_str().unwrap_or_abort()] {
        let mut args = vec![
            "wrap",
            "--session-dir",
            session_arg,
            "--output",
            output.to_str().unwrap_or_abort(),
        ];
        let (code, _, stderr) = run_cli_in_dir(&args, &workspace);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(
            archive_members(&output),
            vec![(PathBuf::from("README.md"), b"fixture".to_vec())]
        );
        let previous = std::fs::read(&output).unwrap_or_abort();
        args.push("--with-sessions");
        let (code, stdout, stderr) = run_cli_in_dir(&args, &workspace);
        assert_eq!(code, 2, "{stderr}");
        assert!(stdout.is_empty());
        assert!(stderr.contains("outside the workspace"), "{stderr}");
        assert_eq!(std::fs::read(&output).unwrap_or_abort(), previous);
    }
}

#[cfg(unix)]
#[test]
fn wrap_command_rejects_symlinks_instead_of_following_or_skipping_them() {
    let outside = tempfile::tempdir().unwrap_or_abort();
    std::fs::write(outside.path().join("raw.bin"), b"external").unwrap_or_abort();
    for kind in ["directory", "file", "cycle", "dangling"] {
        let dir = tempfile::tempdir().unwrap_or_abort();
        let target = match kind {
            "directory" => outside.path().to_path_buf(),
            "file" => outside.path().join("raw.bin"),
            "cycle" => dir.path().to_path_buf(),
            _ => outside.path().join("missing"),
        };
        std::os::unix::fs::symlink(target, dir.path().join("link")).unwrap_or_abort();
        let output = dir.path().join("workspace.tar.gz");
        let (code, stdout, stderr) = run_cli_in_dir(
            &["wrap", "--output", output.to_str().unwrap_or_abort()],
            dir.path(),
        );
        assert_eq!(code, 2, "{kind}: {stderr}");
        assert!(stdout.is_empty());
        assert!(stderr.contains("symlink"), "{kind}: {stderr}");
        assert!(!output.exists());
    }
}

#[cfg(unix)]
#[test]
fn wrap_command_preserves_output_when_input_is_unreadable_or_unsupported() {
    use std::os::unix::fs::PermissionsExt;

    for kind in ["file", "directory", "socket"] {
        let dir = tempfile::tempdir().unwrap_or_abort();
        let input = dir.path().join("unreadable");
        if kind == "directory" {
            std::fs::create_dir(&input).unwrap_or_abort();
        } else if kind == "socket" {
            drop(std::os::unix::net::UnixListener::bind(&input).unwrap_or_abort());
        } else {
            std::fs::write(&input, b"unreadable").unwrap_or_abort();
        }
        let permissions = std::fs::metadata(&input).unwrap_or_abort().permissions();
        std::fs::set_permissions(&input, std::fs::Permissions::from_mode(0o000)).unwrap_or_abort();
        let output = dir.path().join("workspace.tar.gz");
        std::fs::write(&output, b"previous archive").unwrap_or_abort();
        let (code, stdout, stderr) = run_cli_in_dir(
            &["wrap", "--output", output.to_str().unwrap_or_abort()],
            dir.path(),
        );
        std::fs::set_permissions(&input, permissions).unwrap_or_abort();
        assert_eq!(code, 2, "{stderr}");
        assert!(stdout.is_empty());
        let expected_error = if kind == "socket" {
            "unsupported archive entry"
        } else {
            "failed to read"
        };
        assert!(stderr.contains(expected_error), "{stderr}");
        assert_eq!(
            std::fs::read(&output).unwrap_or_abort(),
            b"previous archive"
        );
    }
}

#[test]
fn trace_command_preserves_raw_artifacts_and_excludes_its_previous_archive() {
    let dir = tempfile::tempdir().unwrap_or_abort();
    let run_dir = dir.path().join("run-001");
    std::fs::create_dir(&run_dir).unwrap_or_abort();
    std::fs::write(run_dir.join("events.jsonl"), b"").unwrap_or_abort();
    std::fs::write(run_dir.join("raw.bin"), b"raw\0artifact").unwrap_or_abort();
    let args = [
        "trace",
        "run-001",
        "--session-dir",
        dir.path().to_str().unwrap_or_abort(),
        "--json",
    ];
    let output = run_dir.join("run-001.tar.gz");
    for _ in 0..2 {
        let (code, _, stderr) = run_cli_in_dir(&args, dir.path());
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(
            archive_members(&output),
            vec![
                (PathBuf::from("events.jsonl"), Vec::new()),
                (PathBuf::from("raw.bin"), b"raw\0artifact".to_vec()),
            ]
        );
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.path(), run_dir.join("link")).unwrap_or_abort();
        let previous = std::fs::read(&output).unwrap_or_abort();
        let (code, stdout, stderr) = run_cli_in_dir(&args, dir.path());
        assert_eq!(code, 1, "{stderr}");
        assert!(stdout.is_empty());
        assert!(stderr.contains("symlink"), "{stderr}");
        assert_eq!(std::fs::read(&output).unwrap_or_abort(), previous);

        std::fs::remove_file(run_dir.join("link")).unwrap_or_abort();
        let outside = tempfile::tempdir().unwrap_or_abort();
        let moved = outside.path().join("run-001");
        std::fs::rename(&run_dir, &moved).unwrap_or_abort();
        std::os::unix::fs::symlink(&moved, &run_dir).unwrap_or_abort();
        let (code, stdout, stderr) = run_cli_in_dir(&args, dir.path());
        assert_eq!(code, 1, "{stderr}");
        assert!(stdout.is_empty());
        assert!(stderr.contains("symlink"), "{stderr}");
        assert_eq!(std::fs::read(&output).unwrap_or_abort(), previous);
    }
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
