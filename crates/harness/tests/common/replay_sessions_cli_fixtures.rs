use std::collections::BTreeMap;
use std::ffi::OsString;
use std::time::SystemTime;

use harness_core::event::{
    ActorKind, AgentSpawnedEvent, ArtifactWrittenEvent, BackgroundTaskNotificationEvent,
    BackgroundTaskNotificationStatus, EventActor, EventArtifactRef, EventEnvelopeV1, EventV1,
    ExecutionTimingMetadata, PermissionDecision, PermissionRequestedEvent,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, RunFailedEvent, RunFinishedEvent,
    RunStartedEvent, TaskCompletedEvent, TaskCompletionMetadata, TaskLineageMetadata,
    TaskScheduleState, TaskScheduledEvent, ToolCallFinishedEvent, ToolCallMetadata,
    ToolCallRequestedEvent, ToolCallStatus, ToolIdentityMetadata, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use tempfile::tempdir;

#[path = "mod.rs"]
mod common;

use common::{CliHarness, CliHarnessOutput};

fn run_harness<I, S>(args: I) -> CliHarnessOutput
where
    I: IntoIterator<Item = S>,
    S: Into<OsString>,
{
    CliHarness::new().args(args).output()
}

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

fn events_modified_unix_ms(run_dir: &std::path::Path) -> u128 {
    events_modified(run_dir)
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("events modified time after epoch")
        .as_millis()
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
    let output = run_harness(args.iter().copied());
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
