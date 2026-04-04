use std::process::Command;
use std::thread;
use std::time::Duration;
use std::time::SystemTime;

use harness_core::event::{
    ActorKind, AgentSpawnedEvent, ArtifactWrittenEvent, EventActor, EventEnvelopeV1, EventV1,
    ExecutionTimingMetadata, PermissionDecision, PermissionRequestedEvent,
    ProviderRequestStartedEvent, RunFailedEvent, RunFinishedEvent, RunStartedEvent,
    TaskLineageMetadata, TaskScheduleState, TaskScheduledEvent, ToolCallFinishedEvent,
    ToolCallMetadata, ToolCallStatus, SCHEMA_VERSION,
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

fn agent_envelope(run_id: &str, seq: u64, agent_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: run_id.to_string(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some(agent_id.to_string())),
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

fn write_events_lines(run_dir: &std::path::Path, lines: &[&str]) {
    let body = lines.join("\n");
    std::fs::write(run_dir.join("events.jsonl"), format!("{body}\n")).expect("write events");
}

fn events_modified(run_dir: &std::path::Path) -> SystemTime {
    run_dir
        .join("events.jsonl")
        .metadata()
        .expect("read events metadata")
        .modified()
        .expect("read events modified time")
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
                    queue_key: Some("deep/default:gpt-5.4-mini".to_string()),
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
fn replay_cli_surfaces_artifacts_and_child_sessions_for_recovery() {
    let run_dir = tempdir().expect("tempdir");
    std::fs::create_dir_all(run_dir.path().join("artifacts/notes")).expect("create artifacts dir");
    std::fs::write(
        run_dir.path().join("artifacts/notes/output.txt"),
        "artifact body\n",
    )
    .expect("write artifact");

    write_events_jsonl(
        run_dir.path(),
        &[
            envelope(
                "run_recovery_detail",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_recovery_detail",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "worker".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                }),
            ),
            agent_envelope(
                "run_recovery_detail",
                3,
                "agent_child",
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_1".to_string(),
                    provider_id: "openai".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "delegate".to_string(),
                    request_digest: "digest-1".to_string(),
                }),
            ),
            envelope(
                "run_recovery_detail",
                4,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_1".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("wrote artifact".to_string()),
                    output_digest: Some("tool-digest".to_string()),
                    output_json: None,
                    metadata: Some(ToolCallMetadata {
                        lineage: Some(TaskLineageMetadata {
                            parent_tool_call_id: Some("toolcall_parent".to_string()),
                            parent_task_id: Some("task_1".to_string()),
                            parent_request_id: Some("req_0".to_string()),
                            parent_session_id: Some("agent_parent".to_string()),
                            child_session_id: Some("agent_child".to_string()),
                            child_request_id: Some("req_1".to_string()),
                            child_provider_id: Some("openai".to_string()),
                            child_model_id: Some("gpt-5.4-mini".to_string()),
                        }),
                        timing: Some(ExecutionTimingMetadata {
                            started_mono_ms: Some(3),
                            finished_mono_ms: Some(7),
                            elapsed_ms: Some(4),
                        }),
                        ..ToolCallMetadata::default()
                    }),
                }),
            ),
            envelope(
                "run_recovery_detail",
                5,
                EventV1::ArtifactWritten(ArtifactWrittenEvent {
                    path: "artifacts/notes/output.txt".to_string(),
                    digest: "artifact-digest".to_string(),
                    bytes: 14,
                    tool_call_id: Some("toolcall_1".to_string()),
                    tool_metadata: Default::default(),
                    metadata: Default::default(),
                }),
            ),
            envelope(
                "run_recovery_detail",
                6,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let json_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "replay",
            "--session",
            run_dir.path().to_str().expect("run dir utf-8"),
            "--json",
        ])
        .output()
        .expect("run harness replay json");

    assert!(
        json_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&json_output.stderr)
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&json_output.stdout).expect("replay json output should parse");
    assert_eq!(summary["artifact_count"], 1);
    assert_eq!(summary["child_session_count"], 1);
    assert_eq!(
        summary["artifacts"][0]["path"],
        "artifacts/notes/output.txt"
    );
    assert_eq!(summary["artifacts"][0]["tool_call_id"], "toolcall_1");
    assert_eq!(summary["artifacts"][0]["child_session_id"], "agent_child");
    assert_eq!(summary["child_sessions"][0]["session_id"], "agent_child");
    assert_eq!(
        summary["child_sessions"][0]["provider_model"],
        "openai/gpt-5.4-mini"
    );

    let human_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "replay",
            "--session",
            run_dir.path().to_str().expect("run dir utf-8"),
        ])
        .output()
        .expect("run harness replay human");

    assert!(
        human_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&human_output.stderr)
    );
    let stdout = String::from_utf8_lossy(&human_output.stdout);
    assert!(stdout.contains("session_dir:"));
    assert!(stdout.contains("artifacts: 1"));
    assert!(stdout.contains("artifacts/notes/output.txt"));
    assert!(stdout.contains("child_sessions: 1"));
    assert!(stdout.contains("agent_child"));
    assert!(stdout.contains("openai/gpt-5.4-mini"));
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
                    run_name: "interactive".to_string(),
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
                    run_name: "interactive".to_string(),
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
    assert!(stdout.contains("profile"));
    assert!(stdout.contains("provider/model"));
    assert!(stdout.contains("artifacts"));
    assert!(stdout.contains("children"));
    assert!(stdout.contains("session_dir"));
    assert!(stdout.contains("run_finished"));
    assert!(stdout.contains("finished"));
    assert!(stdout.contains("interactive"));
    assert!(stdout.contains("run_failed"));
    assert!(stdout.contains("failed"));
}

#[test]
fn session_history_surfaces_discovery_counts_and_session_path() {
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("run_recovery");
    std::fs::create_dir_all(run_dir.join("artifacts/notes")).expect("create run artifacts");
    std::fs::write(
        run_dir.join("artifacts/notes/output.txt"),
        "artifact body\n",
    )
    .expect("write artifact");

    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_recovery",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_recovery",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "worker".to_string(),
                    parent_agent_id: Some("agent_parent".to_string()),
                }),
            ),
            envelope(
                "run_recovery",
                3,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_1".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("wrote artifact".to_string()),
                    output_digest: Some("tool-digest".to_string()),
                    output_json: None,
                    metadata: Some(ToolCallMetadata {
                        lineage: Some(TaskLineageMetadata {
                            parent_tool_call_id: Some("toolcall_parent".to_string()),
                            parent_task_id: Some("task_1".to_string()),
                            parent_request_id: Some("req_0".to_string()),
                            parent_session_id: Some("agent_parent".to_string()),
                            child_session_id: Some("agent_child".to_string()),
                            child_request_id: None,
                            child_provider_id: None,
                            child_model_id: None,
                        }),
                        ..ToolCallMetadata::default()
                    }),
                }),
            ),
            envelope(
                "run_recovery",
                4,
                EventV1::ArtifactWritten(ArtifactWrittenEvent {
                    path: "artifacts/notes/output.txt".to_string(),
                    digest: "artifact-digest".to_string(),
                    bytes: 14,
                    tool_call_id: Some("toolcall_1".to_string()),
                    tool_metadata: Default::default(),
                    metadata: Default::default(),
                }),
            ),
            envelope(
                "run_recovery",
                5,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
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
        .expect("run harness sessions list for discovery");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("run_recovery"));
    assert!(stdout.contains("worker"));
    assert!(stdout.contains(" 1         1        "));
    assert!(stdout.contains(run_dir.to_str().expect("run dir utf-8")));
}

#[test]
fn sessions_inspect_cli_surfaces_recovery_details() {
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("run_recovery_inspect");
    std::fs::create_dir_all(run_dir.join("artifacts/notes")).expect("create run artifacts");
    std::fs::write(
        run_dir.join("artifacts/notes/output.txt"),
        "artifact body\n",
    )
    .expect("write artifact");

    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_recovery_inspect",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_recovery_inspect",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_root".to_string(),
                    profile: "deep".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                "run_recovery_inspect",
                3,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "worker".to_string(),
                    parent_agent_id: Some("agent_root".to_string()),
                }),
            ),
            agent_envelope(
                "run_recovery_inspect",
                4,
                "agent_child",
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_1".to_string(),
                    provider_id: "openai".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "delegate".to_string(),
                    request_digest: "digest-1".to_string(),
                }),
            ),
            envelope(
                "run_recovery_inspect",
                5,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_1".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("wrote artifact".to_string()),
                    output_digest: Some("tool-digest".to_string()),
                    output_json: None,
                    metadata: Some(ToolCallMetadata {
                        lineage: Some(TaskLineageMetadata {
                            parent_tool_call_id: Some("toolcall_parent".to_string()),
                            parent_task_id: Some("task_1".to_string()),
                            parent_request_id: Some("req_0".to_string()),
                            parent_session_id: Some("agent_root".to_string()),
                            child_session_id: Some("agent_child".to_string()),
                            child_request_id: Some("req_1".to_string()),
                            child_provider_id: Some("openai".to_string()),
                            child_model_id: Some("gpt-5.4-mini".to_string()),
                        }),
                        timing: Some(ExecutionTimingMetadata {
                            started_mono_ms: Some(4),
                            finished_mono_ms: Some(5),
                            elapsed_ms: Some(1),
                        }),
                        ..ToolCallMetadata::default()
                    }),
                }),
            ),
            envelope(
                "run_recovery_inspect",
                6,
                EventV1::ArtifactWritten(ArtifactWrittenEvent {
                    path: "artifacts/notes/output.txt".to_string(),
                    digest: "artifact-digest".to_string(),
                    bytes: 14,
                    tool_call_id: Some("toolcall_1".to_string()),
                    tool_metadata: Default::default(),
                    metadata: Default::default(),
                }),
            ),
            envelope(
                "run_recovery_inspect",
                7,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "inspect",
            "--run",
            "run_recovery_inspect",
        ])
        .output()
        .expect("run harness sessions inspect");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("mode: interactive_live"));
    assert!(stdout.contains("resume: yes"));
    assert!(stdout.contains("session_dir:"));
    assert!(stdout.contains("artifacts: 1"));
    assert!(stdout.contains("artifacts/notes/output.txt"));
    assert!(stdout.contains("child_sessions: 1"));
    assert!(stdout.contains("agent_child"));

    let json_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "inspect",
            "--run",
            "run_recovery_inspect",
            "--json",
        ])
        .output()
        .expect("run harness sessions inspect json");

    assert!(
        json_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&json_output.stderr)
    );
    let inspected: serde_json::Value =
        serde_json::from_slice(&json_output.stdout).expect("sessions inspect json should parse");
    assert_eq!(inspected["catalog"]["run_id"], "run_recovery_inspect");
    assert_eq!(inspected["replay"]["artifact_count"], 1);
    assert_eq!(inspected["replay"]["child_session_count"], 1);
    assert_eq!(
        inspected["replay"]["artifacts"][0]["path"],
        "artifacts/notes/output.txt"
    );
}

#[test]
fn session_history_entries_sort_by_recency() {
    let session_dir = tempdir().expect("tempdir");
    let older_dir = session_dir.path().join("alpha_session");
    std::fs::create_dir_all(&older_dir).expect("create older run dir");
    let older_events = [
        envelope(
            "run_older",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "older-run".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            "run_older",
            2,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ];
    write_events_jsonl(&older_dir, &older_events);
    let older_modified = events_modified(&older_dir);

    let newer_dir = session_dir.path().join("omega_session");
    std::fs::create_dir_all(&newer_dir).expect("create newer run dir");
    let newer_events = [
        envelope(
            "run_newer",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "newer-run".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            "run_newer",
            2,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ];
    write_events_jsonl(&newer_dir, &newer_events);

    for _ in 0..20 {
        if events_modified(&newer_dir) > older_modified {
            break;
        }
        thread::sleep(Duration::from_millis(50));
        write_events_jsonl(&newer_dir, &newer_events);
    }

    assert!(
        events_modified(&newer_dir) > older_modified,
        "test fixture must encode real recency independent of lexical directory names"
    );

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "list",
        ])
        .output()
        .expect("run harness sessions list for recency order");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let rows = stdout.lines().skip(1).collect::<Vec<_>>();
    assert_eq!(rows.len(), 2, "expected exactly two session rows\n{stdout}");
    assert!(
        rows[0].contains("run_newer"),
        "expected newer session first\n{stdout}"
    );
    assert!(
        rows[1].contains("run_older"),
        "expected older session second\n{stdout}"
    );
}

#[test]
fn session_history_marks_corrupt_runs_without_hiding_them() {
    let session_dir = tempdir().expect("tempdir");
    let good_dir = session_dir.path().join("run_good");
    let corrupt_dir = session_dir.path().join("run_corrupt");
    std::fs::create_dir_all(&good_dir).expect("create good run dir");
    std::fs::create_dir_all(&corrupt_dir).expect("create corrupt run dir");

    write_events_jsonl(
        &good_dir,
        &[
            envelope(
                "run_good",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_good",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    write_events_lines(&corrupt_dir, &["{this is not json"]);

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "list",
        ])
        .output()
        .expect("run harness sessions list with corrupt run");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("run_good"));
    assert!(stdout.contains("run_corrupt"));
    assert!(stdout.contains("events unavailable:"));
    assert!(stdout.contains("<unavailable>"));
}

#[test]
fn session_history_excludes_scenario_fixture_runs_by_default() {
    let session_dir = tempdir().expect("tempdir");
    let interactive_dir = session_dir.path().join("interactive_run");
    let scenario_dir = session_dir.path().join("scenario_run");
    std::fs::create_dir_all(&interactive_dir).expect("create interactive run dir");
    std::fs::create_dir_all(&scenario_dir).expect("create scenario run dir");

    write_events_jsonl(
        &interactive_dir,
        &[
            envelope(
                "run_interactive",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_interactive",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    write_events_jsonl(
        &scenario_dir,
        &[
            envelope(
                "run_scenario",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "golden_path".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_scenario",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
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
        .expect("run harness sessions list for scenario filtering");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("run_interactive"));
    assert!(
        !stdout.contains("run_scenario"),
        "scenario fixture sessions must be excluded by default\n{stdout}"
    );
}

#[test]
fn session_history_exposes_profile_and_model_labels() {
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("run_profile_model");
    std::fs::create_dir_all(&run_dir).expect("create run dir");

    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_profile_model",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_profile_model",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_1".to_string(),
                    profile: "worker".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                "run_profile_model",
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_1".to_string(),
                    provider_id: "openai".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "hello".to_string(),
                    request_digest: "digest-1".to_string(),
                }),
            ),
            envelope(
                "run_profile_model",
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
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
        .expect("run harness sessions list for profile/model labels");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("worker"));
    assert!(stdout.contains("openai/gpt-5.4-mini"));
}

#[test]
fn session_history_flags_non_resumable_sessions_with_reason() {
    let session_dir = tempdir().expect("tempdir");
    let prompt_run_dir = session_dir.path().join("prompt_run");
    std::fs::create_dir_all(&prompt_run_dir).expect("create run dir");

    write_events_jsonl(
        &prompt_run_dir,
        &[
            envelope(
                "run_prompt",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "prompt".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_prompt",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
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
        .expect("run harness sessions list for non-resumable flags");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("run_prompt"));
    assert!(stdout.contains("no"));
    assert!(stdout.contains("prompt runs are not resumable"));
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
