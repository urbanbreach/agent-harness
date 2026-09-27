use super::*;

pub(super) fn apply_canonical_background_notifications(
    events: &[EventEnvelopeV1],
    transcript: &harness_core::transcript_projection::TranscriptProjection,
    activities: &mut VecDeque<ActivityEntry>,
    tasks: &mut BTreeMap<String, OrchestrationTaskRow>,
) {
    let cancelled = events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::TaskCancelled(task) if !task.failure => Some(task.task_id.as_str()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    for notification in events {
        let EventV1::BackgroundTaskNotification(data) = &notification.payload else {
            continue;
        };
        let request_id = data
            .delivered_turn_request_id
            .as_deref()
            .unwrap_or(data.child_request_id.as_str());
        if data
            .delivered_turn_request_id
            .as_deref()
            .is_some_and(|id| cancelled.contains(id))
            && !activities.iter().any(|activity| {
                activity.request_id == request_id && activity.request_data.is_some()
            })
        {
            activities.retain(|activity| activity.request_id != request_id);
        } else if let Some(activity) = activities
            .iter_mut()
            .find(|activity| activity.request_id == request_id)
        {
            if activity.first_seq >= notification.seq {
                activity.user_message = None;
            }
        } else {
            let mut activity = new_streaming_activity_entry(NewStreamingActivityEntryArgs {
                request_id: request_id.to_string(),
                profile_label: profile_label(
                    transcript,
                    data.parent_agent_id
                        .as_deref()
                        .or(notification.actor.agent_id.as_deref()),
                ),
                model_id: String::new(),
                provider_id: String::new(),
                user_message: None,
                user_timestamp: notification.ts.clone(),
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
                owner_kind: notification.actor.kind,
                owner_agent_id: notification.actor.agent_id.clone(),
                request_id: notification.correlation_id.clone(),
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
                first_timestamp: notification.ts.clone(),
                last_timestamp: notification.ts.clone(),
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
        row.last_timestamp.clone_from(&notification.ts);
    }
}

pub(super) fn apply_canonical_stale_detections(
    events: &[EventEnvelopeV1],
    tasks: &mut BTreeMap<String, OrchestrationTaskRow>,
) {
    for event in events {
        let EventV1::StaleDetected(stale) = &event.payload else {
            continue;
        };
        let Some(task) = tasks.get_mut(stale.task_id.as_str()) else {
            continue;
        };
        if task.last_seq > event.seq {
            continue;
        }
        task.state = OrchestrationTaskState::Stale;
        task.warning = Some(format!("stale for {} ms", stale.stale_for_ms));
        task.last_seq = event.seq;
        task.last_mono_ms = event.mono_ms;
        task.last_timestamp.clone_from(&event.ts);
    }
}
