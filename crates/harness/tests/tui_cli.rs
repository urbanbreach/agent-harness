use std::process::Command;

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, RunFinishedEvent, RunStartedEvent,
    SCHEMA_VERSION,
};
use tempfile::tempdir;

fn write_events_jsonl(run_dir: &std::path::Path, events: &[EventEnvelopeV1]) {
    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).expect("serialize event"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).expect("write events");
}

fn envelope(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: "run_fixture".to_string(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("test".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some("run:run_fixture".to_string()),
        payload,
    }
}

#[test]
fn tui_cli() {
    let run_dir = tempdir().expect("tempdir");
    write_events_jsonl(
        run_dir.path(),
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "replay-fixture".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "tui",
            "--replay",
            run_dir.path().to_str().expect("run dir utf-8"),
            "--exit-on-finish",
        ])
        .output()
        .expect("run harness tui replay");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn tui_cli_without_config_prints_config_guidance() {
    let temp = tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args(["tui", "--exit-on-finish"])
        .output()
        .expect("run harness tui");

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tui setup failed: interactive mode requires a config file"),
        "expected config guidance prefix, got:\n{stderr}"
    );
    assert!(
        stderr.contains("./harness.jsonc"),
        "expected current-directory config location, got:\n{stderr}"
    );
    assert!(
        stderr.contains("$XDG_CONFIG_HOME/harness/config.jsonc"),
        "expected XDG config location, got:\n{stderr}"
    );
    assert!(
        stderr.contains("--mock"),
        "expected explicit --mock escape hatch, got:\n{stderr}"
    );
}

#[test]
fn tui_cli_bare_harness_reuses_interactive_mode() {
    let temp = tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .output()
        .expect("run bare harness");

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tui setup failed: interactive mode requires a config file"),
        "expected bare harness to enter interactive tui mode, got:\n{stderr}"
    );
}

#[test]
fn tui_cli_legacy_tui_alias_still_works() {
    let temp = tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args(["tui", "--exit-on-finish"])
        .output()
        .expect("run harness tui");

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tui setup failed: interactive mode requires a config file"),
        "expected legacy tui alias to keep interactive mode behavior, got:\n{stderr}"
    );
}

#[test]
fn tui_cli_mock_flag_starts_demo_mode() {
    let temp = tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args(["tui", "--mock", "--exit-on-finish"])
        .output()
        .expect("run harness tui mock");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("tui setup failed:"),
        "expected --mock to bypass config guidance, got:\n{stderr}"
    );
    assert!(
        output.status.success() || stderr.contains("tui failed: TUI error:"),
        "expected --mock to reach demo mode startup, got stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        stderr,
    );
}

#[test]
fn tui_cli_root_help_only_shows_minimal_interactive_overrides() {
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args(["--help"])
        .output()
        .expect("run harness help");

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Launch the interactive harness UI or run subcommands"));
    assert!(stdout.contains("--profile <PROFILE>"));
    assert!(stdout.contains("--mock"));
    assert!(stdout.contains("tui"));
    assert!(stdout.contains("run"));
    assert!(stdout.contains("prompt"));
    assert!(
        !stdout.contains("--replay"),
        "root help should keep replay off the bare surface"
    );
    assert!(
        !stdout.contains("--scenario"),
        "root help should keep scenario off the bare surface"
    );
    assert!(
        !stdout.contains("--deterministic"),
        "root help should keep deterministic off the bare surface"
    );
    assert!(
        !stdout.contains("--exit-on-finish"),
        "root help should keep advanced tui flags off the bare surface"
    );
}

#[test]
fn tui_cli_invalid_config_fails_without_mock_fallback() {
    let temp = tempdir().expect("tempdir");
    let missing_config = temp.path().join("does-not-exist.jsonc");
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .current_dir(temp.path())
        .args([
            "--config",
            missing_config
                .to_str()
                .expect("missing config path should be valid utf-8"),
            "tui",
            "--exit-on-finish",
        ])
        .output()
        .expect("run harness tui with invalid config path");

    assert!(
        !output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tui setup failed:"),
        "expected setup failure prefix, got:\n{stderr}"
    );
    assert!(
        !stderr.contains("golden_path") && !stderr.contains("scenario"),
        "invalid interactive config should fail before scenario/mock fallback, got:\n{stderr}"
    );
}
