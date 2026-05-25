use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use harness_core::event::{
    ActorKind, AgentSpawnedEvent, ArtifactWrittenEvent, EventActor, EventEnvelopeV1, EventV1,
    PermissionDecision, PermissionGrantRecordedEvent, PermissionRequestedEvent,
    PermissionResolvedEvent, ProviderAssistantMessageMetadata, ProviderRequestFinishedEvent,
    ProviderRequestFinishedMetadata, ProviderRequestStartedEvent, ProviderRequestStartedMetadata,
    ProviderThinkingMetadata, RunFinishedEvent, RunStartedEvent, TaskCompletedEvent,
    TaskLineageMetadata, TaskScheduleState, TaskScheduledEvent, ToolCallFinishedEvent,
    ToolCallLifecycleState, ToolCallMetadata, ToolCallRequestedEvent, ToolCallStartedEvent,
    ToolCallStatus, UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::perm::{
    PermissionGrant, PermissionGrantMatcher, PermissionGrantRequest, PermissionGrantScope,
    PermissionKind, PermissionToolSelector,
};
use harness_core::proj::{
    inspect_resume_plan, project_run_summary, project_session_catalog_entry,
    LifecycleSegmentStatus, RunStatus,
};

#[path = "mod.rs"]
mod common;
use common::load_events;

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
