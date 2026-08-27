// allow: SIZE_OK — transcript projection (pure replay state derivation)
use std::collections::BTreeMap;

use crate::event::{EventEnvelopeV1, EventV1, TaskScheduleState, ToolCallStatus};
use crate::session::{classify_compatibility_event, AssistantPart};

mod helpers;
mod model;

pub use model::*;

use helpers::{
    append_or_extend_assistant_text, append_part_to_message, append_system_part, append_task_part,
    apply_assistant_message_metadata, artifact_from_written, artifacts_from_tool_metadata,
    ensure_assistant_message, ensure_strict_seq_order, lineage_projection, permission_part_mut,
    placeholder_tool_call_part, provider_turn_request_id, push_unique_artifact,
    push_unique_lineage, tool_call_part_mut, update_tool_permission_resolution, AssistantTextKind,
    PartLocation, RequestLocations,
};

pub fn project_transcript(
    events: &[EventEnvelopeV1],
) -> Result<TranscriptProjection, TranscriptProjectionError> {
    ensure_strict_seq_order(events)?;

    let mut projection = TranscriptProjection::default();
    let mut request_locations = BTreeMap::<String, RequestLocations>::new();
    let mut tool_locations = BTreeMap::<String, PartLocation>::new();
    let mut permission_locations = BTreeMap::<String, PartLocation>::new();
    let mut pending_attachments =
        BTreeMap::<String, Vec<crate::attachment_transport::AttachmentMetadata>>::new();

    for event in events {
        projection
            .session
            .run_id
            .get_or_insert(event.run_id.to_string());
        projection.session.max_seq = Some(event.seq);

        match &event.payload {
            EventV1::RunStarted(payload) => {
                projection.session.run_id = Some(event.run_id.to_string());
                projection.session.run_name = Some(payload.run_name.to_string());
                projection.session.workspace_root = Some(payload.workspace_root.clone());
                projection.session.status = TranscriptRunStatus::Running;
                projection.session.status_reason = None;
                projection.session.started_seq = Some(event.seq);
                projection.session.terminal_seq = None;
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::Lifecycle(ProjectedLifecyclePart {
                        event: LifecycleEventKind::RunStarted,
                        agent_id: event.actor.agent_id.clone(),
                        profile: None,
                        parent_agent_id: None,
                        summary: Some(payload.run_name.to_string()),
                        error: None,
                        reason: None,
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::SessionTitleUpdated(payload) => {
                projection.session.run_name = Some(payload.title.clone());
            }
            EventV1::RunFinished(payload) => {
                projection.session.status = TranscriptRunStatus::Finished;
                projection.session.status_reason = Some(payload.summary.clone());
                projection.session.terminal_seq = Some(event.seq);
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::Lifecycle(ProjectedLifecyclePart {
                        event: LifecycleEventKind::RunFinished,
                        agent_id: event.actor.agent_id.clone(),
                        profile: None,
                        parent_agent_id: None,
                        summary: Some(payload.summary.clone()),
                        error: None,
                        reason: None,
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::RunFailed(payload) => {
                projection.session.status = TranscriptRunStatus::Failed;
                projection.session.status_reason = Some(payload.error.clone());
                projection.session.terminal_seq = Some(event.seq);
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::Lifecycle(ProjectedLifecyclePart {
                        event: LifecycleEventKind::RunFailed,
                        agent_id: event.actor.agent_id.clone(),
                        profile: None,
                        parent_agent_id: None,
                        summary: None,
                        error: Some(payload.error.clone()),
                        reason: None,
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::AgentSpawned(payload) => {
                projection
                    .session
                    .agent_profiles
                    .insert(payload.agent_id.clone(), payload.profile.clone());
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::Lifecycle(ProjectedLifecyclePart {
                        event: LifecycleEventKind::AgentSpawned,
                        agent_id: Some(payload.agent_id.clone()),
                        profile: Some(payload.profile.clone()),
                        parent_agent_id: payload.parent_agent_id.clone(),
                        summary: None,
                        error: None,
                        reason: None,
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::AgentStopped(payload) => {
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::Lifecycle(ProjectedLifecyclePart {
                        event: LifecycleEventKind::AgentStopped,
                        agent_id: Some(payload.agent_id.clone()),
                        profile: None,
                        parent_agent_id: None,
                        summary: None,
                        error: None,
                        reason: Some(payload.reason.clone()),
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::TaskScheduled(payload) => append_task_part(
                &mut projection,
                event,
                ProjectedTaskPart {
                    task_id: payload.task_id.clone(),
                    state: match payload.state {
                        TaskScheduleState::Queued => ProjectedTaskState::Queued,
                        TaskScheduleState::Started => ProjectedTaskState::Started,
                    },
                    queue_key: payload.queue_key.clone(),
                    reason: None,
                    result_summary: None,
                    result_digest: None,
                    lineage: None,
                    provenance: ProvenanceRange::from_event(event),
                },
            ),
            EventV1::TaskCancelled(payload) => append_task_part(
                &mut projection,
                event,
                ProjectedTaskPart {
                    task_id: payload.task_id.clone(),
                    state: ProjectedTaskState::Cancelled,
                    queue_key: None,
                    reason: Some(payload.reason.clone()),
                    result_summary: None,
                    result_digest: None,
                    lineage: None,
                    provenance: ProvenanceRange::from_event(event),
                },
            ),
            EventV1::TaskCompleted(payload) => {
                let lineage = payload
                    .metadata
                    .as_ref()
                    .and_then(|metadata| lineage_projection(metadata.lineage.as_ref(), event));
                if let Some(lineage) = lineage.as_ref() {
                    push_unique_lineage(&mut projection.session_lineage, lineage.clone());
                }
                append_task_part(
                    &mut projection,
                    event,
                    ProjectedTaskPart {
                        task_id: payload.task_id.clone(),
                        state: ProjectedTaskState::Completed,
                        queue_key: None,
                        reason: None,
                        result_summary: Some(payload.result_summary.clone()),
                        result_digest: Some(payload.result_digest.clone()),
                        lineage,
                        provenance: ProvenanceRange::from_event(event),
                    },
                );
            }
            EventV1::TaskResultLate(payload) => append_task_part(
                &mut projection,
                event,
                ProjectedTaskPart {
                    task_id: payload.task_id.clone(),
                    state: ProjectedTaskState::LateResult,
                    queue_key: None,
                    reason: None,
                    result_summary: None,
                    result_digest: Some(payload.result_digest.clone()),
                    lineage: None,
                    provenance: ProvenanceRange::from_event(event),
                },
            ),
            EventV1::UserMessageSubmitted(payload) => {
                let request_id = payload.request_id.to_string();
                let message = ProjectedMessage {
                    message_id: format!("user:{request_id}:{}", event.seq),
                    role: ProjectedMessageRole::User,
                    state: ProjectedMessageState::Complete,
                    request_id: Some(payload.request_id.clone()),
                    agent_id: event.actor.agent_id.clone(),
                    provider: None,
                    provenance: ProvenanceRange::from_event(event),
                    parts: vec![ProjectedPart::Text(ProjectedTextPart {
                        text: payload.text.clone(),
                        provenance: ProvenanceRange::from_event(event),
                    })],
                    attachments: pending_attachments.remove(&request_id).unwrap_or_default(),
                };
                let index = projection.messages.len();
                projection.messages.push(message);
                request_locations
                    .entry(request_id)
                    .or_default()
                    .user_message_index = Some(index);
            }
            EventV1::PromptAttachmentsSubmitted(payload) => {
                let request_id = payload.request_id.to_string();
                if let Some(message_index) = request_locations
                    .get(&request_id)
                    .and_then(|locations| locations.user_message_index)
                {
                    projection.messages[message_index]
                        .attachments
                        .extend(payload.attachments.iter().cloned());
                    projection.messages[message_index].provenance.extend(event);
                } else {
                    pending_attachments
                        .entry(request_id)
                        .or_default()
                        .extend(payload.attachments.iter().cloned());
                }
            }
            EventV1::ProviderRequestStarted(payload) => {
                let request_id = provider_turn_request_id(event, payload.request_id.as_str());
                let locations = request_locations.entry(request_id).or_default();
                let starts_new_provider_call = locations
                    .pending_provider
                    .as_ref()
                    .and_then(|provider| provider.provider_request_id.as_deref())
                    .is_some_and(|request_id| request_id != payload.request_id.as_str());
                if starts_new_provider_call {
                    locations.assistant_message_index = None;
                    locations.assistant_text_part_index = None;
                    locations.assistant_reasoning_part_index = None;
                    locations.pending_provider = None;
                    locations.pending_provenance = None;
                    locations.semantic_parts_authoritative = false;
                    locations.semantic_tool_requests_seen = 0;
                }
                locations.assistant_agent_id = event.actor.agent_id.clone();
                locations.pending_state = Some(ProjectedMessageState::Streaming);
                let provider = locations
                    .pending_provider
                    .get_or_insert_with(Default::default);
                provider.provider_request_id = Some(payload.request_id.to_string());
                provider.provider_id = Some(payload.provider_id.clone());
                provider.model_id = Some(payload.model_id.clone());
                provider.prompt_summary = Some(payload.prompt_summary.clone());
                provider.request_digest = Some(payload.request_digest.clone());
                if let Some(provenance) = locations.pending_provenance.as_mut() {
                    provenance.extend(event);
                } else {
                    locations.pending_provenance = Some(ProvenanceRange::from_event(event));
                }
            }
            EventV1::ProviderStreamDelta(payload) => {
                let request_id = provider_turn_request_id(event, payload.request_id.as_str());
                append_or_extend_assistant_text(
                    &mut projection,
                    &mut request_locations,
                    event,
                    &request_id,
                    &payload.delta,
                    AssistantTextKind::Text,
                );
            }
            EventV1::ProviderReasoningDelta(payload) => {
                let request_id = provider_turn_request_id(event, payload.request_id.as_str());
                append_or_extend_assistant_text(
                    &mut projection,
                    &mut request_locations,
                    event,
                    &request_id,
                    &payload.delta,
                    AssistantTextKind::Reasoning,
                );
            }
            EventV1::ProviderRequestFinished(payload) => {
                let request_id = provider_turn_request_id(event, payload.request_id.as_str());
                let state = if payload.finish_reason.eq_ignore_ascii_case("error") {
                    ProjectedMessageState::Failed
                } else {
                    ProjectedMessageState::Complete
                };
                let locations = request_locations.entry(request_id).or_default();
                locations.pending_state = Some(state);
                let provider = locations
                    .pending_provider
                    .get_or_insert_with(Default::default);
                provider.provider_request_id = Some(payload.request_id.to_string());
                provider.finish_reason = Some(payload.finish_reason.clone());
                provider.output_digest.clone_from(&payload.output_digest);
                if let Some(assistant_message) = payload
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.assistant_message.as_ref())
                {
                    apply_assistant_message_metadata(provider, assistant_message);
                }
                if let Some(provenance) = locations.pending_provenance.as_mut() {
                    provenance.extend(event);
                } else {
                    locations.pending_provenance = Some(ProvenanceRange::from_event(event));
                }
                if let Some(message_index) = locations.assistant_message_index {
                    let message = &mut projection.messages[message_index];
                    message.state = state;
                    message.provider.clone_from(&locations.pending_provider);
                    message.provenance.extend(event);
                }
            }
            EventV1::AssistantMessageFinished(payload) => {
                let request_id = provider_turn_request_id(event, payload.request_id.as_str());
                let message_index = ensure_assistant_message(
                    &mut projection,
                    &mut request_locations,
                    event,
                    &request_id,
                );
                let message = &mut projection.messages[message_index];
                message.state = ProjectedMessageState::Complete;
                if let Some(provenance) = payload.provenance.as_ref() {
                    let provider = message.provider.get_or_insert_with(Default::default);
                    provider.provider_request_id = Some(provenance.request_id.to_string());
                    provider.provider_id = Some(provenance.provider_id.clone());
                    provider.model_id = Some(provenance.model_id.clone());
                    provider.finish_reason.clone_from(&provenance.stop_reason);
                }
                if let Some(assistant_message) = payload.assistant_message.as_ref() {
                    let provider = message.provider.get_or_insert_with(Default::default);
                    apply_assistant_message_metadata(provider, assistant_message);
                }
                if !payload.parts.is_empty() {
                    tool_locations.retain(|_, location| location.message_index != message_index);
                    message.parts.clear();
                    let locations = request_locations.entry(request_id.clone()).or_default();
                    locations.assistant_text_part_index = None;
                    locations.assistant_reasoning_part_index = None;
                    locations.semantic_parts_authoritative = true;
                    locations.semantic_tool_requests_seen = 0;
                    for part in &payload.parts {
                        let part_index = message.parts.len();
                        match part {
                            AssistantPart::Text { text } => {
                                message.parts.push(ProjectedPart::Text(ProjectedTextPart {
                                    text: text.clone(),
                                    provenance: ProvenanceRange::from_event(event),
                                }));
                                locations.assistant_text_part_index = Some(part_index);
                            }
                            AssistantPart::Reasoning { text } => {
                                message
                                    .parts
                                    .push(ProjectedPart::Reasoning(ProjectedTextPart {
                                        text: text.clone(),
                                        provenance: ProvenanceRange::from_event(event),
                                    }));
                                locations.assistant_reasoning_part_index = Some(part_index);
                            }
                            AssistantPart::ToolCall(tool_call) => {
                                message.parts.push(ProjectedPart::ToolCall(Box::new(
                                    ProjectedToolCallPart {
                                        tool_call_id: tool_call.tool_call_id.clone(),
                                        tool_id: tool_call.tool_id.clone(),
                                        args_summary: tool_call.args_summary.clone(),
                                        args_digest: tool_call.args_digest.clone(),
                                        state: ProjectedToolCallState::Pending,
                                        status: None,
                                        output_summary: None,
                                        output_digest: None,
                                        output_json: None,
                                        requested_seq: None,
                                        started_seq: None,
                                        finished_seq: None,
                                        metadata: None,
                                        permissions: Vec::new(),
                                        artifacts: Vec::new(),
                                        lineage: None,
                                        provenance: ProvenanceRange::from_event(event),
                                    },
                                )));
                                tool_locations.insert(
                                    tool_call.tool_call_id.to_string(),
                                    PartLocation {
                                        message_index,
                                        part_index,
                                    },
                                );
                            }
                        }
                    }
                }
                message.provenance.extend(event);
            }
            EventV1::SessionCompaction(payload) => {
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::Compaction(ProjectedCompactionPart {
                        checkpoint_id: None,
                        agent_id: payload.agent_id.clone(),
                        status: CompactionCheckpointStatus::SessionCompacted,
                        trigger_reason: Some(payload.trigger_reason.clone()),
                        reason: None,
                        through_seq: Some(payload.first_kept_event_seq),
                        through_request_id: payload.first_kept_request_id.clone(),
                        artifact: None,
                        summary: Some(payload.summary.clone()),
                        tokens_before: Some(payload.tokens_before),
                        read_files: payload.read_files.clone(),
                        modified_files: payload.modified_files.clone(),
                        from_hook: Some(payload.from_hook),
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::BranchSummary(payload) => {
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::Compaction(ProjectedCompactionPart {
                        checkpoint_id: None,
                        agent_id: payload.agent_id.clone(),
                        status: CompactionCheckpointStatus::BranchSummary,
                        trigger_reason: None,
                        reason: None,
                        through_seq: Some(payload.from_event_seq),
                        through_request_id: None,
                        artifact: None,
                        summary: Some(payload.summary.clone()),
                        tokens_before: None,
                        read_files: payload.read_files.clone(),
                        modified_files: payload.modified_files.clone(),
                        from_hook: Some(payload.from_hook),
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::ToolCallRequested(payload) => {
                let lineage = payload
                    .metadata
                    .as_ref()
                    .and_then(|metadata| lineage_projection(metadata.lineage.as_ref(), event));
                if let Some(lineage) = lineage.as_ref() {
                    push_unique_lineage(&mut projection.session_lineage, lineage.clone());
                }
                let metadata_artifacts = artifacts_from_tool_metadata(
                    payload.tool_call_id.as_str(),
                    payload.metadata.as_ref(),
                    event,
                );
                for artifact in metadata_artifacts.iter().cloned() {
                    push_unique_artifact(&mut projection.artifacts, artifact);
                }
                let semantic_location = event
                    .correlation_id
                    .as_deref()
                    .and_then(|_| tool_locations.get(payload.tool_call_id.as_str()).copied());
                if let Some(location) = semantic_location {
                    let mut previous_tool_call_id = None;
                    if let Some(tool_call) = tool_call_part_mut(&mut projection, location) {
                        previous_tool_call_id = Some(tool_call.tool_call_id.to_string());
                        tool_call.tool_call_id = payload.tool_call_id.clone();
                        tool_call.requested_seq = Some(event.seq);
                        tool_call.metadata.clone_from(&payload.metadata);
                        tool_call.lineage = lineage.clone();
                        for artifact in metadata_artifacts {
                            push_unique_artifact(&mut tool_call.artifacts, artifact);
                        }
                        tool_call.provenance.extend(event);
                    }
                    if let Some(previous_tool_call_id) = previous_tool_call_id {
                        tool_locations.remove(&previous_tool_call_id);
                    }
                    tool_locations.insert(payload.tool_call_id.to_string(), location);
                    if let Some(request_id) = event.correlation_id.as_deref() {
                        let locations =
                            request_locations.entry(request_id.to_string()).or_default();
                        locations.semantic_tool_requests_seen =
                            locations.semantic_tool_requests_seen.saturating_add(1);
                    }
                    projection.messages[location.message_index]
                        .provenance
                        .extend(event);
                    continue;
                }
                if let Some(location) = tool_locations.get(payload.tool_call_id.as_str()).copied() {
                    if let Some(tool_call) = tool_call_part_mut(&mut projection, location) {
                        tool_call.requested_seq = Some(event.seq);
                        tool_call.metadata.clone_from(&payload.metadata);
                        tool_call.lineage = lineage.clone();
                        for artifact in metadata_artifacts {
                            push_unique_artifact(&mut tool_call.artifacts, artifact);
                        }
                        tool_call.provenance.extend(event);
                    }
                    projection.messages[location.message_index]
                        .provenance
                        .extend(event);
                    continue;
                }

                let tool_part = ProjectedToolCallPart {
                    tool_call_id: payload.tool_call_id.clone(),
                    tool_id: payload.tool_id.clone(),
                    args_summary: payload.args_summary.clone(),
                    args_digest: payload.args_digest.clone(),
                    state: ProjectedToolCallState::Pending,
                    status: None,
                    output_summary: None,
                    output_digest: None,
                    output_json: None,
                    requested_seq: Some(event.seq),
                    started_seq: None,
                    finished_seq: None,
                    metadata: payload.metadata.clone(),
                    permissions: Vec::new(),
                    artifacts: metadata_artifacts,
                    lineage,
                    provenance: ProvenanceRange::from_event(event),
                };

                if let Some(request_id) = event.correlation_id.as_deref() {
                    let message_index = ensure_assistant_message(
                        &mut projection,
                        &mut request_locations,
                        event,
                        request_id,
                    );
                    let part_index = append_part_to_message(
                        &mut projection,
                        message_index,
                        ProjectedPart::ToolCall(Box::new(tool_part)),
                        event,
                    );
                    tool_locations.insert(
                        payload.tool_call_id.to_string(),
                        PartLocation {
                            message_index,
                            part_index,
                        },
                    );
                } else {
                    let message_index = append_system_part(
                        &mut projection,
                        event,
                        ProjectedPart::ToolCall(Box::new(tool_part)),
                    );
                    tool_locations.insert(
                        payload.tool_call_id.to_string(),
                        PartLocation {
                            message_index,
                            part_index: 0,
                        },
                    );
                }
            }
            EventV1::ToolCallStarted(payload) => {
                if let Some(location) = tool_locations.get(payload.tool_call_id.as_str()).copied() {
                    if let Some(tool_call) = tool_call_part_mut(&mut projection, location) {
                        tool_call.state = ProjectedToolCallState::Running;
                        tool_call.started_seq = Some(event.seq);
                        tool_call.provenance.extend(event);
                    }
                    projection.messages[location.message_index]
                        .provenance
                        .extend(event);
                } else {
                    let message_index = append_system_part(
                        &mut projection,
                        event,
                        ProjectedPart::ToolCall(Box::new(placeholder_tool_call_part(
                            payload.tool_call_id.as_str(),
                            ProjectedToolCallState::Running,
                            event,
                        ))),
                    );
                    tool_locations.insert(
                        payload.tool_call_id.to_string(),
                        PartLocation {
                            message_index,
                            part_index: 0,
                        },
                    );
                }
            }
            EventV1::ToolCallFinished(payload) => {
                let metadata_artifacts = artifacts_from_tool_metadata(
                    payload.tool_call_id.as_str(),
                    payload.metadata.as_ref(),
                    event,
                );
                for artifact in metadata_artifacts.iter().cloned() {
                    push_unique_artifact(&mut projection.artifacts, artifact);
                }
                if let Some(lineage) = payload
                    .metadata
                    .as_ref()
                    .and_then(|metadata| lineage_projection(metadata.lineage.as_ref(), event))
                {
                    push_unique_lineage(&mut projection.session_lineage, lineage.clone());
                }

                if let Some(location) = tool_locations.get(payload.tool_call_id.as_str()).copied() {
                    if let Some(tool_call) = tool_call_part_mut(&mut projection, location) {
                        tool_call.state = match payload.status {
                            ToolCallStatus::Succeeded => ProjectedToolCallState::Succeeded,
                            ToolCallStatus::Failed => ProjectedToolCallState::Failed,
                        };
                        tool_call.status = Some(payload.status);
                        tool_call.output_summary = payload.output_summary.clone();
                        tool_call.output_digest = payload.output_digest.clone();
                        tool_call.output_json = payload.output_json.clone();
                        tool_call.finished_seq = Some(event.seq);
                        if tool_call.metadata.is_none() {
                            tool_call.metadata = payload.metadata.clone();
                        }
                        if tool_call.lineage.is_none() {
                            tool_call.lineage = payload.metadata.as_ref().and_then(|metadata| {
                                lineage_projection(metadata.lineage.as_ref(), event)
                            });
                        }
                        for artifact in metadata_artifacts {
                            push_unique_artifact(&mut tool_call.artifacts, artifact);
                        }
                        tool_call.provenance.extend(event);
                    }
                    projection.messages[location.message_index]
                        .provenance
                        .extend(event);
                } else {
                    let mut tool_part = placeholder_tool_call_part(
                        payload.tool_call_id.as_str(),
                        match payload.status {
                            ToolCallStatus::Succeeded => ProjectedToolCallState::Succeeded,
                            ToolCallStatus::Failed => ProjectedToolCallState::Failed,
                        },
                        event,
                    );
                    tool_part.status = Some(payload.status);
                    tool_part.output_summary = payload.output_summary.clone();
                    tool_part.output_digest = payload.output_digest.clone();
                    tool_part.output_json = payload.output_json.clone();
                    tool_part.finished_seq = Some(event.seq);
                    tool_part.metadata = payload.metadata.clone();
                    tool_part.lineage = payload
                        .metadata
                        .as_ref()
                        .and_then(|metadata| lineage_projection(metadata.lineage.as_ref(), event));
                    tool_part.artifacts = metadata_artifacts;
                    let message_index = append_system_part(
                        &mut projection,
                        event,
                        ProjectedPart::ToolCall(Box::new(tool_part)),
                    );
                    tool_locations.insert(
                        payload.tool_call_id.to_string(),
                        PartLocation {
                            message_index,
                            part_index: 0,
                        },
                    );
                }
            }
            EventV1::PermissionRequested(payload) => {
                let part = ProjectedPermissionPart {
                    permission_id: payload.permission_id.clone(),
                    kind: payload.kind.clone(),
                    tool_call_id: payload.tool_call_id.clone(),
                    summary: payload.summary.clone(),
                    request_digest: payload.request_digest.clone(),
                    timeout_ms: payload.timeout_ms,
                    default_decision: payload.default_decision,
                    state: ProjectedPermissionState::Pending,
                    decision: None,
                    reason: None,
                    provenance: ProvenanceRange::from_event(event),
                };
                let message_index = append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::Permission(part.clone()),
                );
                permission_locations.insert(
                    payload.permission_id.clone(),
                    PartLocation {
                        message_index,
                        part_index: 0,
                    },
                );
                if let Some(tool_call_id) = payload.tool_call_id.as_ref() {
                    if let Some(tool_location) = tool_locations.get(tool_call_id.as_str()).copied()
                    {
                        if let Some(tool_call) = tool_call_part_mut(&mut projection, tool_location)
                        {
                            tool_call.permissions.push(part);
                            tool_call.provenance.extend(event);
                        }
                    }
                }
            }
            EventV1::PermissionResolved(payload) => {
                if let Some(location) = permission_locations.get(&payload.permission_id).copied() {
                    if let Some(permission) = permission_part_mut(&mut projection, location) {
                        permission.state = ProjectedPermissionState::Resolved;
                        permission.decision = Some(payload.decision);
                        permission.reason = payload.reason.clone();
                        permission.provenance.extend(event);
                    }
                    projection.messages[location.message_index]
                        .provenance
                        .extend(event);
                } else {
                    append_system_part(
                        &mut projection,
                        event,
                        ProjectedPart::Permission(ProjectedPermissionPart {
                            permission_id: payload.permission_id.clone(),
                            kind: String::new(),
                            tool_call_id: None,
                            summary: String::new(),
                            request_digest: String::new(),
                            timeout_ms: 0,
                            default_decision: payload.decision,
                            state: ProjectedPermissionState::Resolved,
                            decision: Some(payload.decision),
                            reason: payload.reason.clone(),
                            provenance: ProvenanceRange::from_event(event),
                        }),
                    );
                }
                update_tool_permission_resolution(
                    &mut projection,
                    &payload.permission_id,
                    payload.decision,
                    payload.reason.clone(),
                    event,
                );
            }
            EventV1::ArtifactWritten(payload) => {
                let artifact = artifact_from_written(payload, event);
                push_unique_artifact(&mut projection.artifacts, artifact.clone());
                if let Some(tool_call_id) = payload.tool_call_id.as_ref() {
                    if let Some(location) = tool_locations.get(tool_call_id.as_str()).copied() {
                        if let Some(tool_call) = tool_call_part_mut(&mut projection, location) {
                            push_unique_artifact(&mut tool_call.artifacts, artifact.clone());
                            tool_call.provenance.extend(event);
                        }
                    }
                }
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::Artifact(ProjectedArtifactPart { artifact }),
                );
            }
            EventV1::PolicyViolationDetected(payload) => {
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::PolicyViolation(ProjectedPolicyViolationPart {
                        policy: payload.policy.clone(),
                        detail: payload.detail.clone(),
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::UiIntentReceived(payload) => {
                append_system_part(
                    &mut projection,
                    event,
                    ProjectedPart::UiIntent(ProjectedUiIntentPart {
                        intent: payload.intent.clone(),
                        params: payload.params.clone(),
                        provenance: ProvenanceRange::from_event(event),
                    }),
                );
            }
            EventV1::StaleDetected(_)
            | EventV1::BackgroundTaskNotification(_)
            | EventV1::PermissionGrantRecorded(_)
            | EventV1::EditProposed(_)
            | EventV1::EditApplied(_)
            | EventV1::EditRejected(_)
            | EventV1::WorkspaceSnapshot(_)
            | EventV1::WorkspaceReverted(_) => {}
            _ if classify_compatibility_event(&event.payload).is_some() => {}
            _ => {}
        }
    }

    Ok(projection)
}
