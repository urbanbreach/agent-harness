use std::collections::BTreeMap;
use std::process::Command;
use std::thread;
use std::time::Duration;
use std::time::SystemTime;

use harness_core::event::{
    ActorKind, AgentSpawnedEvent, ArtifactWrittenEvent, BackgroundTaskNotificationEvent,
    BackgroundTaskNotificationStatus, EventActor, EventArtifactRef, EventEnvelopeV1, EventV1,
    ExecutionTimingMetadata, PermissionDecision, PermissionRequestedEvent, PersistentTask,
    PersistentTaskCreatedEvent, PersistentTaskStatus, ProviderRequestFinishedEvent,
    ProviderRequestStartedEvent, RunFailedEvent, RunFinishedEvent, RunStartedEvent,
    TaskCompletedEvent, TaskCompletionMetadata, TaskLineageMetadata, TaskScheduleState,
    TaskScheduledEvent, TeamBounds, TeamCreatedEvent, TeamMemberRole, TeamMemberSelector,
    TeamMemberSpawnedEvent, TeamMemberSpec, TeamSpec, TeamTask, TeamTaskCreatedEvent,
    TeamTaskStatus, TeamTaskUpdatedEvent, ToolCallFinishedEvent, ToolCallMetadata,
    ToolCallRequestedEvent, ToolCallStatus, ToolIdentityMetadata, UserMessageSubmittedEvent,
    WorkflowEventMetadata, WorkflowEvidenceRecordedEvent, WorkflowOperatorDecisionRecordedEvent,
    WorkflowStartedEvent, SCHEMA_VERSION,
};
use harness_core::workflow::{
    SIMULATED_TOOL_EVIDENCE_CATEGORY, WORKFLOW_QUESTION_EVIDENCE_CATEGORY,
    WORKFLOW_QUESTION_METADATA_ID, WORKFLOW_QUESTION_METADATA_PROMPT_REF,
    WORKFLOW_QUESTION_METADATA_REASON_CODE, WORKFLOW_QUESTION_METADATA_STATUS,
    WORKFLOW_QUESTION_STATUS_ASKED, WORKFLOW_TASK_METADATA_KEY,
};
use tempfile::tempdir;

mod common;

use common::repo_root;

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
                    task_id: "task_123".to_string(),
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
    assert_eq!(summary["tasks_in_flight"], serde_json::json!(["task_123"]));
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
    assert!(stdout.contains("session_path:"));
    assert!(stdout.contains("status: Finished"));
    assert!(stdout.contains("next_steps:"));
    assert!(stdout.contains("counts:"));
    assert!(stdout.contains("artifacts: 0"));
    assert!(stdout.contains("child_sessions: 0"));
}

#[test]
fn replay_projects_workflow_questions_closeout_team_permissions_and_evidence_without_mutation() {
    let run_dir = tempdir().expect("tempdir");
    let events = vec![
        envelope(
            "run_workflow_projection",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "workflow-projection".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            "run_workflow_projection",
            2,
            EventV1::WorkflowStarted(WorkflowStartedEvent {
                workflow_id: "wf_projection".to_string(),
                mode: "workflow.plan_consensus".to_string(),
                owner: "operator".to_string(),
                lane: Some("planning".to_string()),
                title: Some("Projection-only replay".to_string()),
                idempotency_key: None,
            }),
        ),
        envelope(
            "run_workflow_projection",
            3,
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "perm_000001".to_string(),
                kind: "bash".to_string(),
                tool_call_id: None,
                summary: "approve validation command".to_string(),
                request_digest: "digest-permission".to_string(),
                timeout_ms: 30_000,
                default_decision: PermissionDecision::Deny,
            }),
        ),
        envelope(
            "run_workflow_projection",
            4,
            EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
                workflow_id: "wf_projection".to_string(),
                category: WORKFLOW_QUESTION_EVIDENCE_CATEGORY.to_string(),
                summary: "Need acceptance boundary".to_string(),
                artifact_path: Some("artifacts/questions/q-projection.json".to_string()),
                artifact_digest: Some("digest-question".to_string()),
                acceptance_ref: Some("question:q-projection".to_string()),
                metadata: BTreeMap::from([
                    (
                        WORKFLOW_QUESTION_METADATA_ID.to_string(),
                        "q-projection".to_string(),
                    ),
                    (
                        WORKFLOW_QUESTION_METADATA_STATUS.to_string(),
                        WORKFLOW_QUESTION_STATUS_ASKED.to_string(),
                    ),
                    (
                        WORKFLOW_QUESTION_METADATA_REASON_CODE.to_string(),
                        "missing_boundary".to_string(),
                    ),
                    (
                        WORKFLOW_QUESTION_METADATA_PROMPT_REF.to_string(),
                        "prompts/q-projection.md".to_string(),
                    ),
                ]),
            }),
        ),
        envelope(
            "run_workflow_projection",
            5,
            EventV1::WorkflowEvidenceRecorded(WorkflowEvidenceRecordedEvent {
                workflow_id: "wf_projection".to_string(),
                category: SIMULATED_TOOL_EVIDENCE_CATEGORY.to_string(),
                summary: "Recorded projection evidence".to_string(),
                artifact_path: Some("artifacts/workflow/evidence.json".to_string()),
                artifact_digest: Some("digest-evidence".to_string()),
                acceptance_ref: Some("acceptance:projection".to_string()),
                metadata: BTreeMap::new(),
            }),
        ),
        envelope(
            "run_workflow_projection",
            6,
            EventV1::WorkflowOperatorDecisionRecorded(WorkflowOperatorDecisionRecordedEvent {
                workflow_id: "wf_projection".to_string(),
                decision: "request_evidence".to_string(),
                operator: "operator".to_string(),
                reason: Some("question remains open".to_string()),
                correlation_id: None,
            }),
        ),
        envelope(
            "run_workflow_projection",
            7,
            EventV1::PersistentTaskCreated(PersistentTaskCreatedEvent {
                task: PersistentTask {
                    version: 1,
                    task_id: "task-workflow".to_string(),
                    run_id: Some("run_workflow_projection".to_string()),
                    thread_id: None,
                    subject: "finish projection proof".to_string(),
                    description: "projection-only replay proof".to_string(),
                    status: PersistentTaskStatus::Pending,
                    active_form: None,
                    owner: Some("operator".to_string()),
                    blocks: Vec::new(),
                    blocked_by: Vec::new(),
                    metadata: BTreeMap::from([(
                        WORKFLOW_TASK_METADATA_KEY.to_string(),
                        "wf_projection".to_string(),
                    )]),
                },
            }),
        ),
        envelope(
            "run_workflow_projection",
            8,
            EventV1::TeamCreated(TeamCreatedEvent {
                team_run_id: "team_projection".to_string(),
                spec: TeamSpec {
                    version: 1,
                    name: "operator-owned team escalation".to_string(),
                    description: Some("subordinate projection lane".to_string()),
                    lead: None,
                    members: vec![TeamMemberSpec {
                        name: "worker-1".to_string(),
                        role: TeamMemberRole::Member,
                        selector: TeamMemberSelector::SubagentType {
                            subagent_type: "general".to_string(),
                        },
                        prompt: None,
                    }],
                    bounds: TeamBounds::default(),
                    metadata: BTreeMap::from([
                        (
                            WORKFLOW_TASK_METADATA_KEY.to_string(),
                            "wf_projection".to_string(),
                        ),
                        (
                            "verification_evidence_ref".to_string(),
                            "artifacts/team/verification.md".to_string(),
                        ),
                    ]),
                },
                workflow: Some(WorkflowEventMetadata {
                    workflow_id: Some("wf_projection".to_string()),
                    lane: Some("team".to_string()),
                    owner: Some("operator".to_string()),
                    ..WorkflowEventMetadata::default()
                }),
            }),
        ),
        envelope(
            "run_workflow_projection",
            9,
            EventV1::TeamMemberSpawned(TeamMemberSpawnedEvent {
                team_run_id: "team_projection".to_string(),
                member_name: "worker-1".to_string(),
                agent_id: "agent-team-worker-1".to_string(),
                profile: "general".to_string(),
                workflow: Some(WorkflowEventMetadata {
                    workflow_id: Some("wf_projection".to_string()),
                    lane: Some("team".to_string()),
                    owner: Some("operator".to_string()),
                    ..WorkflowEventMetadata::default()
                }),
            }),
        ),
        envelope(
            "run_workflow_projection",
            10,
            EventV1::TeamTaskCreated(TeamTaskCreatedEvent {
                team_run_id: "team_projection".to_string(),
                task: TeamTask {
                    version: 1,
                    task_id: "team-task-projection".to_string(),
                    subject: "verify replay projection".to_string(),
                    description: "projection visibility".to_string(),
                    status: TeamTaskStatus::Pending,
                    owner: Some("worker-1".to_string()),
                    blocks: Vec::new(),
                    blocked_by: Vec::new(),
                    metadata: BTreeMap::from([
                        (
                            WORKFLOW_TASK_METADATA_KEY.to_string(),
                            "wf_projection".to_string(),
                        ),
                        (
                            "blocker_ref".to_string(),
                            "artifacts/team/blocker.md".to_string(),
                        ),
                    ]),
                },
                workflow: Some(WorkflowEventMetadata {
                    workflow_id: Some("wf_projection".to_string()),
                    lane: Some("team".to_string()),
                    owner: Some("operator".to_string()),
                    ..WorkflowEventMetadata::default()
                }),
            }),
        ),
        envelope(
            "run_workflow_projection",
            11,
            EventV1::TeamTaskUpdated(TeamTaskUpdatedEvent {
                team_run_id: "team_projection".to_string(),
                task_id: "team-task-projection".to_string(),
                status: TeamTaskStatus::Completed,
                owner: Some("worker-1".to_string()),
                metadata: BTreeMap::from([
                    (
                        WORKFLOW_TASK_METADATA_KEY.to_string(),
                        "wf_projection".to_string(),
                    ),
                    (
                        "evidence_ref".to_string(),
                        "artifacts/team/task-verification.md".to_string(),
                    ),
                ]),
                workflow: Some(WorkflowEventMetadata {
                    workflow_id: Some("wf_projection".to_string()),
                    lane: Some("team".to_string()),
                    owner: Some("operator".to_string()),
                    ..WorkflowEventMetadata::default()
                }),
            }),
        ),
    ];
    write_events_jsonl(run_dir.path(), &events);
    let before = std::fs::read_to_string(run_dir.path().join("events.jsonl")).expect("read events");

    let json = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "replay",
            "--session",
            run_dir.path().to_str().expect("run dir utf-8"),
            "--json",
        ])
        .output()
        .expect("run harness replay json workflow projection");

    assert!(
        json.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&json.stderr)
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("replay json output should parse");
    assert_eq!(
        summary["pending_permissions"],
        serde_json::json!(["perm_000001"])
    );
    assert_eq!(
        summary["workflow_projection"]["workflows"]["wf_projection"]["mode"],
        "workflow.plan_consensus"
    );
    assert_eq!(
        summary["workflow_projection"]["questions"]["q-projection"]["status"],
        WORKFLOW_QUESTION_STATUS_ASKED
    );
    assert_eq!(
        summary["workflow_projection"]["evidence"]["wf_projection"][1]["artifact_path"],
        "artifacts/workflow/evidence.json"
    );
    assert_eq!(
        summary["workflow_projection"]["teams"]["team_projection"]["task_statuses"]
            ["team-task-projection"],
        "completed"
    );
    assert_eq!(summary["teams"][0]["workflow_id"], "wf_projection");
    assert_eq!(
        summary["teams"][0]["lane_policy"],
        "operator-owned subordinate escalation"
    );
    assert_eq!(
        summary["teams"][0]["members"][0]["agent_id"],
        "agent-team-worker-1"
    );
    assert!(summary["teams"][0]["verification_evidence_refs"]
        .as_array()
        .expect("team evidence refs should be visible")
        .iter()
        .any(|reference| reference == "artifacts/team/task-verification.md"));
    assert!(summary["workflow_projection"]["teams"]["team_projection"]
        ["verification_evidence_refs"]
        .as_array()
        .expect("workflow team evidence refs should be visible")
        .iter()
        .any(|reference| reference == "artifacts/team/task-verification.md"));
    assert!(
        summary["workflow_projection"]["teams"]["team_projection"]["blocker_refs"]
            .as_array()
            .expect("workflow team blocker refs should be visible")
            .iter()
            .any(|reference| reference == "artifacts/team/blocker.md")
    );
    assert_eq!(summary["teams"][0]["task_status_counts"]["completed"], 1);
    assert_eq!(
        summary["workflow_closeout"]["wf_projection"]["overall_allowed"],
        false
    );
    assert!(
        summary["workflow_closeout"]["wf_projection"]["legal_next_actions"]
            .as_array()
            .expect("legal actions should be an array")
            .iter()
            .any(|action| action["action"] == "request_evidence")
    );
    assert!(summary["workflow_closeout"]["wf_projection"]["dimensions"]
        .as_array()
        .expect("closeout dimensions should be an array")
        .iter()
        .any(|dimension| dimension["id"] == "question"
            && dimension["blocking_refs"]
                .as_array()
                .expect("question refs should be an array")
                .iter()
                .any(|reference| reference == "question:q-projection")));

    let human = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "replay",
            "--session",
            run_dir.path().to_str().expect("run dir utf-8"),
        ])
        .output()
        .expect("run harness replay human workflow projection");

    assert!(
        human.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&human.stderr)
    );
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert!(stdout.contains("pending_permissions:"));
    assert!(stdout.contains("perm_000001"));
    assert!(stdout.contains("workflows: 1"));
    assert!(stdout.contains("wf_projection"));
    assert!(stdout.contains("questions:"));
    assert!(stdout.contains("q-projection"));
    assert!(stdout.contains("legal_next_actions=request_evidence"));
    assert!(stdout.contains("artifacts/workflow/evidence.json"));
    assert!(stdout.contains("operator-owned team escalation (team_projection)"));
    assert!(stdout.contains("lane=operator-owned subordinate escalation"));
    assert!(stdout.contains("workflow=wf_projection"));
    assert!(
        stdout.contains("task_statuses=pending:0 claimed:0 in_progress:0 completed:1 deleted:0")
    );
    assert!(stdout.contains("agent=agent-team-worker-1"));
    assert!(
        stdout.contains(
            "evidence_refs=artifacts/team/task-verification.md,artifacts/team/verification.md"
        ) || stdout.contains(
            "evidence_refs=artifacts/team/verification.md,artifacts/team/task-verification.md"
        )
    );
    assert!(stdout.contains("blocker_refs=artifacts/team/blocker.md"));
    assert_eq!(
        std::fs::read_to_string(run_dir.path().join("events.jsonl")).expect("read events"),
        before,
        "replay projection inspection must not mutate events.jsonl"
    );
}

#[test]
fn replay_cli_surfaces_recovery_story_details_from_resume_metadata() {
    let run_dir = tempdir().expect("tempdir");
    write_events_jsonl(
        run_dir.path(),
        &delegated_recovery_events("run_recovery_replay"),
    );

    let human = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "replay",
            "--session",
            run_dir.path().to_str().expect("run dir utf-8"),
        ])
        .output()
        .expect("run harness replay human recovery");

    assert!(
        human.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&human.stderr)
    );
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert!(stdout.contains("artifacts: 1"));
    assert!(stdout.contains("artifacts/delegated/task-output.json"));
    assert!(stdout.contains("tool_call=toolcall_1"));
    assert!(stdout.contains("canonical=agent.spawn"));
    assert!(stdout.contains("alias=task"));
    assert!(stdout.contains("child_session=child-run-001"));
    assert!(stdout.contains("child_sessions: 1"));
    assert!(stdout.contains("child-run-001"));
    assert!(stdout.contains("parent_tool=toolcall_1"));
    assert!(stdout.contains("provider_model=openai/gpt-5.4-mini"));
    assert!(stdout.contains("notification=completed"));
    assert!(stdout.contains("notification_summary=background child completed"));
    assert!(stdout.contains("artifacts=artifacts/delegated/task-output.json"));
    assert!(stdout.contains("next_actions:"));
    assert!(stdout.contains("background_output(request_id=\"child-req-001\", block=false)"));
    assert!(stdout.contains("task(session_id=\"child-run-001\""));

    let json = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "replay",
            "--session",
            run_dir.path().to_str().expect("run dir utf-8"),
            "--json",
        ])
        .output()
        .expect("run harness replay json recovery");

    assert!(
        json.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&json.stderr)
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&json.stdout).expect("replay recovery json should parse");
    assert_eq!(summary["artifact_count"], 1);
    assert_eq!(summary["child_session_count"], 1);
    assert_eq!(
        summary["session_path"],
        run_dir.path().to_str().expect("run dir utf-8")
    );
    assert_eq!(
        summary["artifacts"][0]["path"],
        "artifacts/delegated/task-output.json"
    );
    assert_eq!(summary["artifacts"][0]["tool_call_id"], "toolcall_1");
    assert_eq!(summary["artifacts"][0]["canonical_tool_id"], "agent.spawn");
    assert_eq!(summary["artifacts"][0]["alias_source_tool_id"], "task");
    assert_eq!(summary["artifacts"][0]["child_session_id"], "child-run-001");
    assert_eq!(
        summary["child_sessions"][0]["child_session_id"],
        "child-run-001"
    );
    assert_eq!(
        summary["child_sessions"][0]["provider_model"],
        "openai/gpt-5.4-mini"
    );
    assert_eq!(
        summary["child_sessions"][0]["artifact_paths"][0],
        "artifacts/delegated/task-output.json"
    );
    assert_eq!(
        summary["child_sessions"][0]["notification_status"],
        "completed"
    );
    assert_eq!(
        summary["child_sessions"][0]["notification_summary"],
        "background child completed"
    );
    assert_eq!(
        summary["child_sessions"][0]["notification_terminal_event_id"],
        "evt-0006"
    );
    assert_eq!(
        summary["child_sessions"][0]["next_actions"][0],
        "background_output(request_id=\"child-req-001\", block=false)"
    );
    assert!(summary["child_sessions"][0]["next_actions"]
        .as_array()
        .expect("child next actions")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .any(|action| action.contains("task(session_id=\"child-run-001\"")));
}

#[test]
fn replay_cli_sanitizes_control_char_metadata_in_human_output() {
    let run_dir = tempdir().expect("tempdir");
    write_events_jsonl(
        run_dir.path(),
        &delegated_recovery_events_with_control_chars("run_recovery_controls"),
    );

    let json_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "replay",
            "--session",
            run_dir.path().to_str().expect("run dir utf-8"),
            "--json",
        ])
        .output()
        .expect("run harness replay json with control chars");

    assert!(
        json_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&json_output.stderr)
    );
    let summary: serde_json::Value = serde_json::from_slice(&json_output.stdout)
        .expect("replay json output with control chars should parse");
    assert_eq!(summary["artifact_count"], 1);
    assert_eq!(summary["child_session_count"], 1);
    assert_eq!(
        summary["artifacts"][0]["path"],
        "artifacts/delegated/task-output\n.json"
    );
    assert_eq!(summary["artifacts"][0]["tool_id"], "task");
    assert_eq!(summary["artifacts"][0]["effective_tool_id"], "agent.spawn");
    assert_eq!(summary["artifacts"][0]["canonical_tool_id"], "agent.spawn");
    assert_eq!(summary["artifacts"][0]["alias_source_tool_id"], "task");
    assert_eq!(
        summary["artifacts"][0]["child_session_id"],
        "child-run-001\n\tcontrol"
    );
    assert_eq!(
        summary["child_sessions"][0]["child_session_id"],
        "child-run-001\n\tcontrol"
    );
    assert_eq!(
        summary["child_sessions"][0]["parent_tool_call_id"],
        "toolcall_parent\rcontrol"
    );
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
        .expect("run harness replay human with control chars");

    assert!(
        human_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&human_output.stderr)
    );
    let stdout = String::from_utf8_lossy(&human_output.stdout);
    assert!(!stdout.contains("artifacts/delegated/task-output\n.json"));
    assert!(!stdout.contains("child-run-001\n\tcontrol"));
    assert!(!stdout.contains("toolcall_parent\rcontrol"));
    assert!(stdout.contains("artifacts/delegated/task-output\\n.json"));
    assert!(stdout.contains("child-run-001\\n\\tcontrol"));
    assert!(stdout.contains("parent_tool=toolcall_parent\\rcontrol"));
    assert!(stdout.contains("tool=task"));
    assert!(stdout.contains("effective=agent.spawn"));
    assert!(stdout.contains("canonical=agent.spawn"));
    assert!(stdout.contains("alias=task"));
}

#[test]
fn replay_cli_surfaces_recovery_context_in_json_summary() {
    let run_dir = tempdir().expect("tempdir");
    write_events_jsonl(
        run_dir.path(),
        &[
            envelope(
                "run_replay_context",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_replay_context",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_child".to_string(),
                    profile: "worker".to_string(),
                    parent_agent_id: Some("agent_root".to_string()),
                }),
            ),
            envelope(
                "run_replay_context",
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_1".to_string(),
                    provider_id: "openai".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "resume safely".to_string(),
                    request_digest: "digest-replay-context".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                "run_replay_context",
                4,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_1".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("wrote diff".to_string()),
                    output_digest: Some("digest-tool-output".to_string()),
                    output_json: None,
                    metadata: Some(ToolCallMetadata {
                        lineage: Some(TaskLineageMetadata {
                            parent_session_id: Some("run_parent".to_string()),
                            ..TaskLineageMetadata::default()
                        }),
                        artifact_refs: vec![EventArtifactRef {
                            path: "artifacts/patch.diff".to_string(),
                            digest: Some("digest-artifact".to_string()),
                        }],
                        ..ToolCallMetadata::default()
                    }),
                }),
            ),
            envelope(
                "run_replay_context",
                5,
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
            "--json",
        ])
        .output()
        .expect("run harness replay json with recovery context");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("replay json output should parse");
    assert_eq!(summary["mode_source"], "interactive_live");
    assert_eq!(summary["is_resumable"], true);
    assert_eq!(summary["artifact_count"], 1);
    assert_eq!(summary["child_session_count"], 1);
    assert_eq!(summary["parent_session_id"], "run_parent");
    assert_eq!(summary["workspace_root"], "/tmp/workspace");
}

#[test]
fn replay_cli_merges_on_disk_artifact_discovery_with_recovery_metadata() {
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
                    metadata: None,
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
    assert_eq!(summary["artifacts"][0]["present_on_disk"], true);
    assert_eq!(
        summary["child_sessions"][0]["child_session_id"],
        "agent_child"
    );
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
    assert!(stdout.contains("session_path:"));
    assert!(stdout.contains("artifacts: 1"));
    assert!(stdout.contains("artifacts/notes/output.txt"));
    assert!(stdout.contains("present=yes"));
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
    assert!(stdout.contains("session_path"));
    assert!(stdout.contains("run_finished"));
    assert!(stdout.contains("finished"));
    assert!(stdout.contains("interactive"));
    assert!(stdout.contains("run_failed"));
    assert!(stdout.contains("failed"));
}

#[test]
fn sessions_list_cli_surfaces_recovery_counts_run_path_and_parent() {
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("run_recovery");
    std::fs::create_dir_all(&run_dir).expect("create recovery run dir");
    write_events_jsonl(&run_dir, &delegated_recovery_events("run_recovery_catalog"));

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "list",
        ])
        .output()
        .expect("run harness sessions list for recovery counts");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let row = stdout
        .lines()
        .find(|line| line.contains("run_recovery_catalog"))
        .expect("recovery row");
    let columns = row.split_whitespace().collect::<Vec<_>>();
    assert_eq!(columns[0], "run_recovery_catalog");
    assert_eq!(columns[7], "1");
    assert_eq!(columns[8], "1");
    assert_eq!(columns[9], run_dir.to_str().expect("run dir utf-8"));
    assert_eq!(columns[10], "agent_supervisor");
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
                    metadata: None,
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
    assert!(stdout.contains("session_path:"));
    assert!(stdout.contains("artifacts: 1"));
    assert!(stdout.contains("artifacts/notes/output.txt"));
    assert!(stdout.contains("present=yes"));
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
        inspected["replay"]["session_path"],
        run_dir.to_str().expect("run dir utf-8")
    );
    assert_eq!(
        inspected["replay"]["artifacts"][0]["path"],
        "artifacts/notes/output.txt"
    );
    assert_eq!(inspected["replay"]["artifacts"][0]["present_on_disk"], true);
    assert_eq!(
        inspected["replay"]["child_sessions"][0]["child_session_id"],
        "agent_child"
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
                    metadata: None,
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
fn sessions_list_surfaces_artifact_and_lineage_columns() {
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("run_context");
    std::fs::create_dir_all(&run_dir).expect("create run dir");

    write_events_jsonl(&run_dir, &delegated_recovery_events("run_context"));

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "list",
        ])
        .output()
        .expect("run harness sessions list with recovery columns");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("artifacts"));
    assert!(stdout.contains("children"));
    assert!(stdout.contains("parent"));
    assert!(stdout.contains("run_context"));
    assert!(stdout.contains("agent_supervisor"));
    assert!(stdout.contains("1"));
}

#[test]
fn sessions_help_lists_lifecycle_subcommands() {
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args(["sessions", "--help"])
        .output()
        .expect("run harness sessions help");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("inspect"));
    assert!(stdout.contains("reopen"));
    assert!(stdout.contains("replay"));
    assert!(stdout.contains("continue"));
    assert!(stdout.contains("export"));
}

#[test]
fn sessions_lineage_help_is_harness_branded() {
    let forbidden_terms = forbidden_brand_terms();
    let sessions_help = run_harness_help(&["sessions", "--help"]);
    assert_harness_branded("harness sessions --help", &sessions_help, &forbidden_terms);

    for command in ["tree", "fork", "clone"] {
        if sessions_help.contains(command) {
            let help = run_harness_help(&["sessions", command, "--help"]);
            assert_harness_branded(
                &format!("harness sessions {command} --help"),
                &help,
                &forbidden_terms,
            );
        }
    }

    let repo_root = repo_root();
    let scan_output = Command::new("python3")
        .arg(repo_root.join("scripts/check-forbidden-branding.py"))
        .output()
        .expect("run forbidden-brand scan");
    assert!(
        scan_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&scan_output.stdout),
        String::from_utf8_lossy(&scan_output.stderr)
    );

    let fixture = tempdir().expect("tempdir");
    std::fs::write(
        fixture.path().join("README.md"),
        forbidden_terms[1].as_bytes(),
    )
    .expect("write injected forbidden term");
    std::fs::create_dir_all(fixture.path().join(".sisyphus")).expect("create allowed dir");
    std::fs::write(
        fixture.path().join(".sisyphus/notes.md"),
        forbidden_terms[0].as_bytes(),
    )
    .expect("write allowed-path forbidden term");
    let injected_output = Command::new("python3")
        .arg(repo_root.join("scripts/check-forbidden-branding.py"))
        .arg("--root")
        .arg(fixture.path())
        .output()
        .expect("run forbidden-brand scan against injected fixture");
    assert!(
        !injected_output.status.success(),
        "injected forbidden term should fail scan"
    );

    let git_fixture = tempdir().expect("tempdir");
    let init_output = Command::new("git")
        .arg("init")
        .arg(git_fixture.path())
        .output()
        .expect("initialize temporary git checkout");
    assert!(
        init_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&init_output.stdout),
        String::from_utf8_lossy(&init_output.stderr)
    );
    std::fs::write(git_fixture.path().join("tracked.txt"), b"Harness only")
        .expect("write tracked safe file");
    let add_output = Command::new("git")
        .arg("-C")
        .arg(git_fixture.path())
        .args(["add", "tracked.txt"])
        .output()
        .expect("stage safe file");
    assert!(
        add_output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&add_output.stdout),
        String::from_utf8_lossy(&add_output.stderr)
    );
    std::fs::write(
        git_fixture.path().join("untracked-note.txt"),
        forbidden_terms[2].as_bytes(),
    )
    .expect("write untracked forbidden term");
    let untracked_output = Command::new("python3")
        .arg(repo_root.join("scripts/check-forbidden-branding.py"))
        .arg("--root")
        .arg(git_fixture.path())
        .output()
        .expect("run forbidden-brand scan against git fixture");
    assert!(
        !untracked_output.status.success(),
        "untracked forbidden term in git checkout should fail scan"
    );
}

fn forbidden_brand_terms() -> Vec<String> {
    let source_prefix = ["p", "i"].concat();
    vec![
        ["open", "code"].concat(),
        ["open", "code"].join(" "),
        format!("{source_prefix}-mono"),
        format!("{source_prefix} mono"),
        source_prefix,
    ]
}

fn run_harness_help(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args(args)
        .output()
        .expect("run harness help");
    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("help is utf-8")
}

fn assert_harness_branded(context: &str, text: &str, forbidden_terms: &[String]) {
    let lower = text.to_lowercase();
    let source_prefix = ["p", "i"].concat();
    for term in forbidden_terms {
        let found = if term == &source_prefix {
            lower
                .split(|character: char| !character.is_ascii_alphanumeric() && character != '_')
                .any(|token| token == term)
        } else {
            lower.contains(term)
        };
        assert!(!found, "{context} contains forbidden source-brand term");
    }
    assert!(
        lower.contains("harness"),
        "{context} should use harness branding"
    );
}

#[test]
fn sessions_list_cli_prints_json_entries() {
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("run_json");
    std::fs::create_dir_all(&run_dir).expect("create run dir");

    write_events_jsonl(&run_dir, &delegated_recovery_events("run_json"));

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "list",
            "--json",
        ])
        .output()
        .expect("run harness sessions list json");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let rows: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("sessions json output should parse");
    assert_eq!(rows.as_array().map(Vec::len), Some(1));
    let row = &rows[0];
    assert_eq!(row["run_dir"], run_dir.to_str().expect("run dir utf-8"));
    assert_eq!(row["run_id"], "run_json");
    assert_eq!(row["run_name"], "recovery-fixture");
    assert_eq!(row["status"], "finished");
    assert_eq!(row["profile_preset"], "worker");
    assert_eq!(row["provider_model"], serde_json::Value::Null);
    assert_eq!(row["mode_source"], "unknown");
    assert_eq!(row["is_resumable"], false);
    assert_eq!(row["artifact_count"], 1);
    assert_eq!(row["child_session_count"], 1);
    assert_eq!(row["parent_session_id"], "agent_supervisor");
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
                metadata: None,
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
                route: None,
                task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
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
fn sessions_surfaces_checkpoint_artifacts_in_catalog_and_recovery_views() {
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("run_checkpoint_artifacts");
    std::fs::create_dir_all(&run_dir).expect("create run dir");

    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_checkpoint_artifacts",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_checkpoint_artifacts",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "worker".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                "run_checkpoint_artifacts",
                3,
                EventV1::ArtifactWritten(ArtifactWrittenEvent {
                    path: "artifacts/compactions/agent_000001/checkpoint_000003.json".to_string(),
                    digest: "digest-checkpoint".to_string(),
                    bytes: 84,
                    tool_call_id: None,
                    tool_metadata: None,
                    metadata: BTreeMap::from([
                        (
                            "artifact_kind".to_string(),
                            "provider_context_checkpoint".to_string(),
                        ),
                        ("checkpoint_id".to_string(), "checkpoint_000003".to_string()),
                        ("agent_id".to_string(), "agent_000001".to_string()),
                    ]),
                }),
            ),
            envelope(
                "run_checkpoint_artifacts",
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let list_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "list",
            "--json",
        ])
        .output()
        .expect("run harness sessions list json");

    assert!(
        list_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&list_output.stderr)
    );
    let rows: serde_json::Value =
        serde_json::from_slice(&list_output.stdout).expect("sessions json output should parse");
    assert_eq!(rows.as_array().map(Vec::len), Some(1));
    assert_eq!(rows[0]["run_id"], "run_checkpoint_artifacts");
    assert_eq!(rows[0]["artifact_count"], 1);

    let reopen_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "reopen",
            "--session",
            "run_checkpoint_artifacts",
            "--json",
        ])
        .output()
        .expect("run harness sessions reopen json");

    assert!(
        reopen_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&reopen_output.stderr)
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&reopen_output.stdout).expect("reopen json should parse");
    assert_eq!(
        summary["artifacts"][0]["path"],
        "artifacts/compactions/agent_000001/checkpoint_000003.json"
    );
    assert_eq!(
        summary["artifacts"][0]["kind"],
        "provider_context_checkpoint"
    );
    assert_eq!(
        summary["artifacts"][0]["tool_call_id"],
        serde_json::Value::Null
    );
}

#[test]
fn sessions_list_cli_filters_machine_readable_selectors() {
    let session_dir = tempdir().expect("tempdir");
    let resumable_dir = session_dir.path().join("run_resumable");
    let prompt_dir = session_dir.path().join("run_prompt");
    let failed_dir = session_dir.path().join("run_failed");
    std::fs::create_dir_all(&resumable_dir).expect("create resumable run dir");
    std::fs::create_dir_all(&prompt_dir).expect("create prompt run dir");
    std::fs::create_dir_all(&failed_dir).expect("create failed run dir");

    write_events_jsonl(
        &resumable_dir,
        &[
            envelope(
                "run_resumable",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_resumable",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_1".to_string(),
                    profile: "worker".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope_with_actor(
                "run_resumable",
                3,
                EventActor::new(ActorKind::Worker, Some("agent_1".to_string())),
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_1".to_string(),
                    provider_id: "openai".to_string(),
                    model_id: "gpt-5.4-mini".to_string(),
                    prompt_summary: "hello".to_string(),
                    request_digest: "digest-1".to_string(),
                    metadata: None,
                }),
            ),
            envelope(
                "run_resumable",
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    write_events_jsonl(
        &prompt_dir,
        &[
            envelope(
                "run_prompt_filtered",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "prompt".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_prompt_filtered",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );
    std::fs::write(
        prompt_dir.join("meta.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&serde_json::json!({
                "run_id": "run_prompt_filtered",
                "run_name": "prompt",
                "workspace_root": "/tmp/workspace",
                "profile_preset": "worker",
                "mode_source": "prompt",
                "created_at": "1710000000000"
            }))
            .expect("serialize prompt catalog metadata")
        ),
    )
    .expect("write prompt catalog metadata");

    write_events_jsonl(
        &failed_dir,
        &[
            envelope(
                "run_failed_filtered",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_failed_filtered",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_2".to_string(),
                    profile: "reviewer".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                "run_failed_filtered",
                3,
                EventV1::RunFailed(RunFailedEvent {
                    error: "boom".to_string(),
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
            "--json",
            "--status",
            "finished",
            "--profile",
            "worker",
            "--resumable",
            "false",
        ])
        .output()
        .expect("run harness sessions list with filters");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let rows: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("filtered sessions json should parse");
    assert_eq!(rows.as_array().map(Vec::len), Some(1));
    let row = &rows[0];
    assert_eq!(
        row["run_dir"],
        prompt_dir.to_str().expect("prompt dir utf-8")
    );
    assert_eq!(row["run_id"], "run_prompt_filtered");
    assert_eq!(row["run_name"], "prompt");
    assert_eq!(row["status"], "finished");
    assert_eq!(row["workspace_root"], "/tmp/workspace");
    assert_eq!(row["profile_preset"], "worker");
    assert_eq!(row["provider_model"], serde_json::Value::Null);
    assert_eq!(row["mode_source"], "prompt");
    assert_eq!(row["is_resumable"], false);
    assert_eq!(
        row["resume_disabled_reason"],
        "prompt runs are not resumable"
    );
    assert!(row["last_updated_at"].is_string());
}

#[test]
fn sessions_inspect_cli_accepts_positional_session_selector() {
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("directory_name_differs");
    std::fs::create_dir_all(&run_dir).expect("create run dir");

    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_inspect_positional",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "inspectable".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_inspect_positional",
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_1".to_string(),
                    profile: "worker".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                "run_inspect_positional",
                3,
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
            "directory_name_differs",
            "--json",
        ])
        .output()
        .expect("run harness sessions inspect with positional selector");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let inspected: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("sessions inspect json should parse");
    assert_eq!(inspected["catalog"]["run_id"], "run_inspect_positional");
    assert_eq!(inspected["replay"]["run_name"], "inspectable");
    assert_eq!(
        inspected["run_dir"],
        run_dir.to_str().expect("run dir utf-8")
    );
}

#[test]
fn sessions_replay_cli_resolves_run_id_from_session_catalog() {
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("directory_name_differs");
    std::fs::create_dir_all(&run_dir).expect("create run dir");

    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_resolved",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "resolved".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_resolved",
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
            "replay",
            "run_resolved",
            "--json",
        ])
        .output()
        .expect("run harness sessions replay");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let summary: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("replay json output should parse");
    assert_eq!(summary["run_id"], "run_resolved");
    assert_eq!(summary["run_name"], "resolved");
}

#[test]
fn sessions_export_cli_writes_json_bundle() {
    let session_dir = tempdir().expect("tempdir");
    let run_dir = session_dir.path().join("run_export");
    std::fs::create_dir_all(&run_dir).expect("create run dir");

    write_events_jsonl(
        &run_dir,
        &[
            envelope(
                "run_export",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "exportable".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_export",
                2,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "done".to_string(),
                }),
            ),
        ],
    );

    let export_path = session_dir.path().join("session-export.json");
    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "export",
            "run_export",
            "--output",
            export_path.to_str().expect("export path utf-8"),
        ])
        .output()
        .expect("run harness sessions export");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bundle: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&export_path).expect("read exported session bundle"))
            .expect("export bundle should parse");
    assert_eq!(bundle["catalog"]["run_id"], "run_export");
    assert_eq!(bundle["replay"]["run_name"], "exportable");
    assert_eq!(bundle["events"].as_array().map(Vec::len), Some(2));
}

#[test]
fn sessions_list_cli_supports_run_id_sorting() {
    let session_dir = tempdir().expect("tempdir");
    for run_id in ["run_b", "run_c", "run_a"] {
        let run_dir = session_dir.path().join(run_id);
        std::fs::create_dir_all(&run_dir).expect("create run dir");
        write_events_jsonl(
            &run_dir,
            &[
                envelope(
                    run_id,
                    1,
                    EventV1::RunStarted(RunStartedEvent {
                        run_name: format!("{run_id}-name"),
                        workspace_root: "/tmp/workspace".to_string(),
                    }),
                ),
                envelope(
                    run_id,
                    2,
                    EventV1::RunFinished(RunFinishedEvent {
                        summary: "done".to_string(),
                    }),
                ),
            ],
        );
    }

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "list",
            "--json",
            "--sort",
            "run_id_asc",
        ])
        .output()
        .expect("run harness sessions list with run_id sort");

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let rows: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("sorted sessions json should parse");
    let run_ids = rows
        .as_array()
        .expect("sessions json array")
        .iter()
        .map(|row| row["run_id"].as_str().expect("run_id string"))
        .collect::<Vec<_>>();
    assert_eq!(run_ids, vec!["run_a", "run_b", "run_c"]);
}

#[test]
fn sessions_tree() {
    let session_dir = tempdir().expect("tempdir");
    let root_dir = session_dir.path().join("root_session_dir");
    let child_dir = session_dir.path().join("child_session_dir");
    std::fs::create_dir_all(&root_dir).expect("create root run dir");
    std::fs::create_dir_all(&child_dir).expect("create child run dir");

    write_events_jsonl(&root_dir, &resumable_finished_events("run_tree_root"));
    write_events_jsonl(&child_dir, &resumable_finished_events("run_tree_child"));
    write_harness_lineage_meta(&child_dir, "run_tree_child", "run_tree_root");

    let json_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "tree",
            "--json",
        ])
        .output()
        .expect("run harness sessions tree json");

    assert!(
        json_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&json_output.stderr)
    );
    let tree: serde_json::Value =
        serde_json::from_slice(&json_output.stdout).expect("tree json should parse");
    assert_eq!(tree["session_count"], 2);
    assert_eq!(tree["harness_lineage"][0]["run_id"], "run_tree_root");
    assert_eq!(tree["harness_lineage"][0]["depth"], 0);
    assert_eq!(tree["harness_lineage"][1]["run_id"], "run_tree_child");
    assert_eq!(tree["harness_lineage"][1]["depth"], 1);
    assert_eq!(
        tree["harness_lineage"][1]["parent_session_id"],
        "run_tree_root"
    );

    let rooted_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "tree",
            "--root",
            root_dir.to_str().expect("root dir utf-8"),
            "--filter",
            "child",
        ])
        .output()
        .expect("run harness sessions tree human");

    assert!(
        rooted_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&rooted_output.stderr)
    );
    let stdout = String::from_utf8_lossy(&rooted_output.stdout);
    assert!(stdout.contains("Harness session lineage"));
    assert!(stdout.contains("root: run_tree_root"));
    assert!(stdout.contains("filter: child"));
    assert!(stdout.contains("run_tree_child"));
    assert!(!stdout.contains("run_tree_root status="));
}

#[test]
fn sessions_fork_clone() {
    let session_dir = tempdir().expect("tempdir");
    let source_dir = session_dir.path().join("source_session");
    std::fs::create_dir_all(&source_dir).expect("create source run dir");
    write_events_jsonl(
        &source_dir,
        &resumable_finished_events("run_fork_clone_source"),
    );

    let fork_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "fork",
            "--source",
            "run_fork_clone_source",
            "--cutoff",
            "5",
            "--json",
        ])
        .output()
        .expect("run harness sessions fork json");

    assert!(
        fork_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&fork_output.stderr)
    );
    let forked: serde_json::Value =
        serde_json::from_slice(&fork_output.stdout).expect("fork json should parse");
    assert_eq!(forked["harness_operation"], "fork");
    assert_eq!(forked["source_run_id"], "run_fork_clone_source");
    assert_eq!(forked["source_cutoff_seq"], 5);
    assert_eq!(forked["event_count"], 5);
    assert_eq!(forked["warnings"], serde_json::json!([]));
    assert_eq!(forked["errors"], serde_json::json!([]));
    let fork_child_dir =
        std::path::PathBuf::from(forked["child_run_dir"].as_str().expect("fork child dir"));
    assert!(fork_child_dir.join("events.jsonl").exists());

    let clone_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "clone",
            "--source",
            source_dir.to_str().expect("source dir utf-8"),
            "--json",
        ])
        .output()
        .expect("run harness sessions clone json");

    assert!(
        clone_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&clone_output.stderr)
    );
    let cloned: serde_json::Value =
        serde_json::from_slice(&clone_output.stdout).expect("clone json should parse");
    assert_eq!(cloned["harness_operation"], "clone");
    assert_eq!(cloned["source_run_id"], "run_fork_clone_source");
    assert_eq!(cloned["source_cutoff_seq"], 5);
    assert_eq!(cloned["warnings"], serde_json::json!([]));
    assert_eq!(cloned["errors"], serde_json::json!([]));
    assert_ne!(forked["child_run_id"], cloned["child_run_id"]);

    let human_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "clone",
            "--source",
            "run_fork_clone_source",
        ])
        .output()
        .expect("run harness sessions clone human");
    assert!(
        human_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&human_output.stderr)
    );
    let stdout = String::from_utf8_lossy(&human_output.stdout);
    assert!(stdout.contains("Harness session clone created"));
    assert!(stdout.contains("child_run_id:"));
    assert!(stdout.contains("child_run_dir:"));
}

#[test]
fn sessions_fork_clone_child_replays() {
    let session_dir = tempdir().expect("tempdir");
    let source_dir = session_dir.path().join("replay_source");
    std::fs::create_dir_all(&source_dir).expect("create source run dir");
    write_events_jsonl(
        &source_dir,
        &resumable_finished_events("run_child_replay_source"),
    );

    let fork_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "fork",
            "--source",
            "run_child_replay_source",
            "--cutoff",
            "5",
            "--json",
        ])
        .output()
        .expect("run harness sessions fork json");
    assert!(
        fork_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&fork_output.stderr)
    );
    let forked: serde_json::Value =
        serde_json::from_slice(&fork_output.stdout).expect("fork json should parse");
    let child_run_id = forked["child_run_id"].as_str().expect("child run id");

    let inspect_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "inspect",
            child_run_id,
            "--json",
        ])
        .output()
        .expect("run harness sessions inspect child");
    assert!(
        inspect_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&inspect_output.stderr)
    );
    let inspected: serde_json::Value =
        serde_json::from_slice(&inspect_output.stdout).expect("inspect json should parse");
    assert_eq!(inspected["catalog"]["run_id"], child_run_id);
    assert_eq!(inspected["replay"]["is_resumable"], true);

    let replay_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "replay",
            child_run_id,
            "--json",
        ])
        .output()
        .expect("run harness sessions replay child");
    assert!(
        replay_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&replay_output.stderr)
    );
    let replay: serde_json::Value =
        serde_json::from_slice(&replay_output.stdout).expect("replay json should parse");
    assert_eq!(replay["run_id"], child_run_id);
    assert_eq!(replay["total_events"], 5);
    assert_eq!(replay["is_resumable"], true);

    let export_path = session_dir.path().join("child-export.json");
    let export_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "export",
            child_run_id,
            "--output",
            export_path.to_str().expect("export path utf-8"),
        ])
        .output()
        .expect("run harness sessions export child");
    assert!(
        export_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&export_output.stderr)
    );
    let exported: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&export_path).expect("read child export"))
            .expect("export json should parse");
    assert_eq!(exported["catalog"]["run_id"], child_run_id);
    assert_eq!(exported["events"].as_array().map(Vec::len), Some(5));

    let tree_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "tree",
            "--root",
            "run_child_replay_source",
            "--json",
        ])
        .output()
        .expect("run harness sessions tree child");
    assert!(
        tree_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&tree_output.stderr)
    );
    let tree: serde_json::Value =
        serde_json::from_slice(&tree_output.stdout).expect("tree json should parse");
    assert_eq!(
        tree["harness_lineage"][0]["run_id"],
        "run_child_replay_source"
    );
    assert_eq!(tree["harness_lineage"][1]["run_id"], child_run_id);
    assert_eq!(tree["harness_lineage"][1]["depth"], 1);
}

#[test]
fn sessions_fork_clone_reject_active_or_writer_locked_source() {
    let session_dir = tempdir().expect("tempdir");
    let active_dir = session_dir.path().join("active_source");
    std::fs::create_dir_all(&active_dir).expect("create active source dir");
    write_events_jsonl(
        &active_dir,
        &[envelope(
            "run_active_lineage_source",
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        )],
    );

    let clone_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "clone",
            "--source",
            "run_active_lineage_source",
            "--json",
        ])
        .output()
        .expect("run harness sessions clone active source");
    assert!(!clone_output.status.success());
    let clone_stderr = String::from_utf8_lossy(&clone_output.stderr);
    assert!(clone_stderr.contains("Harness session clone failed"));
    assert!(clone_stderr.contains("no stable completed prefix"));

    let fork_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "fork",
            "--source",
            "run_active_lineage_source",
            "--cutoff",
            "1",
            "--json",
        ])
        .output()
        .expect("run harness sessions fork active source");
    assert!(!fork_output.status.success());
    let fork_stderr = String::from_utf8_lossy(&fork_output.stderr);
    assert!(fork_stderr.contains("Harness session fork failed"));
    assert!(fork_stderr.contains("run is still active"));

    let locked_dir = session_dir.path().join("locked_source");
    std::fs::create_dir_all(&locked_dir).expect("create locked source dir");
    write_events_jsonl(
        &locked_dir,
        &resumable_finished_events("run_locked_lineage_source"),
    );
    std::fs::write(locked_dir.join(".writer.lock"), "locked").expect("write writer lock");

    let locked_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "fork",
            "--source",
            "run_locked_lineage_source",
            "--cutoff",
            "5",
            "--json",
        ])
        .output()
        .expect("run harness sessions fork locked source");
    assert!(!locked_output.status.success());
    let locked_stderr = String::from_utf8_lossy(&locked_output.stderr);
    assert!(locked_stderr.contains("Harness session fork failed"));
    assert!(locked_stderr.contains("actively writer-locked"));

    let entries = std::fs::read_dir(session_dir.path())
        .expect("read session dir")
        .map(|entry| entry.expect("dir entry").file_name())
        .collect::<Vec<_>>();
    assert_eq!(
        entries.len(),
        2,
        "no child run should be published on rejection"
    );
}

#[test]
fn sessions_child_replay_and_continue_readiness_survive_parent_movement() {
    let session_dir = tempdir().expect("tempdir");
    let source_dir = session_dir.path().join("movable_source");
    std::fs::create_dir_all(&source_dir).expect("create source run dir");
    write_events_jsonl(
        &source_dir,
        &resumable_finished_events("run_movable_parent"),
    );

    let fork_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "fork",
            "--source",
            "run_movable_parent",
            "--cutoff",
            "5",
            "--json",
        ])
        .output()
        .expect("run harness sessions fork json");
    assert!(
        fork_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&fork_output.stderr)
    );
    let forked: serde_json::Value =
        serde_json::from_slice(&fork_output.stdout).expect("fork json should parse");
    let child_run_id = forked["child_run_id"].as_str().expect("child run id");

    let moved_parent_dir = tempdir().expect("moved parent tempdir");
    let moved_parent = moved_parent_dir.path().join("moved_parent");
    std::fs::rename(&source_dir, &moved_parent).expect("move parent outside session catalog");

    let replay_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "replay",
            child_run_id,
            "--json",
        ])
        .output()
        .expect("run harness sessions replay moved-parent child");
    assert!(
        replay_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&replay_output.stderr)
    );
    let replay: serde_json::Value =
        serde_json::from_slice(&replay_output.stdout).expect("replay json should parse");
    assert_eq!(replay["run_id"], child_run_id);
    assert_eq!(replay["is_resumable"], true);

    let reopen_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "reopen",
            "--session",
            child_run_id,
            "--json",
        ])
        .output()
        .expect("run harness sessions reopen moved-parent child");
    assert!(
        reopen_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&reopen_output.stderr)
    );
    let recovery: serde_json::Value =
        serde_json::from_slice(&reopen_output.stdout).expect("reopen json should parse");
    assert_eq!(recovery["run_id"], child_run_id);
    assert_eq!(recovery["resumable"], true);
    assert!(recovery["continue_hint"]
        .as_str()
        .expect("continue hint")
        .contains(child_run_id));

    std::fs::remove_dir_all(&moved_parent).expect("delete moved parent");
    let tree_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "tree",
            "--json",
        ])
        .output()
        .expect("run harness sessions tree after parent deletion");
    assert!(
        tree_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&tree_output.stderr)
    );
    let tree: serde_json::Value =
        serde_json::from_slice(&tree_output.stdout).expect("tree json should parse");
    assert_eq!(tree["session_count"], 1);
    assert_eq!(tree["harness_lineage"][0]["run_id"], child_run_id);
    assert_eq!(tree["harness_lineage"][0]["depth"], 0);
}

#[test]
fn sessions_tree_renders_deep_lineage_deterministically() {
    let session_dir = tempdir().expect("tempdir");
    let chain = [
        ("run_deep_root", None),
        ("run_deep_child", Some("run_deep_root")),
        ("run_deep_grandchild", Some("run_deep_child")),
        ("run_deep_great_grandchild", Some("run_deep_grandchild")),
        ("run_deep_leaf", Some("run_deep_great_grandchild")),
    ];
    for (run_id, parent) in chain {
        let run_dir = session_dir.path().join(run_id);
        std::fs::create_dir_all(&run_dir).expect("create run dir");
        write_events_jsonl(&run_dir, &resumable_finished_events(run_id));
        if let Some(parent) = parent {
            write_harness_lineage_meta(&run_dir, run_id, parent);
        }
    }

    let json_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "tree",
            "--json",
        ])
        .output()
        .expect("run harness sessions tree deep json");
    assert!(
        json_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&json_output.stderr)
    );
    let tree: serde_json::Value =
        serde_json::from_slice(&json_output.stdout).expect("tree json should parse");
    let rows = tree["harness_lineage"].as_array().expect("tree rows");
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0]["run_id"], "run_deep_root");
    assert_eq!(rows[1]["run_id"], "run_deep_child");
    assert_eq!(rows[2]["run_id"], "run_deep_grandchild");
    assert_eq!(rows[3]["run_id"], "run_deep_great_grandchild");
    assert_eq!(rows[4]["run_id"], "run_deep_leaf");
    assert_eq!(
        rows.iter()
            .map(|row| row["depth"].as_u64().expect("depth"))
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4]
    );

    let human_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "tree",
        ])
        .output()
        .expect("run harness sessions tree deep human");
    assert!(
        human_output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&human_output.stderr)
    );
    let stdout = String::from_utf8_lossy(&human_output.stdout);
    assert!(stdout.contains("Harness session lineage"));
    assert!(stdout.contains("        - run_deep_leaf status=finished"));
}

#[test]
fn sessions_fork_rejects_invalid_cutoff() {
    let session_dir = tempdir().expect("tempdir");
    let source_dir = session_dir.path().join("unstable_source");
    std::fs::create_dir_all(&source_dir).expect("create source run dir");
    write_events_jsonl(
        &source_dir,
        &[
            envelope(
                "run_unstable_source",
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/tmp/workspace".to_string(),
                }),
            ),
            envelope(
                "run_unstable_source",
                2,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_in_flight".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("provider_model:default:gpt-5.4-mini".to_string()),
                }),
            ),
            envelope(
                "run_unstable_source",
                3,
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
            "fork",
            "--source",
            "run_unstable_source",
            "--cutoff",
            "2",
            "--json",
        ])
        .output()
        .expect("run harness sessions fork invalid cutoff");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Harness session fork failed"));
    assert!(stderr.contains("prefix ending at seq 2 is unstable"));
    assert!(stderr.contains("tasks are still in flight: task_in_flight"));
}

#[test]
fn sessions_fork_clone_reject_invalid_source_selector() {
    let session_dir = tempdir().expect("tempdir");

    let fork_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "fork",
            "--source",
            "missing_lineage_source",
            "--cutoff",
            "1",
            "--json",
        ])
        .output()
        .expect("run harness sessions fork invalid source");
    assert!(!fork_output.status.success());
    let fork_stderr = String::from_utf8_lossy(&fork_output.stderr);
    assert!(fork_stderr.contains("Harness session fork failed"));
    assert!(fork_stderr.contains("no saved session matched `missing_lineage_source`"));

    let clone_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "clone",
            "--source",
            "missing_lineage_source",
            "--json",
        ])
        .output()
        .expect("run harness sessions clone invalid source");
    assert!(!clone_output.status.success());
    let clone_stderr = String::from_utf8_lossy(&clone_output.stderr);
    assert!(clone_stderr.contains("Harness session clone failed"));
    assert!(clone_stderr.contains("no saved session matched `missing_lineage_source`"));
}

#[test]
fn sessions_fork_clone_reject_ambiguous_source_selector() {
    let session_dir = tempdir().expect("tempdir");
    for run_dir_name in ["ambiguous_source_a", "ambiguous_source_b"] {
        let run_dir = session_dir.path().join(run_dir_name);
        std::fs::create_dir_all(&run_dir).expect("create source run dir");
        write_events_jsonl(&run_dir, &resumable_finished_events("run_ambiguous_source"));
    }

    let fork_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "fork",
            "--source",
            "run_ambiguous_source",
            "--cutoff",
            "5",
            "--json",
        ])
        .output()
        .expect("run harness sessions fork ambiguous source");
    assert!(!fork_output.status.success());
    let fork_stderr = String::from_utf8_lossy(&fork_output.stderr);
    assert!(fork_stderr.contains("Harness session fork failed"));
    assert!(fork_stderr.contains("multiple saved sessions matched `run_ambiguous_source`"));

    let clone_output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "clone",
            "--source",
            "run_ambiguous_source",
            "--json",
        ])
        .output()
        .expect("run harness sessions clone ambiguous source");
    assert!(!clone_output.status.success());
    let clone_stderr = String::from_utf8_lossy(&clone_output.stderr);
    assert!(clone_stderr.contains("Harness session clone failed"));
    assert!(clone_stderr.contains("multiple saved sessions matched `run_ambiguous_source`"));
}

#[test]
fn sessions_fork_rejects_cutoff_beyond_log() {
    let session_dir = tempdir().expect("tempdir");
    let source_dir = session_dir.path().join("short_source");
    std::fs::create_dir_all(&source_dir).expect("create source run dir");
    write_events_jsonl(
        &source_dir,
        &resumable_finished_events("run_short_lineage_source"),
    );

    let output = Command::new(env!("CARGO_BIN_EXE_harness"))
        .args([
            "--session-dir",
            session_dir.path().to_str().expect("session dir utf-8"),
            "sessions",
            "fork",
            "--source",
            "run_short_lineage_source",
            "--cutoff",
            "99",
            "--json",
        ])
        .output()
        .expect("run harness sessions fork cutoff beyond log");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Harness session fork failed"));
    assert!(stderr.contains("stable prefix cutoff seq 99 is outside event log range 0..=5"));
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

fn resumable_finished_events(run_id: &str) -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            run_id,
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            run_id,
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_1".to_string(),
                profile: "worker".to_string(),
                parent_agent_id: None,
            }),
        ),
        agent_envelope(
            run_id,
            3,
            "agent_1",
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_1".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "continue safely".to_string(),
                request_digest: "digest-request".to_string(),
                metadata: None,
            }),
        ),
        agent_envelope(
            run_id,
            4,
            "agent_1",
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_1".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        envelope(
            run_id,
            5,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ]
}

fn write_harness_lineage_meta(run_dir: &std::path::Path, run_id: &str, parent_run_id: &str) {
    std::fs::write(
        run_dir.join("meta.json"),
        format!(
            "{}\n",
            serde_json::to_string_pretty(&serde_json::json!({
                "run_id": run_id,
                "run_name": format!("Harness child of {parent_run_id}"),
                "workspace_root": "/tmp/workspace",
                "created_at": "1710000000000",
                "config_digest": "test-digest",
                "harness_version": "test",
                "harness_lineage": {
                    "harness_operation": "child_session_materialization",
                    "harness_source_run_id": parent_run_id,
                    "harness_source_cutoff_seq": 5,
                    "harness_source_digest": "test-source-digest"
                }
            }))
            .expect("serialize harness lineage metadata")
        ),
    )
    .expect("write harness lineage metadata");
}

fn delegated_recovery_events(run_id: &str) -> Vec<EventEnvelopeV1> {
    let lineage = TaskLineageMetadata {
        parent_tool_call_id: Some("toolcall_1".to_string()),
        parent_task_id: Some("task_1".to_string()),
        parent_request_id: Some("req_1".to_string()),
        parent_session_id: Some("agent_supervisor".to_string()),
        child_session_id: Some("child-run-001".to_string()),
        child_request_id: Some("child-req-001".to_string()),
        child_provider_id: Some("openai".to_string()),
        child_model_id: Some("gpt-5.4-mini".to_string()),
    };
    let tool_metadata = ToolCallMetadata {
        canonical_tool_id: Some("agent.spawn".to_string()),
        alias_source_tool_id: Some("task".to_string()),
        lineage: Some(lineage.clone()),
        artifact_refs: vec![EventArtifactRef {
            path: "artifacts/delegated/task-output.json".to_string(),
            digest: Some("artifact-digest-001".to_string()),
        }],
        timing: None,
        hook_executions: Vec::new(),
    };

    vec![
        envelope(
            run_id,
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "recovery-fixture".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            run_id,
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "child-run-001".to_string(),
                profile: "worker".to_string(),
                parent_agent_id: Some("agent_supervisor".to_string()),
            }),
        ),
        envelope(
            run_id,
            3,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "toolcall_1".to_string(),
                tool_id: "task".to_string(),
                args_summary: "delegate".to_string(),
                args_digest: "args-digest-001".to_string(),
                metadata: Some(tool_metadata.clone()),
            }),
        ),
        envelope(
            run_id,
            4,
            EventV1::ArtifactWritten(ArtifactWrittenEvent {
                path: "artifacts/delegated/task-output.json".to_string(),
                digest: "artifact-digest-001".to_string(),
                bytes: 42,
                tool_call_id: Some("toolcall_1".to_string()),
                tool_metadata: Some(ToolIdentityMetadata {
                    canonical_tool_id: Some("agent.spawn".to_string()),
                    alias_source_tool_id: Some("task".to_string()),
                }),
                metadata: BTreeMap::new(),
            }),
        ),
        envelope(
            run_id,
            5,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_1".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("delegated".to_string()),
                output_digest: Some("output-digest-001".to_string()),
                output_json: None,
                metadata: Some(tool_metadata),
            }),
        ),
        envelope(
            run_id,
            6,
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_1".to_string(),
                result_summary: "delegated result".to_string(),
                result_digest: "result-digest-001".to_string(),
                metadata: Some(TaskCompletionMetadata {
                    lineage: Some(lineage),
                    route: None,
                    task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                    timing: Some(ExecutionTimingMetadata {
                        started_mono_ms: Some(10),
                        finished_mono_ms: Some(25),
                        elapsed_ms: Some(15),
                    }),
                    hook_executions: Vec::new(),
                }),
            }),
        ),
        envelope(
            run_id,
            7,
            EventV1::BackgroundTaskNotification(BackgroundTaskNotificationEvent {
                parent_session_id: "agent_supervisor".to_string(),
                parent_agent_id: Some("agent_supervisor".to_string()),
                child_session_id: "child-run-001".to_string(),
                child_request_id: "child-req-001".to_string(),
                task_id: "task_1".to_string(),
                description: "delegate".to_string(),
                status: BackgroundTaskNotificationStatus::Completed,
                summary: "background child completed".to_string(),
                terminal_event_id: "evt-0006".to_string(),
                terminal_task_id: "task_1".to_string(),
                delivered_turn_request_id: Some("parent-notice-req".to_string()),
            }),
        ),
        envelope(
            run_id,
            8,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ]
}

fn delegated_recovery_events_with_control_chars(run_id: &str) -> Vec<EventEnvelopeV1> {
    let child_session_id = "child-run-001\n\tcontrol".to_string();
    let parent_tool_call_id = "toolcall_parent\rcontrol".to_string();
    let artifact_path = "artifacts/delegated/task-output\n.json".to_string();
    let lineage = TaskLineageMetadata {
        parent_tool_call_id: Some(parent_tool_call_id.clone()),
        parent_task_id: Some("task_1".to_string()),
        parent_request_id: Some("req_1".to_string()),
        parent_session_id: Some("agent_supervisor".to_string()),
        child_session_id: Some(child_session_id.clone()),
        child_request_id: Some("child-req-001".to_string()),
        child_provider_id: Some("openai".to_string()),
        child_model_id: Some("gpt-5.4-mini".to_string()),
    };
    let tool_metadata = ToolCallMetadata {
        canonical_tool_id: Some("agent.spawn".to_string()),
        alias_source_tool_id: Some("task".to_string()),
        lineage: Some(lineage.clone()),
        artifact_refs: vec![EventArtifactRef {
            path: artifact_path.clone(),
            digest: Some("artifact-digest-002".to_string()),
        }],
        timing: None,
        hook_executions: Vec::new(),
    };

    vec![
        envelope(
            run_id,
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "recovery-fixture".to_string(),
                workspace_root: "/tmp/workspace".to_string(),
            }),
        ),
        envelope(
            run_id,
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: child_session_id.clone(),
                profile: "worker".to_string(),
                parent_agent_id: Some("agent_supervisor".to_string()),
            }),
        ),
        envelope(
            run_id,
            3,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "toolcall_1".to_string(),
                tool_id: "task".to_string(),
                args_summary: "delegate".to_string(),
                args_digest: "args-digest-002".to_string(),
                metadata: Some(tool_metadata.clone()),
            }),
        ),
        envelope(
            run_id,
            4,
            EventV1::ArtifactWritten(ArtifactWrittenEvent {
                path: artifact_path.clone(),
                digest: "artifact-digest-002".to_string(),
                bytes: 42,
                tool_call_id: Some("toolcall_1".to_string()),
                tool_metadata: Some(ToolIdentityMetadata {
                    canonical_tool_id: Some("agent.spawn".to_string()),
                    alias_source_tool_id: Some("task".to_string()),
                }),
                metadata: BTreeMap::new(),
            }),
        ),
        envelope(
            run_id,
            5,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_1".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("delegated".to_string()),
                output_digest: Some("output-digest-002".to_string()),
                output_json: None,
                metadata: Some(tool_metadata),
            }),
        ),
        envelope(
            run_id,
            6,
            EventV1::TaskCompleted(TaskCompletedEvent {
                task_id: "task_1".to_string(),
                result_summary: "delegated result".to_string(),
                result_digest: "result-digest-002".to_string(),
                metadata: Some(TaskCompletionMetadata {
                    lineage: Some(lineage),
                    route: None,
                    task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                    timing: Some(ExecutionTimingMetadata {
                        started_mono_ms: Some(10),
                        finished_mono_ms: Some(25),
                        elapsed_ms: Some(15),
                    }),
                    hook_executions: Vec::new(),
                }),
            }),
        ),
        envelope(
            run_id,
            7,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ]
}
