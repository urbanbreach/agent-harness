use harness_core::UnwrapOrAbort;
use std::collections::BTreeMap;

use harness_core::event::{
    ActorKind, AgentSpawnedEvent, AgentStoppedEvent, ArtifactWrittenEvent,
    AssistantMessageFinishedEvent, CompactionAppliedEvent, CompactionFailedEvent,
    CompactionRequestedEvent, CompactionWrittenEvent, EventActor, EventArtifactRef,
    EventEnvelopeV1, EventV1, PermissionDecision, PermissionRequestedEvent,
    PermissionResolvedEvent, PolicyViolationDetectedEvent, ProviderReasoningDeltaEvent,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, ProviderStreamDeltaEvent,
    RunFailedEvent, RunFinishedEvent, RunStartedEvent, TaskCancelledEvent, TaskCompletedEvent,
    TaskCompletionMetadata, TaskLineageMetadata, TaskResultLateEvent, TaskScheduleState,
    TaskScheduledEvent, ToolCallFinishedEvent, ToolCallMetadata,
    ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UiIntentReceivedEvent,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use harness_core::transcript_projection::{
    project_transcript, ArtifactProjectionSource, CompactionCheckpointStatus, ProjectedMessageRole,
    ProjectedPart, ProjectedPermissionState, ProjectedTaskState, ProjectedToolCallState,
    TranscriptProjectionError, TranscriptRunStatus,
};

fn assistant_message<'a>(
    projection: &'a harness_core::transcript_projection::TranscriptProjection,
    request_id: &str,
) -> &'a harness_core::transcript_projection::ProjectedMessage {
    projection
        .messages
        .iter()
        .find(|message| {
            message.role == ProjectedMessageRole::Assistant
                && message.request_id.as_ref().map(|r| r.as_str()) == Some(request_id)
        })
        .unwrap_or_abort()
}

fn tool_requested(
    seq: u64,
    request_id: &str,
    tool_call_id: &str,
    tool_id: &str,
    args_summary: &str,
    metadata: Option<ToolCallMetadata>,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        worker(),
        Some(request_id),
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.into(),
            tool_id: tool_id.to_string(),
            args_summary: args_summary.to_string(),
            args_digest: format!("digest-{tool_call_id}"),
            metadata,
        }),
    )
}

fn supervisor() -> EventActor {
    EventActor::new(ActorKind::Supervisor, Some("coordinator".to_string()))
}

fn system() -> EventActor {
    EventActor::new(ActorKind::System, Some("coordinator".to_string()))
}

fn worker() -> EventActor {
    EventActor::new(ActorKind::Worker, Some("agent_000001".to_string()))
}

fn user() -> EventActor {
    EventActor::new(ActorKind::User, None)
}

fn envelope(
    seq: u64,
    actor: EventActor,
    correlation_id: Option<&str>,
    payload: EventV1,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq:020}"),
        seq,
        run_id: "run_transcript_projection".into(),
        mono_ms: seq,
        ts: None,
        actor,
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: None,
        payload,
    }
}
