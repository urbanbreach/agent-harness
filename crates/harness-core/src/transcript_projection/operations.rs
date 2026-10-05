use super::*;

pub(super) fn apply(
    output: &mut TranscriptProjection,
    index: &mut TranscriptIndex,
    event: &EventEnvelopeV1,
) -> bool {
    match &event.payload {
        EventV1::RunStarted(data) => {
            output.session.run_name = Some(data.run_name.to_string());
            output.session.workspace_root = Some(data.workspace_root.clone());
            output.session.status = TranscriptRunStatus::Running;
            output.session.started_seq = Some(event.seq);
            lifecycle(
                output,
                index,
                event,
                LifecycleEventKind::RunStarted,
                None,
                None,
                None,
                None,
            );
        }
        EventV1::RunFinished(data) => {
            output.session.status = TranscriptRunStatus::Finished;
            output.session.terminal_seq = Some(event.seq);
            output.session.status_reason = Some(data.summary.clone());
            lifecycle(
                output,
                index,
                event,
                LifecycleEventKind::RunFinished,
                None,
                None,
                Some(&data.summary),
                None,
            );
        }
        EventV1::RunFailed(data) => {
            output.session.status = TranscriptRunStatus::Failed;
            output.session.terminal_seq = Some(event.seq);
            output.session.status_reason = Some(data.error.clone());
            lifecycle(
                output,
                index,
                event,
                LifecycleEventKind::RunFailed,
                None,
                None,
                None,
                Some(&data.error),
            );
        }
        EventV1::AgentSpawned(data) => {
            output
                .session
                .agent_profiles
                .insert(data.agent_id.clone(), data.profile.clone());
            lifecycle(
                output,
                index,
                event,
                LifecycleEventKind::AgentSpawned,
                Some(data),
                None,
                None,
                None,
            );
        }
        EventV1::AgentStopped(data) => lifecycle(
            output,
            index,
            event,
            LifecycleEventKind::AgentStopped,
            None,
            Some(data),
            None,
            None,
        ),
        EventV1::ToolCallRequested(data) => {
            let location = tool_location(output, index, event, data.tool_call_id.as_str());
            if let Some(tool) = tool_mut(output, location) {
                tool.tool_id.clone_from(&data.tool_id);
                tool.args_summary.clone_from(&data.args_summary);
                tool.args_digest.clone_from(&data.args_digest);
                tool.requested_seq = Some(event.seq);
                tool.provenance.extend(event);
            }
            tool_metadata(output, location, data.metadata.as_ref(), event);
        }
        EventV1::ToolCallStarted(data) => {
            let location = tool_location(output, index, event, data.tool_call_id.as_str());
            if let Some(tool) = tool_mut(output, location) {
                tool.state = ProjectedToolCallState::Running;
                tool.started_seq = Some(event.seq);
                tool.provenance.extend(event);
            }
        }
        EventV1::ToolCallFinished(data) | EventV1::EvalCellFinished(data) => {
            let location = tool_location(output, index, event, data.tool_call_id.as_str());
            if let Some(tool) = tool_mut(output, location) {
                tool.state = if data
                    .output_json
                    .as_ref()
                    .is_some_and(|v| v["detached"] == true)
                {
                    ProjectedToolCallState::Running
                } else {
                    match data.status {
                        ToolCallStatus::Succeeded => ProjectedToolCallState::Succeeded,
                        ToolCallStatus::Failed => ProjectedToolCallState::Failed,
                    }
                };
                tool.status = Some(data.status);
                tool.output_summary.clone_from(&data.output_summary);
                tool.output_digest.clone_from(&data.output_digest);
                tool.output_json.clone_from(&data.output_json);
                tool.finished_seq = Some(event.seq);
                tool.provenance.extend(event);
            }
            tool_metadata(output, location, data.metadata.as_ref(), event);
        }
        EventV1::PermissionRequested(data) => {
            let permission = ProjectedPermissionPart {
                permission_id: data.permission_id.clone(),
                kind: data.kind.clone(),
                tool_call_id: data.tool_call_id.clone(),
                summary: data.summary.clone(),
                request_digest: data.request_digest.clone(),
                timeout_ms: data.timeout_ms,
                default_decision: data.default_decision,
                state: ProjectedPermissionState::Pending,
                decision: None,
                reason: None,
                provenance: ProvenanceRange::at(event),
            };
            if let Some(location) = data
                .tool_call_id
                .as_ref()
                .and_then(|id| index.tools.get(id.as_str()))
                .copied()
            {
                if let Some(tool) = tool_mut(output, location) {
                    tool.permissions.push(permission.clone());
                }
            }
            let (message, part) =
                push_part(output, index, event, ProjectedPart::Permission(permission));
            index
                .permissions
                .insert(data.permission_id.clone(), (message, part, None));
        }
        EventV1::PermissionResolved(data) => {
            if let Some(&(message, part, nested)) = index.permissions.get(&data.permission_id) {
                let permission = match &mut output.messages[message].parts[part] {
                    ProjectedPart::Permission(permission) => Some(permission),
                    ProjectedPart::ToolCall(tool) => {
                        nested.and_then(|at| tool.permissions.get_mut(at))
                    }
                    _ => None,
                };
                if let Some(permission) = permission {
                    permission.state = ProjectedPermissionState::Resolved;
                    permission.decision = Some(data.decision);
                    permission.reason.clone_from(&data.reason);
                    permission.provenance.extend(event);
                    let resolved = permission.clone();
                    let tool = resolved
                        .tool_call_id
                        .as_ref()
                        .and_then(|id| index.tools.get(id.as_str()))
                        .copied();
                    if let Some(permission) =
                        tool.and_then(|at| tool_mut(output, at)).and_then(|tool| {
                            tool.permissions
                                .iter_mut()
                                .find(|p| p.permission_id == resolved.permission_id)
                        })
                    {
                        *permission = resolved;
                    }
                }
            }
        }
        EventV1::TaskScheduled(_)
        | EventV1::TaskCompleted(_)
        | EventV1::TaskCancelled(_)
        | EventV1::TaskResultLate(_) => task(output, index, event),
        EventV1::ArtifactWritten(data) => {
            let artifact = TranscriptArtifactRef {
                path: data.path.clone(),
                digest: Some(data.digest.clone()),
                bytes: Some(data.bytes),
                tool_call_id: data.tool_call_id.clone(),
                source: ArtifactProjectionSource::ArtifactWritten,
                metadata: data.metadata.clone(),
                provenance: ProvenanceRange::at(event),
            };
            if let Some(location) = data
                .tool_call_id
                .as_ref()
                .and_then(|id| index.tools.get(id.as_str()))
                .copied()
            {
                if let Some(tool) = tool_mut(output, location) {
                    tool.artifacts.push(artifact.clone());
                }
            }
            output.artifacts.push(artifact.clone());
            push_part(
                output,
                index,
                event,
                ProjectedPart::Artifact(ProjectedArtifactPart { artifact }),
            );
        }
        EventV1::PolicyViolationDetected(data) => {
            push_part(
                output,
                index,
                event,
                ProjectedPart::PolicyViolation(ProjectedPolicyViolationPart {
                    policy: data.policy.clone(),
                    detail: data.detail.clone(),
                    provenance: ProvenanceRange::at(event),
                }),
            );
        }
        EventV1::UiIntentReceived(data) => {
            push_part(
                output,
                index,
                event,
                ProjectedPart::UiIntent(ProjectedUiIntentPart {
                    intent: data.intent.clone(),
                    params: data.params.clone(),
                    provenance: ProvenanceRange::at(event),
                }),
            );
        }
        EventV1::SessionTitleUpdated(data) => output.session.run_name = Some(data.title.clone()),
        _ => return false,
    }
    true
}

fn tool_location(
    output: &mut TranscriptProjection,
    index: &mut TranscriptIndex,
    event: &EventEnvelopeV1,
    id: &str,
) -> (usize, usize) {
    if let Some(&at) = index.tools.get(id) {
        return at;
    }
    let tool = ProjectedToolCallPart {
        tool_call_id: id.into(),
        permissions: output
            .messages
            .iter()
            .flat_map(|message| &message.parts)
            .filter_map(|part| match part {
                ProjectedPart::Permission(permission)
                    if permission
                        .tool_call_id
                        .as_ref()
                        .is_some_and(|tool| tool.as_str() == id) =>
                {
                    Some(permission.clone())
                }
                _ => None,
            })
            .collect(),
        provenance: ProvenanceRange::at(event),
        ..Default::default()
    };
    let location = push_part(
        output,
        index,
        event,
        ProjectedPart::ToolCall(Box::new(tool)),
    );
    index.tools.insert(id.into(), location);
    location
}

fn tool_mut(
    output: &mut TranscriptProjection,
    (message, part): (usize, usize),
) -> Option<&mut ProjectedToolCallPart> {
    if let ProjectedPart::ToolCall(tool) = &mut output.messages[message].parts[part] {
        Some(tool)
    } else {
        None
    }
}

pub(super) fn lineage(
    data: &TaskLineageMetadata,
    event: &EventEnvelopeV1,
) -> SessionLineageProjection {
    SessionLineageProjection {
        parent_tool_call_id: data.parent_tool_call_id.clone(),
        parent_task_id: data.parent_task_id.clone(),
        parent_request_id: data.parent_request_id.clone(),
        parent_session_id: data.parent_session_id.clone(),
        child_session_id: data.child_session_id.clone(),
        child_request_id: data.child_request_id.clone(),
        child_provider_id: data.child_provider_id.clone(),
        child_model_id: data.child_model_id.clone(),
        provenance: ProvenanceRange::at(event),
    }
}

fn tool_metadata(
    output: &mut TranscriptProjection,
    location: (usize, usize),
    metadata: Option<&ToolCallMetadata>,
    event: &EventEnvelopeV1,
) {
    let Some(metadata) = metadata else { return };
    let projected_lineage = metadata.lineage.as_ref().map(|data| lineage(data, event));
    if let Some(lineage) = &projected_lineage {
        output.session_lineage.push(lineage.clone());
    }
    let Some(tool) = tool_mut(output, location) else {
        return;
    };
    if projected_lineage.is_some() {
        tool.lineage = projected_lineage;
    }
    let merged = tool.metadata.get_or_insert_with(Default::default);
    if metadata.canonical_tool_id.is_some() {
        merged
            .canonical_tool_id
            .clone_from(&metadata.canonical_tool_id);
    }
    if metadata.alias_source_tool_id.is_some() {
        merged
            .alias_source_tool_id
            .clone_from(&metadata.alias_source_tool_id);
    }
    if metadata.lineage.is_some() {
        merged.lineage.clone_from(&metadata.lineage);
    }
    if metadata.timing.is_some() {
        merged.timing.clone_from(&metadata.timing);
    }
    for hook in &metadata.hook_executions {
        if !merged.hook_executions.contains(hook) {
            merged.hook_executions.push(hook.clone());
        }
    }
    for artifact in &metadata.artifact_refs {
        if merged.artifact_refs.contains(artifact) {
            continue;
        }
        merged.artifact_refs.push(artifact.clone());
        tool.artifacts.push(TranscriptArtifactRef {
            path: artifact.path.clone(),
            digest: artifact.digest.clone(),
            bytes: None,
            tool_call_id: Some(tool.tool_call_id.clone()),
            source: ArtifactProjectionSource::ToolCallMetadata,
            metadata: Default::default(),
            provenance: ProvenanceRange::at(event),
        });
    }
}

fn lifecycle(
    output: &mut TranscriptProjection,
    index: &TranscriptIndex,
    event: &EventEnvelopeV1,
    kind: LifecycleEventKind,
    start: Option<&AgentSpawnedEvent>,
    stop: Option<&AgentStoppedEvent>,
    summary: Option<&str>,
    error: Option<&str>,
) {
    push_part(
        output,
        index,
        event,
        ProjectedPart::Lifecycle(ProjectedLifecyclePart {
            event: kind,
            agent_id: start
                .map(|s| s.agent_id.clone())
                .or_else(|| stop.map(|s| s.agent_id.clone()))
                .or_else(|| event.actor.agent_id.clone()),
            profile: start.map(|s| s.profile.clone()),
            parent_agent_id: start.and_then(|s| s.parent_agent_id.clone()),
            summary: summary.map(str::to_owned),
            error: error.map(str::to_owned),
            reason: stop.map(|s| s.reason.clone()),
            provenance: ProvenanceRange::at(event),
        }),
    );
}

fn task(output: &mut TranscriptProjection, index: &TranscriptIndex, event: &EventEnvelopeV1) {
    let mut task = ProjectedTaskPart {
        task_id: Default::default(),
        state: ProjectedTaskState::Queued,
        queue_key: None,
        reason: None,
        result_summary: None,
        result_digest: None,
        lineage: None,
        terminal_scope: None,
        timing_elapsed_ms: None,
        terminal_mono_ms: None,
        provenance: ProvenanceRange::at(event),
    };
    match &event.payload {
        EventV1::TaskScheduled(data) => {
            task.task_id = data.task_id.clone();
            task.queue_key.clone_from(&data.queue_key);
            task.state = match data.state {
                TaskScheduleState::Queued => ProjectedTaskState::Queued,
                TaskScheduleState::Started => ProjectedTaskState::Started,
            };
            task.lineage = data
                .metadata
                .as_ref()
                .and_then(|m| m.lineage.as_ref())
                .map(|l| lineage(l, event));
        }
        EventV1::TaskCompleted(data) => {
            task.task_id = data.task_id.clone();
            task.state = ProjectedTaskState::Completed;
            task.result_summary = Some(data.result_summary.clone());
            task.result_digest = Some(data.result_digest.clone());
            task.terminal_mono_ms = Some(event.mono_ms);
            if let Some(metadata) = &data.metadata {
                task.lineage = metadata.lineage.as_ref().map(|l| lineage(l, event));
                task.terminal_scope = metadata.task_scope;
                task.timing_elapsed_ms = metadata.timing.as_ref().and_then(|t| t.elapsed_ms);
            }
        }
        EventV1::TaskCancelled(data) => {
            task.task_id = data.task_id.clone();
            task.state = if data.failure {
                ProjectedTaskState::Failed
            } else {
                ProjectedTaskState::Cancelled
            };
            task.reason = Some(data.reason.clone());
            task.terminal_scope = data.task_scope;
            task.terminal_mono_ms = Some(event.mono_ms);
            if let Some(&at) = event
                .correlation_id
                .as_ref()
                .and_then(|id| index.turns.get(id))
            {
                if output.messages[at].state != ProjectedMessageState::Failed {
                    output.messages[at].state = if data.failure {
                        ProjectedMessageState::Failed
                    } else {
                        ProjectedMessageState::Incomplete
                    };
                }
            }
        }
        EventV1::TaskResultLate(data) => {
            task.task_id = data.task_id.clone();
            task.state = ProjectedTaskState::LateResult;
            task.reason = Some("late result after stale cancellation".into());
            task.result_digest = Some(data.result_digest.clone());
        }
        _ => return,
    }
    if let Some(lineage) = &task.lineage {
        output.session_lineage.push(lineage.clone());
    }
    push_part(output, index, event, ProjectedPart::Task(task));
}
