use std::collections::{BTreeMap, VecDeque};

use harness_core::event::{
    ActorKind, ProviderRequestStartedEvent, ToolCallLifecycleState, UserMessageSubmittedEvent,
};
use harness_core::transcript_projection::{
    CompactionCheckpointStatus, ProjectedMessageRole, ProjectedMessageState, ProjectedPart,
    ProjectedPermissionPart, ProjectedPermissionState, ProjectedTaskState, ProjectedToolCallPart,
    ProjectedToolCallState, SessionLineageProjection,
};

use super::*;
use crate::app::permissions::PermissionEntry;
use crate::app::{TaskLineageEntry, ToolArtifactEntry};

impl SessionProjection {
    pub(super) fn rebuild_settled_presentation(&mut self) {
        let Some(canonical) = self.canonical_projection.as_ref() else {
            return;
        };
        let transcript = canonical.transcript.clone();
        let run_summary = canonical.run_summary.clone();
        let presentation_enrichment = std::mem::take(&mut self.activities);
        let mut activities = VecDeque::new();
        let mut activity_by_request = BTreeMap::new();
        let mut pending_permissions = BTreeMap::new();
        let mut orchestration_tasks = BTreeMap::new();
        let mut turn_terminals = BTreeMap::new();

        for message in &transcript.messages {
            let request_id = message
                .request_id
                .as_ref()
                .map_or_else(|| message.message_id.clone(), ToString::to_string);
            let activity_index = match message.role {
                ProjectedMessageRole::User => {
                    let text = message
                        .parts
                        .iter()
                        .filter_map(|part| match part {
                            ProjectedPart::Text(text) => Some(text.text.as_str()),
                            _ => None,
                        })
                        .collect::<String>();
                    let index = activities.len();
                    activities.push_back(new_streaming_activity_entry(
                        NewStreamingActivityEntryArgs {
                            request_id: request_id.clone(),
                            profile_label: profile_label(&transcript, message.agent_id.as_deref()),
                            model_id: String::new(),
                            provider_id: String::new(),
                            user_message: Some(UserMessageSubmittedEvent {
                                request_id: request_id.as_str().into(),
                                text,
                            }),
                            user_timestamp: None,
                            request_data: None,
                            transcript_text: String::new(),
                            first_seq: message.provenance.first_seq,
                            first_mono_ms: message.provenance.first_seq,
                        },
                    ));
                    activity_by_request.insert(request_id.clone(), index);
                    Some(index)
                }
                ProjectedMessageRole::Assistant => {
                    let index = activity_by_request
                        .get(&request_id)
                        .copied()
                        .unwrap_or_else(|| {
                            let index = activities.len();
                            activities.push_back(new_streaming_activity_entry(
                                NewStreamingActivityEntryArgs {
                                    request_id: request_id.clone(),
                                    profile_label: profile_label(
                                        &transcript,
                                        message.agent_id.as_deref(),
                                    ),
                                    model_id: String::new(),
                                    provider_id: String::new(),
                                    user_message: None,
                                    user_timestamp: None,
                                    request_data: None,
                                    transcript_text: String::new(),
                                    first_seq: message.provenance.first_seq,
                                    first_mono_ms: message.provenance.first_seq,
                                },
                            ));
                            activity_by_request.insert(request_id.clone(), index);
                            index
                        });
                    Some(index)
                }
                ProjectedMessageRole::System => None,
            };

            if let Some(index) = activity_index {
                let activity = &mut activities[index];
                activity.last_seq = message.provenance.last_seq;
                activity.last_mono_ms = message.provenance.last_seq;
                activity.status = activity_status(message.state);
                if let Some(provider) = message.provider.as_ref() {
                    activity.provider_id = provider.provider_id.clone().unwrap_or_default();
                    activity.model_id = provider.model_id.clone().unwrap_or_default();
                    activity.request_data = provider.provider_request_id.as_ref().map(|id| {
                        ProviderRequestStartedEvent {
                            request_id: id.as_str().into(),
                            provider_id: activity.provider_id.clone(),
                            model_id: activity.model_id.clone(),
                            prompt_summary: provider.prompt_summary.clone().unwrap_or_default(),
                            request_digest: provider.request_digest.clone().unwrap_or_default(),
                            metadata: None,
                        }
                    });
                }
                if message.role == ProjectedMessageRole::Assistant {
                    for part in &message.parts {
                        apply_message_part(
                            activity,
                            part,
                            &mut pending_permissions,
                            &mut orchestration_tasks,
                            &mut turn_terminals,
                            message.agent_id.as_deref(),
                            &request_id,
                        );
                    }
                }
            } else {
                for part in &message.parts {
                    apply_system_part(
                        part,
                        &mut pending_permissions,
                        &mut orchestration_tasks,
                        &mut turn_terminals,
                        message.agent_id.as_deref(),
                        message.request_id.as_ref().map(|id| id.as_str()),
                    );
                }
            }
        }

        merge_presentation_enrichment(&mut activities, &presentation_enrichment);
        apply_turn_terminals(
            &mut activities,
            &turn_terminals,
            &mut self.completed_turn_request_ids,
            &mut self.terminal_elapsed_ms,
        );
        self.activities = activities;
        self.pending_permissions = pending_permissions;
        self.orchestration_tasks = orchestration_tasks;
        self.run_terminal_seen = matches!(
            run_summary.status,
            harness_core::proj::RunStatus::Finished | harness_core::proj::RunStatus::Failed
        );
        self.rebuild_compaction_presentation(&transcript.compaction_checkpoints);
        if self.compaction_status.is_none() {
            self.rebuild_legacy_compaction_presentation(&run_summary.counts.by_type);
        }
        self.enforce_transcript_memory_cap();
        self.transcript_delta = ProjectionDelta::FullRebuild;
    }

    fn rebuild_compaction_presentation(
        &mut self,
        checkpoints: &[harness_core::transcript_projection::CompactionCheckpointProjection],
    ) {
        self.compaction_status = None;
        self.compaction_usage_metrics = CompactionUsageMetrics::default();
        let Some(checkpoint) = checkpoints.last() else {
            return;
        };
        let (state, label) = match checkpoint.status {
            CompactionCheckpointStatus::Requested => (CompactionState::Requested, "requested"),
            CompactionCheckpointStatus::Written => (CompactionState::Written, "written"),
            CompactionCheckpointStatus::Failed => (CompactionState::Failed, "failed"),
            CompactionCheckpointStatus::Applied
            | CompactionCheckpointStatus::SessionCompacted
            | CompactionCheckpointStatus::BranchSummary => (CompactionState::Applied, "applied"),
        };
        if state == CompactionState::Applied {
            self.compaction_usage_metrics.completed_count = 1;
            self.compaction_usage_metrics.summary_tokens_estimate =
                u64::from(checkpoint.summary_tokens_estimate.unwrap_or(0));
            self.compaction_usage_metrics.reduction_tokens_estimate =
                u64::from(checkpoint.reduction_tokens_estimate.unwrap_or(0));
        }
        self.compaction_usage_metrics.last_tokens_before_estimate = checkpoint
            .tokens_before_estimate
            .or(checkpoint.tokens_before);
        self.compaction_usage_metrics.last_tokens_after_estimate = checkpoint.tokens_after_estimate;
        self.compaction_usage_metrics
            .last_reduction_percent_estimate = checkpoint.reduction_percent_estimate;
        if state == CompactionState::Applied {
            self.active_context_usage = Some(
                checkpoint
                    .tokens_after_estimate
                    .map(ActiveContextUsage::estimate)
                    .unwrap_or_else(ActiveContextUsage::compacted_pending_refresh),
            );
        }
        self.compaction_status = Some(CompactionStatus {
            agent_id: checkpoint.agent_id.clone(),
            checkpoint_id: checkpoint.checkpoint_id.clone(),
            trigger_reason: checkpoint
                .trigger_reason
                .clone()
                .unwrap_or_else(|| label.to_string()),
            state,
            message: format!("compaction {label}"),
        });
    }

    fn rebuild_legacy_compaction_presentation(&mut self, counts: &BTreeMap<String, u64>) {
        let status = [
            ("compaction_failed", CompactionState::Failed, "failed"),
            ("compaction_applied", CompactionState::Applied, "applied"),
            ("compaction_written", CompactionState::Written, "written"),
            (
                "compaction_requested",
                CompactionState::Requested,
                "requested",
            ),
        ]
        .into_iter()
        .find(|(event_type, _, _)| counts.get(*event_type).is_some_and(|count| *count > 0));
        let Some((_, state, label)) = status else {
            return;
        };
        if state == CompactionState::Applied {
            self.active_context_usage = Some(ActiveContextUsage::compacted_pending_refresh());
        }
        self.compaction_status = Some(CompactionStatus {
            agent_id: "legacy".to_string(),
            checkpoint_id: None,
            trigger_reason: "legacy_compatibility".to_string(),
            state,
            message: format!("compaction {label} · legacy compatibility"),
        });
    }
}

fn merge_presentation_enrichment(
    activities: &mut VecDeque<ActivityEntry>,
    prior: &VecDeque<ActivityEntry>,
) {
    for activity in activities.iter_mut() {
        let Some(existing) = prior
            .iter()
            .find(|candidate| candidate.request_id == activity.request_id)
        else {
            continue;
        };
        activity.user_timestamp.clone_from(&existing.user_timestamp);
        activity.request_data.clone_from(&existing.request_data);
        activity.thinking_first_mono_ms = existing.thinking_first_mono_ms;
        activity.thinking_last_mono_ms = existing.thinking_last_mono_ms;
        activity.first_delta_mono_ms = existing.first_delta_mono_ms;
        activity.usage = existing.usage;
        activity.cache_usage = existing.cache_usage;
        activity.error_message.clone_from(&existing.error_message);
        activity.first_mono_ms = existing.first_mono_ms;
        activity.last_mono_ms = existing.last_mono_ms;
        activity.request_started_mono_ms = existing.request_started_mono_ms;
        activity.revision = existing.revision;
    }

    for tool in activities
        .iter_mut()
        .flat_map(|activity| activity.tool_calls.iter_mut())
    {
        let Some(existing) = prior
            .iter()
            .flat_map(|activity| activity.tool_calls.iter())
            .find(|candidate| candidate.tool_call_id == tool.tool_call_id)
        else {
            continue;
        };
        tool.edit.clone_from(&existing.edit);
        tool.truncated_output.clone_from(&existing.truncated_output);
        tool.resolved_tool_identity
            .clone_from(&existing.resolved_tool_identity);
        tool.first_timestamp.clone_from(&existing.first_timestamp);
        tool.last_timestamp.clone_from(&existing.last_timestamp);
        if tool.timing_elapsed_ms.is_none() {
            tool.timing_elapsed_ms = existing.timing_elapsed_ms;
        }
    }
}

fn profile_label(
    transcript: &harness_core::transcript_projection::TranscriptProjection,
    agent_id: Option<&str>,
) -> String {
    agent_id
        .and_then(|id| transcript.session.agent_profiles.get(id))
        .cloned()
        .unwrap_or_else(|| "default".to_string())
}

const fn activity_status(state: ProjectedMessageState) -> ActivityStatus {
    match state {
        ProjectedMessageState::Complete => ActivityStatus::Done,
        ProjectedMessageState::Streaming | ProjectedMessageState::Incomplete => {
            ActivityStatus::Streaming
        }
        ProjectedMessageState::Failed => ActivityStatus::Error,
    }
}

fn apply_message_part(
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

fn apply_system_part(
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

fn tool_entry(tool: &ProjectedToolCallPart) -> ToolCallEntry {
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

fn add_permission(
    activity: &mut ActivityEntry,
    permission: &ProjectedPermissionPart,
    pending_permissions: &mut BTreeMap<String, PendingPermission>,
) {
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

fn add_task(
    task: &harness_core::transcript_projection::ProjectedTaskPart,
    rows: &mut BTreeMap<String, OrchestrationTaskRow>,
    turn_terminals: &mut BTreeMap<String, SettledTurnTerminal>,
    agent_id: Option<&str>,
    request_id: Option<&str>,
) {
    let lineage = task.lineage.as_ref();
    let task_id = task.task_id.to_string();
    let row = rows
        .entry(task_id.clone())
        .or_insert_with(|| OrchestrationTaskRow {
            task_id: task.task_id.to_string(),
            queue_key: task.queue_key.clone(),
            state: task_state(task.state),
            warning: task.reason.clone(),
            owner_kind: agent_id.map_or(ActorKind::System, |_| ActorKind::Worker),
            owner_agent_id: agent_id.map(str::to_string),
            request_id: request_id.map(str::to_string),
            parent_tool_call_id: lineage.and_then(|value| value.parent_tool_call_id.clone()),
            parent_request_id: lineage.and_then(|value| value.parent_request_id.clone()),
            child_session_id: lineage.and_then(|value| value.child_session_id.clone()),
            child_request_id: lineage.and_then(|value| value.child_request_id.clone()),
            result_summary: task.result_summary.clone(),
            child_tool_call_count: 0,
            current_child_tool_title: None,
            timing_elapsed_ms: task.timing_elapsed_ms,
            first_seq: task.provenance.first_seq,
            last_seq: task.provenance.last_seq,
            first_mono_ms: task.provenance.first_seq,
            last_mono_ms: task.provenance.last_seq,
            first_timestamp: None,
            last_timestamp: None,
        });
    row.state = task_state(task.state);
    if task.queue_key.is_some() {
        row.queue_key.clone_from(&task.queue_key);
    }
    if task.reason.is_some() {
        row.warning.clone_from(&task.reason);
    }
    if task.result_summary.is_some() {
        row.result_summary.clone_from(&task.result_summary);
    }
    if let Some(lineage) = lineage {
        if lineage.parent_tool_call_id.is_some() {
            row.parent_tool_call_id
                .clone_from(&lineage.parent_tool_call_id);
        }
        if lineage.parent_request_id.is_some() {
            row.parent_request_id.clone_from(&lineage.parent_request_id);
        }
        if lineage.child_session_id.is_some() {
            row.child_session_id.clone_from(&lineage.child_session_id);
        }
        if lineage.child_request_id.is_some() {
            row.child_request_id.clone_from(&lineage.child_request_id);
        }
    }
    if request_id.is_some() {
        row.request_id = request_id.map(str::to_string);
    }
    if task.timing_elapsed_ms.is_some() {
        row.timing_elapsed_ms = task.timing_elapsed_ms;
    }
    row.last_seq = task.provenance.last_seq;
    row.last_mono_ms = task.terminal_mono_ms.unwrap_or(task.provenance.last_seq);

    if task.terminal_scope == Some(harness_core::event::TaskTerminalScope::AgentTurn) {
        if let Some(request_id) = request_id {
            turn_terminals.insert(
                request_id.to_string(),
                SettledTurnTerminal {
                    state: task.state,
                    reason: task.reason.clone(),
                    elapsed_ms: task.timing_elapsed_ms,
                    terminal_mono_ms: task.terminal_mono_ms,
                },
            );
        }
    }
}

#[derive(Debug, Clone)]
struct SettledTurnTerminal {
    state: ProjectedTaskState,
    reason: Option<String>,
    elapsed_ms: Option<u64>,
    terminal_mono_ms: Option<u64>,
}

fn apply_turn_terminals(
    activities: &mut VecDeque<ActivityEntry>,
    terminals: &BTreeMap<String, SettledTurnTerminal>,
    completed: &mut std::collections::BTreeSet<String>,
    elapsed: &mut BTreeMap<String, u64>,
) {
    for (request_id, terminal) in terminals {
        let Some(activity) = activities
            .iter_mut()
            .find(|activity| activity.request_id == *request_id)
        else {
            continue;
        };
        if let Some(terminal_mono_ms) = terminal.terminal_mono_ms {
            activity.last_mono_ms = terminal_mono_ms;
        }
        if let Some(value) = terminal.elapsed_ms.or_else(|| {
            terminal
                .terminal_mono_ms
                .map(|terminal| terminal.saturating_sub(activity.first_mono_ms))
        }) {
            elapsed.insert(request_id.clone(), value);
        }
        match terminal.state {
            ProjectedTaskState::Completed => {
                activity.status = ActivityStatus::Done;
                completed.insert(request_id.clone());
            }
            ProjectedTaskState::Cancelled => {
                activity.status = ActivityStatus::Error;
                activity.error_message.clone_from(&terminal.reason);
                completed.insert(request_id.clone());
            }
            ProjectedTaskState::Queued
            | ProjectedTaskState::Started
            | ProjectedTaskState::LateResult => {}
        }
    }
}

const fn task_state(state: ProjectedTaskState) -> OrchestrationTaskState {
    match state {
        ProjectedTaskState::Queued => OrchestrationTaskState::Queued,
        ProjectedTaskState::Started => OrchestrationTaskState::Running,
        ProjectedTaskState::Cancelled => OrchestrationTaskState::Cancelled,
        ProjectedTaskState::Completed => OrchestrationTaskState::Completed,
        ProjectedTaskState::LateResult => OrchestrationTaskState::LateResult,
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
