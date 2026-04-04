use std::process::Command;
use std::thread;
use std::time::Duration;
use std::time::SystemTime;

use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EventActor, EventArtifactRef, EventEnvelopeV1, EventV1,
    ExecutionTimingMetadata, PermissionDecision, PermissionRequestedEvent,
    ProviderRequestStartedEvent, RunFailedEvent, RunFinishedEvent, RunStartedEvent,
    TaskCompletedEvent, TaskCompletionMetadata, TaskLineageMetadata, TaskScheduleState,
    TaskScheduledEvent, ToolCallFinishedEvent, ToolCallMetadata, ToolCallRequestedEvent,
    ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
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
    assert!(stdout.contains("run_finished"));
    assert!(stdout.contains("finished"));
    assert!(stdout.contains("interactive"));
    assert!(stdout.contains("run_failed"));
    assert!(stdout.contains("failed"));
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
fn sessions_reopen_json_surfaces_prompt_context_child_sessions_and_artifacts() {
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("resume_fixture_dir");
    std::fs::create_dir_all(&run_dir).expect("create run dir");

    let mut events = vec![
        envelope(
            "run_resume_fixture",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            "run_resume_fixture",
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "worker".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope(
            "run_resume_fixture",
            3,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_000001".to_string(),
                text: "Recover this session headlessly".to_string(),
            }),
        ),
        envelope_with_actor(
            "run_resume_fixture",
            4,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-4o-mini".to_string(),
                prompt_summary: "Recover this session headlessly".to_string(),
                request_digest: "digest-user".to_string(),
            }),
        ),
    ];
    let mut completed_parent_turn = envelope_with_actor(
        "run_resume_fixture",
        5,
        EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000099".to_string(),
            result_summary: "Recovered summary".to_string(),
            result_digest: "digest-parent".to_string(),
            metadata: None,
        }),
    );
    completed_parent_turn.correlation_id = Some("req_000001".to_string());
    events.push(completed_parent_turn);
    events.push(envelope(
        "run_resume_fixture",
        6,
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: "agent_000002".to_string(),
            profile: "worker".to_string(),
            parent_agent_id: Some("agent_000001".to_string()),
        }),
    ));
    events.push(envelope_with_actor(
        "run_resume_fixture",
        7,
        EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "toolcall_000001".to_string(),
            tool_id: "fs.read".to_string(),
            args_summary: "read report.txt".to_string(),
            args_digest: "digest-tool".to_string(),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("fs.read".to_string()),
                alias_source_tool_id: None,
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: None,
                    parent_task_id: Some("task_000001".to_string()),
                    parent_request_id: Some("req_000001".to_string()),
                    parent_session_id: Some("agent_000001".to_string()),
                    child_session_id: Some("agent_000002".to_string()),
                    child_request_id: Some("req_000002".to_string()),
                    child_provider_id: Some("default".to_string()),
                    child_model_id: Some("gpt-4o-mini".to_string()),
                }),
                artifact_refs: vec![EventArtifactRef {
                    path: "artifacts/report.txt".to_string(),
                    digest: Some("digest-report".to_string()),
                }],
                timing: Some(ExecutionTimingMetadata {
                    started_mono_ms: Some(6),
                    finished_mono_ms: Some(7),
                    elapsed_ms: Some(1),
                }),
                hook_executions: Vec::new(),
            }),
        }),
    ));
    events.push(envelope_with_actor(
        "run_resume_fixture",
        8,
        EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "toolcall_000001".to_string(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("read artifact".to_string()),
            output_digest: Some("digest-output".to_string()),
            output_json: None,
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("fs.read".to_string()),
                alias_source_tool_id: None,
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: None,
                    parent_task_id: Some("task_000001".to_string()),
                    parent_request_id: Some("req_000001".to_string()),
                    parent_session_id: Some("agent_000001".to_string()),
                    child_session_id: Some("agent_000002".to_string()),
                    child_request_id: Some("req_000002".to_string()),
                    child_provider_id: Some("default".to_string()),
                    child_model_id: Some("gpt-4o-mini".to_string()),
                }),
                artifact_refs: vec![EventArtifactRef {
                    path: "artifacts/report.txt".to_string(),
                    digest: Some("digest-report".to_string()),
                }],
                timing: Some(ExecutionTimingMetadata {
                    started_mono_ms: Some(6),
                    finished_mono_ms: Some(7),
                    elapsed_ms: Some(1),
                }),
                hook_executions: Vec::new(),
            }),
        }),
    ));
    events.push(envelope_with_actor(
        "run_resume_fixture",
        9,
        EventActor::new(ActorKind::Worker, Some("agent_000002".to_string())),
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_000001".to_string(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:gpt-4o-mini".to_string()),
        }),
    ));
    events.push(envelope_with_actor(
        "run_resume_fixture",
        10,
        EventActor::new(ActorKind::Worker, Some("agent_000002".to_string())),
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000001".to_string(),
            result_summary: "child completed".to_string(),
            result_digest: "digest-child".to_string(),
            metadata: Some(TaskCompletionMetadata {
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some("toolcall_000001".to_string()),
                    parent_task_id: Some("task_000001".to_string()),
                    parent_request_id: Some("req_000001".to_string()),
                    parent_session_id: Some("agent_000001".to_string()),
                    child_session_id: Some("agent_000002".to_string()),
                    child_request_id: Some("req_000002".to_string()),
                    child_provider_id: Some("default".to_string()),
                    child_model_id: Some("gpt-4o-mini".to_string()),
                }),
                timing: Some(ExecutionTimingMetadata {
                    started_mono_ms: Some(8),
                    finished_mono_ms: Some(9),
                    elapsed_ms: Some(1),
                }),
                hook_executions: Vec::new(),
            }),
        }),
    ));
    events.push(envelope(
        "run_resume_fixture",
        11,
        EventV1::RunFinished(RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));

    write_events_jsonl(&run_dir, &events);

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "reopen",
            "--session",
            "run_resume_fixture",
            "--json",
        ])
        .output()
        .expect("run harness sessions reopen");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let summary: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("reopen json should parse");
    assert_eq!(summary["run_id"], "run_resume_fixture");
    assert_eq!(summary["resumable"], true);
    assert_eq!(summary["resume_agent_id"], "agent_000002");
    assert_eq!(
        summary["continue_hint"],
        "harness prompt --resume run_resume_fixture --text \"<next prompt>\""
    );
    assert_eq!(
        summary["prompt_context"][0]["text"],
        "Recover this session headlessly"
    );
    assert_eq!(
        summary["prompt_context"][1]["text"],
        "Recover this session headlessly"
    );
    assert_eq!(
        summary["child_sessions"][0]["parent_tool_call_id"],
        serde_json::Value::Null
    );
    assert_eq!(
        summary["child_sessions"][1]["parent_tool_call_id"],
        "toolcall_000001"
    );
    assert_eq!(summary["artifacts"][0]["path"], "artifacts/report.txt");
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

fn envelope_with_actor(
    run_id: &str,
    seq: u64,
    actor: EventActor,
    payload: EventV1,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: run_id.to_string(),
        mono_ms: seq,
        ts: None,
        actor,
        correlation_id: None,
        causation_id: None,
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}
