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
fn tui_cli_requires_config_for_interactive_mode() {
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
        stderr.contains("tui setup failed: interactive tui mode requires a config file")
            || stderr.contains("tui failed: TUI error:"),
        "expected interactive tui failure, got:\n{stderr}"
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
