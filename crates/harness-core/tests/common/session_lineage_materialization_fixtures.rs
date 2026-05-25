use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::thread;

use harness_core::event::{
    ActorKind, AgentSpawnedEvent, ArtifactWrittenEvent, EventActor, EventArtifactRef,
    EventEnvelopeV1, EventV1, PermissionDecision, PermissionRequestedEvent,
    ProviderRequestStartedEvent, RunFinishedEvent, RunStartedEvent, TaskScheduleState,
    TaskScheduledEvent, ToolCallFinishedEvent, ToolCallMetadata, ToolCallRequestedEvent,
    ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::proj::inspect_resume_plan;
use harness_core::session_lineage::{
    materialize_child_session, validate_fork_stable_prefix, validate_tui_fork_stable_prefix,
    ChildSessionMaterializationError, ChildSessionMaterializationRequest,
    ChildSessionMaterializationSourceKind,
};

#[path = "mod.rs"]
mod common;
use common::load_events;

fn stable_events(
    run_id: &str,
    artifact_path: &str,
    artifact_digest: &str,
    artifact_bytes: usize,
) -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            run_id,
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "parent".to_string(),
                workspace_root: "/workspace/source".to_string(),
            }),
        ),
        envelope(
            run_id,
            2,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "toolcall_000001".to_string(),
                tool_id: "bash".to_string(),
                args_summary: "{}".to_string(),
                args_digest: "argsdigest".to_string(),
                metadata: Some(ToolCallMetadata {
                    artifact_refs: vec![EventArtifactRef {
                        path: artifact_path.to_string(),
                        digest: Some(artifact_digest.to_string()),
                    }],
                    ..ToolCallMetadata::default()
                }),
            }),
        ),
        envelope(
            run_id,
            3,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "toolcall_000001".to_string(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("wrote artifact".to_string()),
                output_digest: Some("outputdigest".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        envelope(
            run_id,
            4,
            EventV1::ArtifactWritten(ArtifactWrittenEvent {
                path: artifact_path.to_string(),
                digest: artifact_digest.to_string(),
                bytes: artifact_bytes as u64,
                tool_call_id: Some("toolcall_000001".to_string()),
                tool_metadata: None,
                metadata: BTreeMap::new(),
            }),
        ),
        envelope(
            run_id,
            5,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "finished".to_string(),
            }),
        ),
    ]
}

fn live_snapshot_with_open_state(run_id: &str) -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            run_id,
            1,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "interactive".to_string(),
                workspace_root: "/workspace/source".to_string(),
            }),
        ),
        envelope(
            run_id,
            2,
            EventV1::AgentSpawned(AgentSpawnedEvent {
                agent_id: "agent_000001".to_string(),
                profile: "default".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope(
            run_id,
            3,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_000001".to_string(),
                text: "edit file".to_string(),
            }),
        ),
        envelope(
            run_id,
            4,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_000001".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-5".to_string(),
                prompt_summary: "edit file".to_string(),
                request_digest: "digest-req".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            run_id,
            5,
            EventV1::TaskScheduled(TaskScheduledEvent {
                task_id: "task_000001".to_string(),
                state: TaskScheduleState::Started,
                queue_key: Some("tool:edit".to_string()),
            }),
        ),
        envelope(
            run_id,
            6,
            EventV1::PermissionRequested(PermissionRequestedEvent {
                permission_id: "perm_000001".to_string(),
                kind: "edit".to_string(),
                tool_call_id: Some("toolcall_000001".to_string()),
                summary: "allow edit".to_string(),
                request_digest: "digest-perm".to_string(),
                timeout_ms: 60_000,
                default_decision: PermissionDecision::Deny,
            }),
        ),
    ]
}

fn envelope(run_id: &str, seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-parent-{seq:04}"),
        seq,
        run_id: run_id.to_string(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        correlation_id: Some("corr-parent".to_string()),
        causation_id: Some("evt-parent-cause".to_string()),
        stream_key: Some(format!("run:{run_id}")),
        payload,
    }
}

fn write_source_artifact(source_run_dir: &Path, artifact_path: &str, contents: &[u8]) {
    let path = source_run_dir.join(artifact_path);
    fs::create_dir_all(path.parent().expect("artifact parent")).expect("create artifact parent");
    fs::write(path, contents).expect("write source artifact");
}

fn write_source_events(source_run_dir: &Path, events: &[EventEnvelopeV1]) {
    let body = events
        .iter()
        .map(|event| serde_json::to_string(event).expect("serialize source event"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(source_run_dir.join("events.jsonl"), format!("{body}\n"))
        .expect("write source events");
}

fn read_events(run_dir: &Path) -> Vec<EventEnvelopeV1> {
    load_events(&run_dir.join("events.jsonl"))
}

fn source_prefix_digest(events: &[EventEnvelopeV1]) -> String {
    let mut bytes = Vec::new();
    for event in events {
        serde_json::to_writer(&mut bytes, event).expect("serialize source event");
        bytes.push(b'\n');
    }
    blake3::hash(&bytes).to_hex().to_string()
}

fn session_dir_entries(session_dir: &Path) -> Vec<String> {
    let mut entries = fs::read_dir(session_dir)
        .expect("read session dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

fn assert_no_unpublished_temp_dirs(session_dir: &Path) {
    for entry in fs::read_dir(session_dir).expect("read session dir") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name();
        let name = name.to_string_lossy();
        assert!(
            !(name.starts_with(".run_harness_child") && name.contains(".tmp-")),
            "unpublished temp dir remained: {name}"
        );
    }
}
