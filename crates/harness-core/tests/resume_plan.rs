use std::fs;
use std::path::Path;

use harness_core::event::{
    ActorKind, AgentSpawnedEvent, EventActor, EventEnvelopeV1, EventV1, PermissionDecision,
    PermissionRequestedEvent, PermissionResolvedEvent, ProviderRequestStartedEvent,
    RunFinishedEvent, RunStartedEvent, TaskCompletedEvent, TaskScheduleState, TaskScheduledEvent,
    ToolCallFinishedEvent, ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::proj::{inspect_resume_plan, LifecycleSegmentStatus};

#[test]
fn resume_plan_reconstructs_sequence_and_id_watermarks() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_resume_ok");
    write_events(
        &run_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                3,
                EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                    request_id: "req_000003".to_string(),
                    text: "hello".to_string(),
                }),
            ),
            envelope(
                4,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000004".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5".to_string(),
                    prompt_summary: "prompt".to_string(),
                    request_digest: "digest-req".to_string(),
                }),
            ),
            envelope(
                5,
                EventV1::ToolCallRequested(ToolCallRequestedEvent {
                    tool_call_id: "toolcall_000002".to_string(),
                    tool_id: "shell.run".to_string(),
                    args_summary: "{\"cmd\":\"true\"}".to_string(),
                    args_digest: "digest-tool-args".to_string(),
                }),
            ),
            envelope(
                6,
                EventV1::ToolCallStarted(ToolCallStartedEvent {
                    tool_call_id: "toolcall_000002".to_string(),
                }),
            ),
            envelope(
                7,
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000002".to_string(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("ok".to_string()),
                    output_digest: Some("digest-tool-out".to_string()),
                    output_json: None,
                }),
            ),
            envelope(
                8,
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "perm_000002".to_string(),
                    kind: "shell".to_string(),
                    tool_call_id: Some("toolcall_000002".to_string()),
                    summary: "allow shell".to_string(),
                    request_digest: "digest-perm".to_string(),
                    timeout_ms: 1_000,
                    default_decision: PermissionDecision::Deny,
                }),
            ),
            envelope(
                9,
                EventV1::PermissionResolved(PermissionResolvedEvent {
                    permission_id: "perm_000002".to_string(),
                    decision: PermissionDecision::Allow,
                    reason: Some("approved".to_string()),
                }),
            ),
            envelope(
                10,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000004".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("tool:shell.run".to_string()),
                }),
            ),
            envelope(
                11,
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "task_000004".to_string(),
                    result_summary: "done".to_string(),
                    result_digest: "digest-task".to_string(),
                }),
            ),
            envelope(
                12,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&run_dir);

    assert_eq!(plan.max_seq, 12);
    assert_eq!(plan.id_watermarks.max_request_id, 4);
    assert_eq!(plan.id_watermarks.max_task_id, 4);
    assert_eq!(plan.id_watermarks.max_tool_call_id, 2);
    assert_eq!(plan.id_watermarks.max_permission_id, 2);
    assert_eq!(
        plan.latest_lifecycle_status,
        LifecycleSegmentStatus::Finished
    );
    assert_eq!(
        plan.known_agents.get("agent_000001").map(String::as_str),
        Some("default")
    );
    assert!(plan.pending_permissions.is_empty());
    assert!(plan.tasks_in_flight.is_empty());
    assert_eq!(plan.workspace_root.as_deref(), Some("/workspace/project"));
    assert_eq!(plan.provider_model.as_deref(), Some("default/gpt-5"));
    assert!(plan.is_resumable);
    assert_eq!(plan.resume_disabled_reason, None);
}

#[test]
fn resume_plan_rejects_sessions_with_pending_permissions() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_pending_permission");
    write_events(
        &run_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5".to_string(),
                    prompt_summary: "prompt".to_string(),
                    request_digest: "digest".to_string(),
                }),
            ),
            envelope(
                4,
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "perm_000001".to_string(),
                    kind: "shell".to_string(),
                    tool_call_id: None,
                    summary: "ask".to_string(),
                    request_digest: "digest-perm".to_string(),
                    timeout_ms: 1_000,
                    default_decision: PermissionDecision::Deny,
                }),
            ),
            envelope(
                5,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&run_dir);
    assert!(!plan.is_resumable);
    assert_eq!(
        plan.resume_disabled_reason.as_deref(),
        Some("pending permissions must be resolved")
    );
}

#[test]
fn resume_plan_rejects_sessions_with_tasks_in_flight() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_tasks_in_flight");
    write_events(
        &run_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5".to_string(),
                    prompt_summary: "prompt".to_string(),
                    request_digest: "digest".to_string(),
                }),
            ),
            envelope(
                4,
                EventV1::TaskScheduled(TaskScheduledEvent {
                    task_id: "task_000001".to_string(),
                    state: TaskScheduleState::Started,
                    queue_key: Some("tool:shell.run".to_string()),
                }),
            ),
            envelope(
                5,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&run_dir);
    assert!(!plan.is_resumable);
    assert_eq!(
        plan.resume_disabled_reason.as_deref(),
        Some("tasks are still in flight")
    );
}

#[test]
fn resume_plan_rejects_non_monotonic_or_corrupt_logs() {
    let temp_dir = tempfile::tempdir().expect("tempdir");

    let non_monotonic_dir = temp_dir.path().join("run_non_monotonic");
    write_events(
        &non_monotonic_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/project".to_string(),
                }),
            ),
            envelope(
                3,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "finished".to_string(),
                }),
            ),
        ],
    );

    let non_monotonic_plan = inspect_resume_plan(&non_monotonic_dir);
    assert!(!non_monotonic_plan.is_resumable);
    assert!(non_monotonic_plan
        .resume_disabled_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("corrupt or non-monotonic")));

    let corrupt_dir = temp_dir.path().join("run_corrupt");
    fs::create_dir_all(&corrupt_dir).expect("create run dir");
    let valid_first_line = serde_json::to_string(&envelope(
        1,
        EventV1::RunStarted(RunStartedEvent {
            run_name: "interactive".to_string(),
            workspace_root: "/workspace/project".to_string(),
        }),
    ))
    .expect("serialize first event");
    fs::write(
        corrupt_dir.join("events.jsonl"),
        format!("{valid_first_line}\n{{bad-json}}\n"),
    )
    .expect("write corrupt events");

    let corrupt_plan = inspect_resume_plan(&corrupt_dir);
    assert!(!corrupt_plan.is_resumable);
    assert!(corrupt_plan
        .resume_disabled_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("invalid JSON event")));
}

#[test]
fn resume_plan_uses_latest_lifecycle_segment_instead_of_any_terminal_event() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_latest_segment");
    write_events(
        &run_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/older".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5".to_string(),
                    prompt_summary: "old".to_string(),
                    request_digest: "digest-old".to_string(),
                }),
            ),
            envelope(
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "first segment done".to_string(),
                }),
            ),
            envelope(
                5,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/newer".to_string(),
                }),
            ),
            envelope(
                6,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000002".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                7,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000002".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5".to_string(),
                    prompt_summary: "new".to_string(),
                    request_digest: "digest-new".to_string(),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&run_dir);

    assert_eq!(plan.latest_lifecycle_status, LifecycleSegmentStatus::Active);
    assert_eq!(plan.workspace_root.as_deref(), Some("/workspace/newer"));
    assert_eq!(
        plan.known_agents.keys().cloned().collect::<Vec<_>>(),
        vec!["agent_000002".to_string()]
    );
    assert!(!plan.is_resumable);
    assert_eq!(
        plan.resume_disabled_reason.as_deref(),
        Some("run is still active")
    );
}

#[test]
fn resume_plan_keeps_provider_model_after_open_and_quit_resumed_segment() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let run_dir = temp_dir.path().join("run_open_quit_latest_segment");
    write_events(
        &run_dir,
        &[
            envelope(
                1,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/original".to_string(),
                }),
            ),
            envelope(
                2,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                3,
                EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                    request_id: "req_000001".to_string(),
                    provider_id: "default".to_string(),
                    model_id: "gpt-5".to_string(),
                    prompt_summary: "hello".to_string(),
                    request_digest: "digest-1".to_string(),
                }),
            ),
            envelope(
                4,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "first segment done".to_string(),
                }),
            ),
            envelope(
                5,
                EventV1::RunStarted(RunStartedEvent {
                    run_name: "interactive".to_string(),
                    workspace_root: "/workspace/resumed".to_string(),
                }),
            ),
            envelope(
                6,
                EventV1::AgentSpawned(AgentSpawnedEvent {
                    agent_id: "agent_000001".to_string(),
                    profile: "default".to_string(),
                    parent_agent_id: None,
                }),
            ),
            envelope(
                7,
                EventV1::RunFinished(RunFinishedEvent {
                    summary: "resumed segment quit without prompt".to_string(),
                }),
            ),
        ],
    );

    let plan = inspect_resume_plan(&run_dir);

    assert_eq!(
        plan.latest_lifecycle_status,
        LifecycleSegmentStatus::Finished
    );
    assert_eq!(plan.workspace_root.as_deref(), Some("/workspace/resumed"));
    assert_eq!(
        plan.known_agents.get("agent_000001").map(String::as_str),
        Some("default")
    );
    assert_eq!(plan.provider_model.as_deref(), Some("default/gpt-5"));
    assert!(
        plan.is_resumable,
        "open-and-quit resumed segment should remain resumable"
    );
}

fn envelope(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:04}"),
        seq,
        run_id: "run_resume_fixture".to_string(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: Some("run:run_resume_fixture".to_string()),
        payload,
    }
}

fn write_events(run_dir: &Path, events: &[EventEnvelopeV1]) {
    fs::create_dir_all(run_dir).expect("create run directory");
    let mut body = String::new();
    for event in events {
        let line = serde_json::to_string(event).expect("serialize event line");
        body.push_str(&line);
        body.push('\n');
    }
    fs::write(run_dir.join("events.jsonl"), body).expect("write events file");
}
