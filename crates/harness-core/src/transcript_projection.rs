use std::collections::BTreeMap;

use crate::event::{EventEnvelopeV1, EventV1, TaskScheduleState, ToolCallStatus};

mod helpers;
mod model;

pub use model::*;

use helpers::{
    append_or_extend_assistant_text, append_part_to_message, append_system_part, append_task_part,
    apply_assistant_message_metadata, artifact_from_written, artifacts_from_tool_metadata,
    ensure_assistant_message, ensure_strict_seq_order, lineage_projection, permission_part_mut,
    placeholder_tool_call_part, provider_turn_request_id, push_unique_artifact,
    push_unique_lineage, tool_call_part_mut, update_tool_permission_resolution,
    upsert_compaction_checkpoint, AssistantTextKind, PartLocation, RequestLocations,
};

pub fn project_transcript(
    events: &[EventEnvelopeV1],
) -> Result<TranscriptProjection, TranscriptProjectionError> {
    ensure_strict_seq_order(events)?;

    let mut projection = TranscriptProjection::default();
    let mut request_locations = BTreeMap::<String, RequestLocations>::new();
    let mut tool_locations = BTreeMap::<String, PartLocation>::new();
    let mut permission_locations = BTreeMap::<String, PartLocation>::new();
    let mut compaction_locations = BTreeMap::<String, usize>::new();

    for event in events {
        projection
            .session
            .run_id
            .get_or_insert(event.run_id.clone());
        projection.session.max_seq = Some(event.seq);

        match &event.payload {
            EventV1::RunStarted(payload) => {
                projection.session.run_id = Some(event.run_id.clone());
                projection.session.run_name = Some(payload.run_name.clone());
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
                        summary: Some(payload.run_name.clone()),
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
                let message = ProjectedMessage {
                    message_id: format!("user:{}:{}", payload.request_id, event.seq),
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
                };
                let index = projection.messages.len();
                projection.messages.push(message);
                request_locations
                    .entry(payload.request_id.clone())
                    .or_default()
                    .user_message_index = Some(index);
            }
            EventV1::ProviderRequestStarted(payload) => {
                let request_id = provider_turn_request_id(event, &payload.request_id);
                let message_index = ensure_assistant_message(
                    &mut projection,
                    &mut request_locations,
                    event,
                    &request_id,
                );
                let message = &mut projection.messages[message_index];
                message.state = ProjectedMessageState::Streaming;
                let provider = message.provider.get_or_insert_with(Default::default);
                provider.provider_request_id = Some(payload.request_id.clone());
                provider.provider_id = Some(payload.provider_id.clone());
                provider.model_id = Some(payload.model_id.clone());
                provider.prompt_summary = Some(payload.prompt_summary.clone());
                provider.request_digest = Some(payload.request_digest.clone());
                message.provenance.extend(event);
            }
            EventV1::ProviderStreamDelta(payload) => {
                let request_id = provider_turn_request_id(event, &payload.request_id);
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
                let request_id = provider_turn_request_id(event, &payload.request_id);
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
                let request_id = provider_turn_request_id(event, &payload.request_id);
                let message_index = ensure_assistant_message(
                    &mut projection,
                    &mut request_locations,
                    event,
                    &request_id,
                );
                let message = &mut projection.messages[message_index];
                message.state = if payload.finish_reason.eq_ignore_ascii_case("error") {
                    ProjectedMessageState::Failed
                } else {
                    ProjectedMessageState::Complete
                };
                let provider = message.provider.get_or_insert_with(Default::default);
                provider.provider_request_id = Some(payload.request_id.clone());
                provider.finish_reason = Some(payload.finish_reason.clone());
                provider.output_digest = payload.output_digest.clone();
                if let Some(metadata) = payload.metadata.as_ref() {
                    if let Some(assistant_message) = metadata.assistant_message.as_ref() {
                        apply_assistant_message_metadata(provider, assistant_message);
                    }
                }
                message.provenance.extend(event);
            }
            EventV1::AssistantMessageFinished(payload) => {
                let request_id = provider_turn_request_id(event, &payload.request_id);
                let message_index = ensure_assistant_message(
                    &mut projection,
                    &mut request_locations,
                    event,
                    &request_id,
                );
                let message = &mut projection.messages[message_index];
                message.state = ProjectedMessageState::Complete;
                if let Some(assistant_message) = payload.assistant_message.as_ref() {
                    let provider = message.provider.get_or_insert_with(Default::default);
                    apply_assistant_message_metadata(provider, assistant_message);
                }
                message.provenance.extend(event);
            }
            EventV1::CompactionRequested(payload) => {
                let checkpoint = CompactionCheckpointProjection {
                    checkpoint_id: Some(payload.checkpoint_id.clone()),
                    agent_id: payload.agent_id.clone(),
                    status: CompactionCheckpointStatus::Requested,
                    trigger_reason: Some(payload.trigger_reason.clone()),
                    reason: None,
                    through_seq: Some(payload.through_seq),
                    through_request_id: payload.through_request_id.clone(),
                    provider_id: payload.provider_id.clone(),
                    model_id: payload.model_id.clone(),
                    tokens_before: payload.tokens_before,
                    tokens_before_estimate: payload.tokens_before_estimate,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    preserved_turns: None,
                    artifact: None,
                    provenance: ProvenanceRange::from_event(event),
                };
                upsert_compaction_checkpoint(
                    &mut projection,
                    &mut compaction_locations,
                    event,
                    checkpoint,
                );
            }
            EventV1::CompactionWritten(payload) => {
                let artifact = TranscriptArtifactRef {
                    path: payload.artifact_path.clone(),
                    digest: payload.artifact_digest.clone(),
                    bytes: Some(payload.artifact_bytes),
                    tool_call_id: None,
                    source: ArtifactProjectionSource::CompactionWritten,
                    metadata: BTreeMap::from([
                        ("checkpoint_id".to_string(), payload.checkpoint_id.clone()),
                        ("agent_id".to_string(), payload.agent_id.clone()),
                    ]),
                    provenance: ProvenanceRange::from_event(event),
                };
                push_unique_artifact(&mut projection.artifacts, artifact.clone());
                let checkpoint = CompactionCheckpointProjection {
                    checkpoint_id: Some(payload.checkpoint_id.clone()),
                    agent_id: payload.agent_id.clone(),
                    status: CompactionCheckpointStatus::Written,
                    trigger_reason: Some(payload.trigger_reason.clone()),
                    reason: None,
                    through_seq: Some(payload.through_seq),
                    through_request_id: payload.through_request_id.clone(),
                    provider_id: payload.provider_id.clone(),
                    model_id: payload.model_id.clone(),
                    tokens_before: payload.tokens_before,
                    tokens_before_estimate: payload.tokens_before_estimate,
                    tokens_after_estimate: payload.tokens_after_estimate,
                    summary_tokens_estimate: payload.summary_tokens_estimate,
                    compacted_turns: payload.compacted_turns,
                    reduction_tokens_estimate: payload.reduction_tokens_estimate,
                    reduction_percent_estimate: payload.reduction_percent_estimate,
                    preserved_turns: Some(payload.preserved_turns),
                    artifact: Some(artifact),
                    provenance: ProvenanceRange::from_event(event),
                };
                upsert_compaction_checkpoint(
                    &mut projection,
                    &mut compaction_locations,
                    event,
                    checkpoint,
                );
            }
            EventV1::CompactionApplied(payload) => {
                let checkpoint = CompactionCheckpointProjection {
                    checkpoint_id: Some(payload.checkpoint_id.clone()),
                    agent_id: payload.agent_id.clone(),
                    status: CompactionCheckpointStatus::Applied,
                    trigger_reason: None,
                    reason: None,
                    through_seq: Some(payload.through_seq),
                    through_request_id: payload.through_request_id.clone(),
                    provider_id: None,
                    model_id: None,
                    tokens_before: None,
                    tokens_before_estimate: payload.tokens_before_estimate,
                    tokens_after_estimate: payload.tokens_after_estimate,
                    summary_tokens_estimate: payload.summary_tokens_estimate,
                    compacted_turns: payload.compacted_turns,
                    reduction_tokens_estimate: payload.reduction_tokens_estimate,
                    reduction_percent_estimate: payload.reduction_percent_estimate,
                    preserved_turns: payload.preserved_turns,
                    artifact: None,
                    provenance: ProvenanceRange::from_event(event),
                };
                upsert_compaction_checkpoint(
                    &mut projection,
                    &mut compaction_locations,
                    event,
                    checkpoint,
                );
            }
            EventV1::CompactionFailed(payload) => {
                let checkpoint = CompactionCheckpointProjection {
                    checkpoint_id: payload.checkpoint_id.clone(),
                    agent_id: payload.agent_id.clone(),
                    status: CompactionCheckpointStatus::Failed,
                    trigger_reason: Some(payload.trigger_reason.clone()),
                    reason: Some(payload.reason.clone()),
                    through_seq: payload.through_seq,
                    through_request_id: payload.through_request_id.clone(),
                    provider_id: None,
                    model_id: None,
                    tokens_before: None,
                    tokens_before_estimate: None,
                    tokens_after_estimate: None,
                    summary_tokens_estimate: None,
                    compacted_turns: None,
                    reduction_tokens_estimate: None,
                    reduction_percent_estimate: None,
                    preserved_turns: None,
                    artifact: None,
                    provenance: ProvenanceRange::from_event(event),
                };
                upsert_compaction_checkpoint(
                    &mut projection,
                    &mut compaction_locations,
                    event,
                    checkpoint,
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
                    &payload.tool_call_id,
                    payload.metadata.as_ref(),
                    event,
                );
                for artifact in metadata_artifacts.iter().cloned() {
                    push_unique_artifact(&mut projection.artifacts, artifact);
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
                        payload.tool_call_id.clone(),
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
                        payload.tool_call_id.clone(),
                        PartLocation {
                            message_index,
                            part_index: 0,
                        },
                    );
                }
            }
            EventV1::ToolCallStarted(payload) => {
                if let Some(location) = tool_locations.get(&payload.tool_call_id).copied() {
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
                            &payload.tool_call_id,
                            ProjectedToolCallState::Running,
                            event,
                        ))),
                    );
                    tool_locations.insert(
                        payload.tool_call_id.clone(),
                        PartLocation {
                            message_index,
                            part_index: 0,
                        },
                    );
                }
            }
            EventV1::ToolCallFinished(payload) => {
                let metadata_artifacts = artifacts_from_tool_metadata(
                    &payload.tool_call_id,
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

                if let Some(location) = tool_locations.get(&payload.tool_call_id).copied() {
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
                        &payload.tool_call_id,
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
                        payload.tool_call_id.clone(),
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
                    if let Some(tool_location) = tool_locations.get(tool_call_id).copied() {
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
                    if let Some(location) = tool_locations.get(tool_call_id).copied() {
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
            | EventV1::EditRejected(_) => {}
        }
    }

    Ok(projection)
}
