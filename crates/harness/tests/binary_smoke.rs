use std::fs;
use std::path::Path;
use std::process::Command;
use std::process::Output;

use serde_json::Value;
use tempfile::tempdir;

const EXAMPLE_CONFIG: &str = include_str!("../../../configs/harness.example.jsonc");

#[test]
#[ignore = "T5 binary smoke; set HARNESS_BINARY_SMOKE=1 and run explicitly"]
fn harness_binary_supports_operator_first_run_smoke() {
    // arrange
    assert_eq!(
        std::env::var("HARNESS_BINARY_SMOKE").as_deref(),
        Ok("1"),
        "set HARNESS_BINARY_SMOKE=1 to run the T5 binary smoke"
    );
    let temp = tempdir().expect("tempdir");
    let config_path = temp.path().join("harness.jsonc");
    let prompt_events_path = temp.path().join("prompt.events.jsonl");
    fs::write(&config_path, EXAMPLE_CONFIG).expect("write copied harness config");
    fs::create_dir_all(temp.path().join(".agent-harness"))
        .expect("create session directory parent");

    // act
    let help_output = harness_binary()
        .arg("--help")
        .output()
        .expect("run harness --help through real binary");
    let version_output = harness_binary()
        .arg("--version")
        .output()
        .expect("run harness --version through real binary");
    let validate_output = outside_repo_harness(temp.path())
        .args(["config", "validate"])
        .output()
        .expect("run harness config validate through real binary outside repo");
    let doctor_output = outside_repo_harness(temp.path())
        .arg("doctor")
        .output()
        .expect("run harness doctor through real binary outside repo");
    let doctor_json_output = outside_repo_harness(temp.path())
        .args(["doctor", "--json"])
        .output()
        .expect("run harness doctor --json through real binary outside repo");
    let prompt_events_arg = prompt_events_path
        .to_str()
        .expect("prompt events path utf-8");
    let prompt_output = outside_repo_harness(temp.path())
        .args([
            "prompt",
            "--mock",
            "--text",
            "Hello from PTY",
            "--out",
            prompt_events_arg,
            "--print-run-dir",
        ])
        .output()
        .expect("run harness prompt --mock through real binary outside repo");

    // assert
    assert_success(&help_output);

    let stdout = String::from_utf8_lossy(&help_output.stdout);
    assert!(stdout.contains("Usage:"), "stdout:\n{stdout}");
    assert!(stdout.contains("config"), "stdout:\n{stdout}");

    assert_success(&version_output);

    let stdout = String::from_utf8_lossy(&version_output.stdout);
    assert!(
        stdout.trim() == format!("harness {}", env!("CARGO_PKG_VERSION")),
        "stdout:\n{stdout}"
    );

    assert_success(&validate_output);

    let stdout = String::from_utf8_lossy(&validate_output.stdout);
    assert!(stdout.contains("harness.jsonc"), "stdout:\n{stdout}");

    assert_success(&doctor_output);

    let stdout = String::from_utf8_lossy(&doctor_output.stdout);
    assert!(stdout.contains("doctor ok:"), "stdout:\n{stdout}");
    assert!(stdout.contains("resolved_routes"), "stdout:\n{stdout}");
    assert!(
        stdout.contains("will launch only at runtime"),
        "stdout:\n{stdout}"
    );

    assert_success(&doctor_json_output);

    let report: Value =
        serde_json::from_slice(&doctor_json_output.stdout).expect("doctor json report");
    assert!(report["config"]
        .as_str()
        .expect("config display")
        .contains("harness.jsonc"));
    let route_check = report["checks"]
        .as_array()
        .expect("checks array")
        .iter()
        .find(|check| check["name"] == "resolved_routes")
        .expect("resolved_routes check");
    assert_eq!(route_check["status"], "pass");
    assert_eq!(route_check["details"]["no_network_probes"], true);
    assert_eq!(
        route_check["details"]["routes"]["build"]["model"]["model"],
        "gpt-5.4-mini"
    );

    assert_success(&prompt_output);

    let stdout = String::from_utf8_lossy(&prompt_output.stdout);
    assert!(stdout.contains("Hello world"), "stdout:\n{stdout}");
    assert!(
        stdout.contains(".agent-harness/sessions/prompt_"),
        "stdout:\n{stdout}"
    );
    let prompt_events = fs::read_to_string(&prompt_events_path).expect("read prompt event log");
    assert!(prompt_events.contains("\"event_type\":\"task_completed\""));
    assert!(prompt_events.contains("Hello world"));
}

fn harness_binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_harness"))
}

fn outside_repo_harness(workdir: &Path) -> Command {
    let mut command = harness_binary();
    command.current_dir(workdir);
    command.env_remove("HARNESS_CONFIG");
    command.env_remove("HARNESS_CONFIG_CONTENT");
    command.env("XDG_CONFIG_HOME", workdir.join("xdg"));
    command
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
