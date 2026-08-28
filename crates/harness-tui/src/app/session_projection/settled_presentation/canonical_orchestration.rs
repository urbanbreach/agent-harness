use super::*;

pub(super) fn apply_canonical_background_notifications(
    canonical: &CanonicalSessionProjection,
    activities: &mut VecDeque<ActivityEntry>,
    tasks: &mut BTreeMap<String, OrchestrationTaskRow>,
) {
    for notification in canonical.background_notifications() {
        let data = notification.payload;
        let request_id = data
            .delivered_turn_request_id
            .as_deref()
            .unwrap_or(data.child_request_id.as_str());
        if !activities
            .iter()
            .any(|activity| activity.request_id == request_id)
        {
            let mut activity = new_streaming_activity_entry(NewStreamingActivityEntryArgs {
                request_id: request_id.to_string(),
                profile_label: profile_label(
                    &canonical.transcript,
                    data.parent_agent_id
                        .as_deref()
                        .or(notification.actor_agent_id),
                ),
                model_id: String::new(),
                provider_id: String::new(),
                user_message: Some(UserMessageSubmittedEvent {
                    request_id: request_id.into(),
                    text: background_task_notification_text(data),
                }),
                user_timestamp: notification.timestamp.map(str::to_string),
                request_data: None,
                transcript_text: String::new(),
                first_seq: notification.seq,
                first_mono_ms: notification.mono_ms,
            });
            activity.status = if data.delivered_turn_request_id.is_some() {
                ActivityStatus::Queued
            } else {
                ActivityStatus::Done
            };
            activities.push_back(activity);
        }

        let row = tasks
            .entry(data.task_id.to_string())
            .or_insert_with(|| OrchestrationTaskRow {
                task_id: data.task_id.to_string(),
                queue_key: None,
                state: OrchestrationTaskState::Running,
                warning: None,
                owner_kind: notification.actor_kind,
                owner_agent_id: notification.actor_agent_id.map(str::to_string),
                request_id: notification.correlation_id.map(str::to_string),
                parent_tool_call_id: None,
                parent_request_id: None,
                child_session_id: Some(data.child_session_id.to_string()),
                child_request_id: Some(data.child_request_id.clone()),
                result_summary: None,
                child_tool_call_count: 0,
                current_child_tool_title: None,
                timing_elapsed_ms: None,
                first_seq: notification.seq,
                last_seq: notification.seq,
                first_mono_ms: notification.mono_ms,
                last_mono_ms: notification.mono_ms,
                first_timestamp: notification.timestamp.map(str::to_string),
                last_timestamp: notification.timestamp.map(str::to_string),
            });
        row.child_session_id = Some(data.child_session_id.to_string());
        row.child_request_id = Some(data.child_request_id.clone());
        row.result_summary = non_empty_preserved_string(&data.summary);
        row.state = match data.status {
            harness_core::event::BackgroundTaskNotificationStatus::Completed => {
                OrchestrationTaskState::Completed
            }
            harness_core::event::BackgroundTaskNotificationStatus::Cancelled => {
                OrchestrationTaskState::Cancelled
            }
            harness_core::event::BackgroundTaskNotificationStatus::Failed => {
                OrchestrationTaskState::Failed
            }
            harness_core::event::BackgroundTaskNotificationStatus::TimedOut => {
                OrchestrationTaskState::TimedOut
            }
        };
        row.warning = match data.status {
            harness_core::event::BackgroundTaskNotificationStatus::Completed => None,
            _ => Some(data.status.as_str().replace('_', " ")),
        };
        row.last_seq = notification.seq;
        row.last_mono_ms = notification.mono_ms;
        row.last_timestamp = notification.timestamp.map(str::to_string);
    }
}

pub(super) fn apply_canonical_stale_detections(
    canonical: &CanonicalSessionProjection,
    tasks: &mut BTreeMap<String, OrchestrationTaskRow>,
) {
    for stale in canonical.stale_detections() {
        let Some(task) = tasks.get_mut(stale.task_id) else {
            continue;
        };
        if task.last_seq > stale.seq {
            continue;
        }
        task.state = OrchestrationTaskState::Stale;
        task.warning = Some(format!("stale for {} ms", stale.stale_for_ms));
        task.last_seq = stale.seq;
        task.last_mono_ms = stale.mono_ms;
        task.last_timestamp = stale.timestamp.map(str::to_string);
    }
}

pub(super) fn apply_canonical_edits(
    canonical: &CanonicalSessionProjection,
    activities: &mut VecDeque<ActivityEntry>,
) {
    for event in canonical.edit_events() {
        let Some(tool_call_id) = event.tool_call_id else {
            continue;
        };
        let Some(tool) = activities
            .iter_mut()
            .flat_map(|activity| activity.tool_calls.iter_mut())
            .find(|tool| tool.tool_call_id == tool_call_id)
        else {
            continue;
        };
        match event.payload {
            CanonicalEditPayload::Proposed(data) => {
                tool.edit = Some(EditEntry {
                    edit_id: data.edit_id.clone(),
                    path: data.path.clone(),
                    status: EditDisplayStatus::Proposed,
                    summary: Some(data.summary.clone()),
                    patch_digest: Some(data.patch_digest.clone()),
                    new_file_digest: None,
                    diff_rel_path: None,
                    diff_digest: None,
                    rejection_reason: None,
                });
            }
            CanonicalEditPayload::Applied(data) => {
                let prior = tool.edit.take();
                tool.edit = Some(EditEntry {
                    edit_id: data.edit_id.clone(),
                    path: data.path.clone(),
                    status: EditDisplayStatus::Applied,
                    summary: prior.as_ref().and_then(|edit| edit.summary.clone()),
                    patch_digest: prior.and_then(|edit| edit.patch_digest),
                    new_file_digest: Some(data.new_file_digest.clone()),
                    diff_rel_path: data.diff_rel_path.clone(),
                    diff_digest: data.diff_digest.clone(),
                    rejection_reason: None,
                });
            }
            CanonicalEditPayload::Rejected(data) => {
                let prior = tool.edit.take();
                tool.edit = Some(EditEntry {
                    edit_id: data.edit_id.clone(),
                    path: data.path.clone(),
                    status: EditDisplayStatus::Rejected,
                    summary: prior.as_ref().and_then(|edit| edit.summary.clone()),
                    patch_digest: prior.and_then(|edit| edit.patch_digest),
                    new_file_digest: None,
                    diff_rel_path: None,
                    diff_digest: None,
                    rejection_reason: Some(data.reason.clone()),
                });
            }
        }
        tool.last_seq = tool.last_seq.max(event.seq);
    }
}
