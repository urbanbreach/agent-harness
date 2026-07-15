// allow: SIZE_OK — transcript projection (pure replay state derivation)
use std::collections::BTreeMap;

use crate::event::{
    ArtifactWrittenEvent, EventEnvelopeV1, PermissionDecision, ProviderAssistantMessageMetadata,
    TaskLineageMetadata, ToolCallMetadata,
};
use crate::text::non_empty_trimmed;

use super::model::{
    ArtifactProjectionSource, ProjectedMessage, ProjectedMessageRole, ProjectedMessageState,
    ProjectedPart, ProjectedPermissionPart, ProjectedPermissionState,
    ProjectedProviderMessageMetadata, ProjectedTaskPart, ProjectedTextPart, ProjectedToolCallPart,
    ProjectedToolCallState, ProvenanceRange, SessionLineageProjection, TranscriptArtifactRef,
    TranscriptProjection, TranscriptProjectionError,
};

#[derive(Debug, Clone, Default)]
pub(super) struct RequestLocations {
    pub(super) user_message_index: Option<usize>,
    pub(super) assistant_message_index: Option<usize>,
    pub(super) assistant_text_part_index: Option<usize>,
    pub(super) assistant_reasoning_part_index: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PartLocation {
    pub(super) message_index: usize,
    pub(super) part_index: usize,
}

pub(super) fn ensure_strict_seq_order(
    events: &[EventEnvelopeV1],
) -> Result<(), TranscriptProjectionError> {
    let mut previous_seq = None;
    for event in events {
        if let Some(previous_seq) = previous_seq {
            if event.seq <= previous_seq {
                return Err(TranscriptProjectionError::EventsOutOfOrder {
                    previous_seq,
                    seq: event.seq,
                });
            }
        }
        previous_seq = Some(event.seq);
    }
    Ok(())
}

pub(super) fn append_system_part(
    projection: &mut TranscriptProjection,
    event: &EventEnvelopeV1,
    part: ProjectedPart,
) -> usize {
    let index = projection.messages.len();
    projection.messages.push(ProjectedMessage {
        message_id: format!("system:{}", event.seq),
        role: ProjectedMessageRole::System,
        state: ProjectedMessageState::Complete,
        request_id: event.correlation_id.clone().map(Into::into),
        agent_id: event.actor.agent_id.clone(),
        provider: None,
        provenance: ProvenanceRange::from_event(event),
        parts: vec![part],
    });
    index
}

pub(super) fn append_task_part(
    projection: &mut TranscriptProjection,
    event: &EventEnvelopeV1,
    part: ProjectedTaskPart,
) {
    append_system_part(projection, event, ProjectedPart::Task(part));
}

pub(super) fn append_part_to_message(
    projection: &mut TranscriptProjection,
    message_index: usize,
    part: ProjectedPart,
    event: &EventEnvelopeV1,
) -> usize {
    let message = &mut projection.messages[message_index];
    message.provenance.extend(event);
    let part_index = message.parts.len();
    message.parts.push(part);
    part_index
}

pub(super) fn ensure_assistant_message(
    projection: &mut TranscriptProjection,
    request_locations: &mut BTreeMap<String, RequestLocations>,
    event: &EventEnvelopeV1,
    request_id: &str,
) -> usize {
    if let Some(index) = request_locations
        .get(request_id)
        .and_then(|locations| locations.assistant_message_index)
    {
        return index;
    }

    let index = projection.messages.len();
    projection.messages.push(ProjectedMessage {
        message_id: format!("assistant:{request_id}"),
        role: ProjectedMessageRole::Assistant,
        state: ProjectedMessageState::Incomplete,
        request_id: Some(request_id.into()),
        agent_id: event.actor.agent_id.clone(),
        provider: None,
        provenance: ProvenanceRange::from_event(event),
        parts: Vec::new(),
    });
    request_locations
        .entry(request_id.to_string())
        .or_default()
        .assistant_message_index = Some(index);
    index
}

#[derive(Debug, Clone, Copy)]
pub(super) enum AssistantTextKind {
    Text,
    Reasoning,
}

pub(super) fn append_or_extend_assistant_text(
    projection: &mut TranscriptProjection,
    request_locations: &mut BTreeMap<String, RequestLocations>,
    event: &EventEnvelopeV1,
    request_id: &str,
    delta: &str,
    kind: AssistantTextKind,
) {
    let message_index = ensure_assistant_message(projection, request_locations, event, request_id);
    let existing_part_index = match kind {
        AssistantTextKind::Text => request_locations
            .get(request_id)
            .and_then(|locations| locations.assistant_text_part_index),
        AssistantTextKind::Reasoning => request_locations
            .get(request_id)
            .and_then(|locations| locations.assistant_reasoning_part_index),
    };

    projection.messages[message_index].state = ProjectedMessageState::Streaming;
    projection.messages[message_index].provenance.extend(event);

    if let Some(part_index) = existing_part_index {
        if let Some(text_part) = text_part_mut(&mut projection.messages[message_index], part_index)
        {
            text_part.text.push_str(delta);
            text_part.provenance.extend(event);
            return;
        }
    }

    let part = ProjectedTextPart {
        text: delta.to_string(),
        provenance: ProvenanceRange::from_event(event),
    };
    let part_index = projection.messages[message_index].parts.len();
    projection.messages[message_index].parts.push(match kind {
        AssistantTextKind::Text => ProjectedPart::Text(part),
        AssistantTextKind::Reasoning => ProjectedPart::Reasoning(part),
    });
    let locations = request_locations.entry(request_id.to_string()).or_default();
    match kind {
        AssistantTextKind::Text => locations.assistant_text_part_index = Some(part_index),
        AssistantTextKind::Reasoning => locations.assistant_reasoning_part_index = Some(part_index),
    }
}

pub(super) fn provider_turn_request_id(
    event: &EventEnvelopeV1,
    provider_request_id: &str,
) -> String {
    event
        .correlation_id
        .as_deref()
        .and_then(non_empty_trimmed)
        .unwrap_or(provider_request_id)
        .to_string()
}

pub(super) fn apply_assistant_message_metadata(
    provider: &mut ProjectedProviderMessageMetadata,
    metadata: &ProviderAssistantMessageMetadata,
) {
    provider.assistant_message_id = metadata.message_id.clone();
    provider.assistant_text_digest = metadata.text_digest.clone();
    provider.assistant_reasoning_digest = metadata.reasoning_digest.clone();
}

fn text_part_mut(
    message: &mut ProjectedMessage,
    part_index: usize,
) -> Option<&mut ProjectedTextPart> {
    match message.parts.get_mut(part_index) {
        Some(ProjectedPart::Text(part)) | Some(ProjectedPart::Reasoning(part)) => Some(part),
        _ => None,
    }
}

pub(super) fn tool_call_part_mut(
    projection: &mut TranscriptProjection,
    location: PartLocation,
) -> Option<&mut ProjectedToolCallPart> {
    match projection
        .messages
        .get_mut(location.message_index)?
        .parts
        .get_mut(location.part_index)?
    {
        ProjectedPart::ToolCall(part) => Some(part),
        _ => None,
    }
}

pub(super) fn permission_part_mut(
    projection: &mut TranscriptProjection,
    location: PartLocation,
) -> Option<&mut ProjectedPermissionPart> {
    match projection
        .messages
        .get_mut(location.message_index)?
        .parts
        .get_mut(location.part_index)?
    {
        ProjectedPart::Permission(part) => Some(part),
        _ => None,
    }
}

pub(super) fn update_tool_permission_resolution(
    projection: &mut TranscriptProjection,
    permission_id: &str,
    decision: PermissionDecision,
    reason: Option<String>,
    event: &EventEnvelopeV1,
) {
    for message in &mut projection.messages {
        let mut updated_message = false;
        for part in &mut message.parts {
            let ProjectedPart::ToolCall(tool_call) = part else {
                continue;
            };
            for permission in &mut tool_call.permissions {
                if permission.permission_id == permission_id {
                    permission.state = ProjectedPermissionState::Resolved;
                    permission.decision = Some(decision);
                    permission.reason = reason.clone();
                    permission.provenance.extend(event);
                    tool_call.provenance.extend(event);
                    updated_message = true;
                }
            }
        }
        if updated_message {
            message.provenance.extend(event);
        }
    }
}

pub(super) fn placeholder_tool_call_part(
    tool_call_id: &str,
    state: ProjectedToolCallState,
    event: &EventEnvelopeV1,
) -> ProjectedToolCallPart {
    ProjectedToolCallPart {
        tool_call_id: tool_call_id.into(),
        tool_id: String::new(),
        args_summary: String::new(),
        args_digest: String::new(),
        state,
        status: None,
        output_summary: None,
        output_digest: None,
        output_json: None,
        requested_seq: None,
        started_seq: matches!(state, ProjectedToolCallState::Running).then_some(event.seq),
        finished_seq: None,
        metadata: None,
        permissions: Vec::new(),
        artifacts: Vec::new(),
        lineage: None,
        provenance: ProvenanceRange::from_event(event),
    }
}

pub(super) fn artifact_from_written(
    payload: &ArtifactWrittenEvent,
    event: &EventEnvelopeV1,
) -> TranscriptArtifactRef {
    TranscriptArtifactRef {
        path: payload.path.clone(),
        digest: Some(payload.digest.clone()),
        bytes: Some(payload.bytes),
        tool_call_id: payload.tool_call_id.clone(),
        source: ArtifactProjectionSource::ArtifactWritten,
        metadata: payload.metadata.clone(),
        provenance: ProvenanceRange::from_event(event),
    }
}

pub(super) fn artifacts_from_tool_metadata(
    tool_call_id: &str,
    metadata: Option<&ToolCallMetadata>,
    event: &EventEnvelopeV1,
) -> Vec<TranscriptArtifactRef> {
    metadata
        .map(|metadata| {
            metadata
                .artifact_refs
                .iter()
                .map(|artifact| TranscriptArtifactRef {
                    path: artifact.path.clone(),
                    digest: artifact.digest.clone(),
                    bytes: None,
                    tool_call_id: Some(tool_call_id.into()),
                    source: ArtifactProjectionSource::ToolCallMetadata,
                    metadata: BTreeMap::new(),
                    provenance: ProvenanceRange::from_event(event),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn push_unique_artifact(
    artifacts: &mut Vec<TranscriptArtifactRef>,
    artifact: TranscriptArtifactRef,
) {
    if artifacts.iter().any(|existing| {
        existing.path == artifact.path
            && existing.digest == artifact.digest
            && existing.tool_call_id == artifact.tool_call_id
    }) {
        return;
    }
    artifacts.push(artifact);
}

pub(super) fn lineage_projection(
    lineage: Option<&TaskLineageMetadata>,
    event: &EventEnvelopeV1,
) -> Option<SessionLineageProjection> {
    let lineage = lineage?;
    let has_any = lineage.parent_tool_call_id.is_some()
        || lineage.parent_task_id.is_some()
        || lineage.parent_request_id.is_some()
        || lineage.parent_session_id.is_some()
        || lineage.child_session_id.is_some()
        || lineage.child_request_id.is_some()
        || lineage.child_provider_id.is_some()
        || lineage.child_model_id.is_some();
    if !has_any {
        return None;
    }
    Some(SessionLineageProjection {
        parent_tool_call_id: lineage.parent_tool_call_id.clone(),
        parent_task_id: lineage.parent_task_id.clone(),
        parent_request_id: lineage.parent_request_id.clone(),
        parent_session_id: lineage.parent_session_id.clone(),
        child_session_id: lineage.child_session_id.clone(),
        child_request_id: lineage.child_request_id.clone(),
        child_provider_id: lineage.child_provider_id.clone(),
        child_model_id: lineage.child_model_id.clone(),
        provenance: ProvenanceRange::from_event(event),
    })
}

pub(super) fn push_unique_lineage(
    lineages: &mut Vec<SessionLineageProjection>,
    lineage: SessionLineageProjection,
) {
    if lineages
        .iter()
        .any(|existing| same_lineage(existing, &lineage))
    {
        return;
    }
    lineages.push(lineage);
}

fn same_lineage(left: &SessionLineageProjection, right: &SessionLineageProjection) -> bool {
    left.parent_tool_call_id == right.parent_tool_call_id
        && left.parent_task_id == right.parent_task_id
        && left.parent_request_id == right.parent_request_id
        && left.parent_session_id == right.parent_session_id
        && left.child_session_id == right.child_session_id
        && left.child_request_id == right.child_request_id
        && left.child_provider_id == right.child_provider_id
        && left.child_model_id == right.child_model_id
}
