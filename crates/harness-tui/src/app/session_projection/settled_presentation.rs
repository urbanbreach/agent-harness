use std::collections::{BTreeMap, VecDeque};

use harness_core::event::{
    ActorKind, ProviderRequestStartedEvent, ToolCallLifecycleState, UserMessageSubmittedEvent,
};
use harness_core::session::{
    CanonicalEditPayload, CanonicalLegacyCompaction, CanonicalLegacyCompactionStatus,
    CanonicalProviderFragmentKind,
};
use harness_core::transcript_projection::{
    CompactionCheckpointStatus, ProjectedMessageRole, ProjectedMessageState, ProjectedPart,
    ProjectedPermissionPart, ProjectedPermissionState, ProjectedTaskState, ProjectedToolCallPart,
    ProjectedToolCallState, SessionLineageProjection,
};

use super::*;
use crate::app::permissions::PermissionEntry;
use crate::app::{TaskLineageEntry, ToolArtifactEntry};

mod canonical_orchestration;
mod canonical_provider;
mod compaction;
mod parts;
mod presentation_merge;
mod tasks;

use self::canonical_orchestration::*;
use self::canonical_provider::*;
use self::parts::*;
use self::presentation_merge::*;
use self::tasks::*;

impl SessionProjection {
    pub(super) fn rebuild_settled_presentation(&mut self) {
        let Some(canonical) = self.canonical_projection.as_ref() else {
            return;
        };
        let transcript = canonical.transcript.clone();
        let run_summary = canonical.run_summary.clone();
        let legacy_compaction = canonical.latest_legacy_compaction();
        let presentation_enrichment = std::mem::take(&mut self.activities);
        let presentation_orchestration = std::mem::take(&mut self.orchestration_tasks);
        let mut settled_activities = VecDeque::new();
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
                    let index = settled_activities.len();
                    settled_activities.push_back(new_streaming_activity_entry(
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
                            let index = settled_activities.len();
                            settled_activities.push_back(new_streaming_activity_entry(
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
                let activity = &mut settled_activities[index];
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
                    if let ProjectedPart::Permission(permission) = part {
                        if let Some(index) = message
                            .request_id
                            .as_ref()
                            .and_then(|id| activity_by_request.get(id.as_str()))
                            .copied()
                        {
                            add_permission(
                                &mut settled_activities[index],
                                permission,
                                &mut pending_permissions,
                            );
                            continue;
                        }
                    }
                    if let ProjectedPart::ToolCall(tool) = part {
                        if let Some(activity) = settled_activities.back_mut() {
                            activity.tool_calls.push(tool_entry(tool));
                            activity.last_seq = activity.last_seq.max(tool.provenance.last_seq);
                            continue;
                        }
                    }
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

        let (latest_request_budget, provider_context_usage) =
            apply_canonical_provider_presentation(canonical, &mut settled_activities);
        for (index, activity) in settled_activities.iter_mut().enumerate() {
            let is_user_only = activity.user_message.is_some()
                && activity.request_data.is_none()
                && activity.transcript_text.is_empty()
                && activity.tool_calls.is_empty();
            if is_user_only {
                activity.status = if index == 0 {
                    ActivityStatus::Streaming
                } else {
                    ActivityStatus::Queued
                };
            }
        }
        apply_canonical_background_notifications(
            canonical,
            &mut settled_activities,
            &mut orchestration_tasks,
        );
        apply_canonical_stale_detections(canonical, &mut orchestration_tasks);
        apply_canonical_edits(canonical, &mut settled_activities);

        merge_presentation_enrichment(&mut settled_activities, &presentation_enrichment);
        merge_orchestration_presentation(&mut orchestration_tasks, &presentation_orchestration);
        let mut activities = settled_activities;
        apply_turn_terminals(
            &mut activities,
            &turn_terminals,
            &mut self.completed_turn_request_ids,
            &mut self.terminal_elapsed_ms,
        );
        for activity in &mut activities {
            let has_active_turn_task = orchestration_tasks.values().any(|task| {
                !task.state.is_terminal()
                    && task
                        .queue_key
                        .as_deref()
                        .is_some_and(|queue_key| queue_key.starts_with("provider_model:"))
                    && (task.request_id.as_deref() == Some(activity.request_id.as_str())
                        || task.child_request_id.as_deref() == Some(activity.request_id.as_str()))
            });
            if has_active_turn_task && activity.status == ActivityStatus::Done {
                activity.status = ActivityStatus::Streaming;
            }
        }
        self.run_terminal_seen = run_summary.status != harness_core::proj::RunStatus::Running;
        if let Some(activity) = activities.back_mut() {
            match run_summary.status {
                harness_core::proj::RunStatus::Failed => {
                    activity.status = ActivityStatus::Error;
                    activity.error_message.clone_from(&run_summary.last_error);
                }
                harness_core::proj::RunStatus::Finished
                    if activity.status == ActivityStatus::Streaming =>
                {
                    activity.status = ActivityStatus::Done;
                }
                harness_core::proj::RunStatus::Running
                | harness_core::proj::RunStatus::Finished => {}
            }
        }
        self.activities = activities;
        self.latest_request_budget = latest_request_budget;
        if let Some(context_usage) = provider_context_usage {
            self.active_context_usage = Some(context_usage);
        }
        self.pending_permissions = pending_permissions;
        self.orchestration_tasks = orchestration_tasks;
        self.enforce_orchestration_retention();
        self.rebuild_compaction_presentation(&transcript.compaction_checkpoints);
        if self.compaction_status.is_none() {
            self.rebuild_legacy_compaction_presentation(legacy_compaction.as_ref());
        }
        self.enforce_transcript_memory_cap();
        self.transcript_delta = ProjectionDelta::FullRebuild;
    }
}
