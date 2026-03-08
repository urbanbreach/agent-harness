use std::process::Command;

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionDecision, PermissionRequestedEvent,
    RunFailedEvent, RunFinishedEvent, RunStartedEvent, TaskScheduleState, TaskScheduledEvent,
    SCHEMA_VERSION,
};
use tempfile::tempdir;

fn envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: run_id.to_string(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("test".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}

fn write_events_jsonl(run_dir: &std::path::Path, events: &[EventEnvelopeV1]) {
    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).expect("serialize event"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).expect("write events");
}

#[test]
fn replay_cli_prints_json_summary() {
    let run_dir = tempdir().expect("tempdir");
    write_events_jsonl(
        run_dir.path(),
        &[
            envelope(
                "run_replay_json",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "json-fixture".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_replay_json",
                2,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task-123".to_string(),
                    state: TaskScheduleState::Queued,
                    queue_key: Some("deep/default:gpt-5.3-codex".to_string()),
                }),
            ),
            envelope(
                "run_replay_json",
                3,
                EventV1::RunFailed(RunFailedEvent {
                    error: "boom".to_string(),
                }),
            ),
        ],
    );

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "replay",
            "--session",
            run_dir.path().to_str().expect("run dir utf-8"),
            "--json",
        ])
        .output()
        .expect("run harness replay json");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("replay json output should parse");
    assert_eq!(summary["run_id"], "run_replay_json");
    assert_eq!(summary["run_name"], "json-fixture");
    assert_eq!(summary["status"], "failed");
    assert_eq!(summary["last_error"], "boom");
    assert_eq!(summary["tasks_in_flight"], serde_json::json!(["task-123"]));
}

#[test]
fn replay_cli_prints_human_summary() {
    let run_dir = tempdir().expect("tempdir");
    write_events_jsonl(
        run_dir.path(),
        &[
            envelope(
                "run_replay_human",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "human-fixture".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_replay_human",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "replay",
            "--session",
            run_dir.path().to_str().expect("run dir utf-8"),
        ])
        .output()
        .expect("run harness replay human");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("run_id: run_replay_human"));
    assert!(stdout.contains("run_name: human-fixture"));
    assert!(stdout.contains("status: Finished"));
    assert!(stdout.contains("counts:"));
}

#[test]
fn sessions_list_cli_prints_finished_and_failed_runs() {
    let session_dir = tempdir().expect("tempdir");
    let finished_dir = session_dir.path().join("run_a");
    let failed_dir = session_dir.path().join("run_b");
    std::fs::create_dir_all(&finished_dir).expect("create finished run dir");
    std::fs::create_dir_all(&failed_dir).expect("create failed run dir");

    write_events_jsonl(
        &finished_dir,
        &[
            envelope(
                "run_finished",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "finished-run".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_finished",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    write_events_jsonl(
        &failed_dir,
        &[
            envelope(
                "run_failed",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "failed-run".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_failed",
                2,
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "perm-1".to_string(),
                    kind: "shell".to_string(),
                    tool_call_id: None,
                    summary: "needs shell".to_string(),
                    request_digest: "digest-1".to_string(),
                    timeout_ms: 30_000,
                    default_decision: PermissionDecision::Deny,
                }),
            ),
            envelope(
                "run_failed",
                3,
                EventV1::RunFailed(RunFailedEvent {
                    error: "nope".to_string(),
                }),
            ),
        ],
    );

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "list",
        ])
        .output()
        .expect("run harness sessions list");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("run_id"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("run_name"));
    assert!(stdout.contains("run_finished"));
    assert!(stdout.contains("finished"));
    assert!(stdout.contains("finished-run"));
    assert!(stdout.contains("run_failed"));
    assert!(stdout.contains("failed"));
    assert!(stdout.contains("failed-run"));
}

#[test]
fn replay_cli_fails_when_events_are_missing() {
    let run_dir = tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "replay",
            "--session",
            run_dir.path().to_str().expect("run dir utf-8"),
        ])
        .output()
        .expect("run harness replay with missing events");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("replay failed:"));
    assert!(stderr.contains("events.jsonl"));
}
