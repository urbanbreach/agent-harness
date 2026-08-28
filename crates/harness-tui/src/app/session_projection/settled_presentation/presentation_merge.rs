use super::*;

pub(super) fn provider_activity_mut<'a>(
    activities: &'a mut VecDeque<ActivityEntry>,
    turn_request_id: Option<&str>,
    provider_request_id: &str,
) -> Option<&'a mut ActivityEntry> {
    let index = provider_activity_index(activities, turn_request_id, provider_request_id)?;
    activities.get_mut(index)
}

pub(super) fn provider_activity_index(
    activities: &VecDeque<ActivityEntry>,
    turn_request_id: Option<&str>,
    provider_request_id: &str,
) -> Option<usize> {
    activities.iter().position(|activity| {
        turn_request_id.is_some_and(|request_id| activity.request_id == request_id)
            || activity.request_id == provider_request_id
            || activity
                .request_data
                .as_ref()
                .is_some_and(|request| request.request_id.as_str() == provider_request_id)
    })
}

pub(super) fn merge_orchestration_presentation(
    settled: &mut BTreeMap<String, OrchestrationTaskRow>,
    prior: &BTreeMap<String, OrchestrationTaskRow>,
) {
    for (task_id, row) in settled {
        if let Some(existing) = prior.get(task_id) {
            row.owner_kind = existing.owner_kind;
            row.owner_agent_id.clone_from(&existing.owner_agent_id);
            row.child_tool_call_count = existing.child_tool_call_count;
            row.current_child_tool_title
                .clone_from(&existing.current_child_tool_title);
            if row.timing_elapsed_ms.is_none() {
                row.timing_elapsed_ms = existing.timing_elapsed_ms;
            }
            row.first_mono_ms = existing.first_mono_ms;
            row.last_mono_ms = existing.last_mono_ms;
            row.first_timestamp.clone_from(&existing.first_timestamp);
            row.last_timestamp.clone_from(&existing.last_timestamp);
        }
    }
}

pub(super) fn merge_presentation_enrichment(
    activities: &mut VecDeque<ActivityEntry>,
    prior: &VecDeque<ActivityEntry>,
) {
    for activity in activities.iter_mut() {
        let Some(existing) = prior.iter().find(|candidate| {
            candidate.request_id == activity.request_id
                || activity.request_data.as_ref().is_some_and(|request| {
                    candidate
                        .request_data
                        .as_ref()
                        .is_some_and(|prior_request| prior_request.request_id == request.request_id)
                })
        }) else {
            continue;
        };
        activity.user_timestamp.clone_from(&existing.user_timestamp);
        if activity.user_message.is_none() {
            activity.user_message.clone_from(&existing.user_message);
        }
        activity.request_data.clone_from(&existing.request_data);
        if activity.thinking_first_mono_ms.is_none() {
            activity.thinking_first_mono_ms = existing.thinking_first_mono_ms;
        }
        if activity.thinking_last_mono_ms.is_none() {
            activity.thinking_last_mono_ms = existing.thinking_last_mono_ms;
        }
        if activity.first_delta_mono_ms.is_none() {
            activity.first_delta_mono_ms = existing.first_delta_mono_ms;
        }
        if activity.usage.is_none() {
            activity.usage = existing.usage;
        }
        if activity.cache_usage.is_none() {
            activity.cache_usage = existing.cache_usage;
        }
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

    for local_echo in prior
        .iter()
        .filter(|activity| activity.request_id.is_empty())
    {
        activities.push_back(clone_local_prompt_echo(local_echo));
    }
}

fn clone_local_prompt_echo(activity: &ActivityEntry) -> ActivityEntry {
    ActivityEntry {
        request_id: activity.request_id.clone(),
        profile_label: activity.profile_label.clone(),
        model_id: activity.model_id.clone(),
        provider_id: activity.provider_id.clone(),
        status: activity.status,
        user_message: activity.user_message.clone(),
        user_timestamp: activity.user_timestamp.clone(),
        request_data: activity.request_data.clone(),
        thinking_text: activity.thinking_text.clone(),
        thinking_first_mono_ms: activity.thinking_first_mono_ms,
        thinking_last_mono_ms: activity.thinking_last_mono_ms,
        transcript_text: activity.transcript_text.clone(),
        first_delta_mono_ms: activity.first_delta_mono_ms,
        usage: activity.usage,
        cache_usage: activity.cache_usage,
        error_message: activity.error_message.clone(),
        permissions: activity.permissions.clone(),
        tool_calls: activity.tool_calls.clone(),
        first_seq: activity.first_seq,
        last_seq: activity.last_seq,
        first_mono_ms: activity.first_mono_ms,
        last_mono_ms: activity.last_mono_ms,
        request_started_mono_ms: activity.request_started_mono_ms,
        revision: activity.revision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn activity(request_id: &str, status: ActivityStatus) -> ActivityEntry {
        let mut entry = new_streaming_activity_entry(NewStreamingActivityEntryArgs {
            request_id: request_id.to_string(),
            profile_label: "default".to_string(),
            model_id: String::new(),
            provider_id: String::new(),
            user_message: None,
            user_timestamp: None,
            request_data: None,
            transcript_text: String::new(),
            first_seq: 1,
            first_mono_ms: 1,
        });
        entry.status = status;
        entry
    }

    #[test]
    fn presentation_enrichment_cannot_override_canonical_activity_semantics() {
        // arrange
        let mut settled = VecDeque::from([activity("turn-1", ActivityStatus::Done)]);
        let mut stale = activity("turn-1", ActivityStatus::Error);
        stale.error_message = Some("stale transient failure".to_string());
        let prior = VecDeque::from([stale]);

        // act
        merge_presentation_enrichment(&mut settled, &prior);

        // assert
        assert_eq!(settled[0].status, ActivityStatus::Done);
        assert_eq!(settled[0].error_message, None);
    }

    #[test]
    fn presentation_enrichment_cannot_restore_noncanonical_activities() {
        // arrange
        let settled = VecDeque::from([activity("turn-1", ActivityStatus::Done)]);
        let prior = VecDeque::from([activity("transient-only", ActivityStatus::Streaming)]);

        // act
        let mut activities = settled;
        merge_presentation_enrichment(&mut activities, &prior);

        // assert
        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].request_id, "turn-1");
    }
}
