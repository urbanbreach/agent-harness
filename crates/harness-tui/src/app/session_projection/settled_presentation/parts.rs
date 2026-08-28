use super::*;

pub(super) fn profile_label(
    transcript: &harness_core::transcript_projection::TranscriptProjection,
    agent_id: Option<&str>,
) -> String {
    agent_id
        .and_then(|id| transcript.session.agent_profiles.get(id))
        .cloned()
        .unwrap_or_else(|| "default".to_string())
}

pub(super) const fn activity_status(state: ProjectedMessageState) -> ActivityStatus {
    match state {
        ProjectedMessageState::Complete => ActivityStatus::Done,
        ProjectedMessageState::Streaming | ProjectedMessageState::Incomplete => {
            ActivityStatus::Streaming
        }
        ProjectedMessageState::Failed => ActivityStatus::Error,
    }
}

pub(super) fn apply_message_part(
    activity: &mut ActivityEntry,
    part: &ProjectedPart,
    pending_permissions: &mut BTreeMap<String, PendingPermission>,
    orchestration_tasks: &mut BTreeMap<String, OrchestrationTaskRow>,
    turn_terminals: &mut BTreeMap<String, SettledTurnTerminal>,
    agent_id: Option<&str>,
    request_id: &str,
) {
    match part {
        ProjectedPart::Text(text) => activity.transcript_text.push_str(&text.text),
        ProjectedPart::Reasoning(text) => activity.thinking_text.push_str(&text.text),
        ProjectedPart::ToolCall(tool) => activity.tool_calls.push(tool_entry(tool)),
        ProjectedPart::Permission(permission) => {
            add_permission(activity, permission, pending_permissions)
        }
        ProjectedPart::Task(task) => add_task(
            task,
            orchestration_tasks,
            turn_terminals,
            agent_id,
            Some(request_id),
        ),
        ProjectedPart::Compaction(_)
        | ProjectedPart::Artifact(_)
        | ProjectedPart::Lifecycle(_)
        | ProjectedPart::PolicyViolation(_)
        | ProjectedPart::UiIntent(_) => {}
    }
}

pub(super) fn apply_system_part(
    part: &ProjectedPart,
    pending_permissions: &mut BTreeMap<String, PendingPermission>,
    orchestration_tasks: &mut BTreeMap<String, OrchestrationTaskRow>,
    turn_terminals: &mut BTreeMap<String, SettledTurnTerminal>,
    agent_id: Option<&str>,
    request_id: Option<&str>,
) {
    match part {
        ProjectedPart::Permission(permission)
            if permission.state == ProjectedPermissionState::Pending =>
        {
            pending_permissions.insert(
                permission.permission_id.clone(),
                pending_permission(permission),
            );
        }
        ProjectedPart::Task(task) => add_task(
            task,
            orchestration_tasks,
            turn_terminals,
            agent_id,
            request_id,
        ),
        ProjectedPart::Text(_)
        | ProjectedPart::Reasoning(_)
        | ProjectedPart::ToolCall(_)
        | ProjectedPart::Permission(_)
        | ProjectedPart::Compaction(_)
        | ProjectedPart::Artifact(_)
        | ProjectedPart::Lifecycle(_)
        | ProjectedPart::PolicyViolation(_)
        | ProjectedPart::UiIntent(_) => {}
    }
}

pub(super) fn tool_entry(tool: &ProjectedToolCallPart) -> ToolCallEntry {
    let permissions = tool.permissions.iter().map(permission_entry).collect();
    let metadata = tool.metadata.as_ref();
    ToolCallEntry {
        tool_call_id: tool.tool_call_id.to_string(),
        tool_id: tool.tool_id.clone(),
        canonical_tool_id: metadata.and_then(|value| value.canonical_tool_id.clone()),
        alias_source_tool_id: metadata.and_then(|value| value.alias_source_tool_id.clone()),
        resolved_tool_identity: None,
        args_summary: tool.args_summary.clone(),
        args_digest: tool.args_digest.clone(),
        lifecycle_state: Some(tool_lifecycle(tool.state)),
        status: tool_status(tool.state, &tool.permissions),
        output_summary: tool.output_summary.clone(),
        output_digest: tool.output_digest.clone(),
        output_json: tool.output_json.clone(),
        truncated_output: None,
        edit: None,
        lineage: tool.lineage.as_ref().map(lineage_entry),
        artifact_refs: tool
            .artifacts
            .iter()
            .map(|artifact| ToolArtifactEntry {
                path: artifact.path.clone(),
                digest: artifact.digest.clone(),
            })
            .collect(),
        timing_elapsed_ms: metadata
            .and_then(|value| value.timing.as_ref())
            .and_then(|timing| timing.elapsed_ms),
        permissions,
        first_seq: tool.provenance.first_seq,
        last_seq: tool.provenance.last_seq,
        first_mono_ms: tool.provenance.first_seq,
        last_mono_ms: tool.provenance.last_seq,
        first_timestamp: None,
        last_timestamp: None,
    }
}

const fn tool_lifecycle(state: ProjectedToolCallState) -> ToolCallLifecycleState {
    match state {
        ProjectedToolCallState::Pending => ToolCallLifecycleState::Pending,
        ProjectedToolCallState::Running => ToolCallLifecycleState::Running,
        ProjectedToolCallState::Succeeded => ToolCallLifecycleState::Completed,
        ProjectedToolCallState::Failed => ToolCallLifecycleState::Error,
    }
}

fn tool_status(
    state: ProjectedToolCallState,
    permissions: &[ProjectedPermissionPart],
) -> ToolCallDisplayStatus {
    if permissions
        .iter()
        .any(|permission| permission.state == ProjectedPermissionState::Pending)
    {
        return ToolCallDisplayStatus::PendingPermission;
    }
    match state {
        ProjectedToolCallState::Pending => ToolCallDisplayStatus::Queued,
        ProjectedToolCallState::Running => ToolCallDisplayStatus::Running,
        ProjectedToolCallState::Succeeded => ToolCallDisplayStatus::Succeeded,
        ProjectedToolCallState::Failed => ToolCallDisplayStatus::Failed,
    }
}

pub(super) fn add_permission(
    activity: &mut ActivityEntry,
    permission: &ProjectedPermissionPart,
    pending_permissions: &mut BTreeMap<String, PendingPermission>,
) {
    activity.last_seq = activity.last_seq.max(permission.provenance.last_seq);
    if let Some(tool_call_id) = permission.tool_call_id.as_ref() {
        if let Some(tool) = activity
            .tool_calls
            .iter_mut()
            .find(|tool| tool.tool_call_id == tool_call_id.as_str())
        {
            if !tool
                .permissions
                .iter()
                .any(|entry| entry.permission_id == permission.permission_id)
            {
                tool.permissions.push(permission_entry(permission));
            }
            tool.status = tool_status_from_permissions(tool.status, &tool.permissions);
        }
    } else {
        activity.permissions.push(permission_entry(permission));
    }
    if permission.state == ProjectedPermissionState::Pending {
        pending_permissions.insert(
            permission.permission_id.clone(),
            pending_permission(permission),
        );
    }
}

fn tool_status_from_permissions(
    current: ToolCallDisplayStatus,
    permissions: &[PermissionEntry],
) -> ToolCallDisplayStatus {
    if permissions
        .iter()
        .any(|permission| permission.resolved_decision.is_none())
    {
        ToolCallDisplayStatus::PendingPermission
    } else {
        current
    }
}

fn permission_entry(permission: &ProjectedPermissionPart) -> PermissionEntry {
    PermissionEntry {
        permission_id: permission.permission_id.clone(),
        kind: permission.kind.clone(),
        tool_call_id: permission.tool_call_id.as_ref().map(ToString::to_string),
        summary: permission.summary.clone(),
        request_digest: permission.request_digest.clone(),
        timeout_ms: permission.timeout_ms,
        default_decision: permission.default_decision,
        resolved_decision: permission.decision,
        resolution_reason: permission.reason.clone(),
        first_seq: permission.provenance.first_seq,
        last_seq: permission.provenance.last_seq,
    }
}

fn pending_permission(permission: &ProjectedPermissionPart) -> PendingPermission {
    PendingPermission {
        seq: permission.provenance.first_seq,
        kind: permission.kind.clone(),
        summary: permission.summary.clone(),
        request_digest: permission.request_digest.clone(),
        timeout_ms: permission.timeout_ms,
        default_decision: permission.default_decision,
        tool_call_id: permission.tool_call_id.as_ref().map(ToString::to_string),
    }
}

fn lineage_entry(lineage: &SessionLineageProjection) -> TaskLineageEntry {
    TaskLineageEntry {
        parent_tool_call_id: lineage.parent_tool_call_id.clone(),
        parent_task_id: lineage.parent_task_id.clone(),
        parent_request_id: lineage.parent_request_id.clone(),
        child_session_id: lineage.child_session_id.clone(),
        child_request_id: lineage.child_request_id.clone(),
    }
}
