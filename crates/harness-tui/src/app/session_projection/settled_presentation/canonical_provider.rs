use super::*;

pub(super) fn apply_canonical_provider_presentation(
    events: &[EventEnvelopeV1],
    transcript: &harness_core::transcript_projection::TranscriptProjection,
    activities: &mut VecDeque<ActivityEntry>,
) -> (
    Option<(u64, Option<RequestBudgetSnapshot>)>,
    Option<ActiveContextUsage>,
) {
    let mut latest_request_budget = None;
    for event in events {
        let EventV1::ProviderRequestStarted(start) = &event.payload else {
            continue;
        };
        let turn_request_id = event.correlation_id.as_deref();
        let provider_request_id = start.request_id.as_str();
        if latest_request_budget
            .as_ref()
            .is_none_or(|(seq, _)| event.seq >= *seq)
        {
            latest_request_budget = Some((
                event.seq,
                start
                    .metadata
                    .as_ref()
                    .and_then(|metadata| metadata.context_budget),
            ));
        }
        let request_id = turn_request_id.unwrap_or(provider_request_id);
        let activity_index =
            provider_activity_index(activities, turn_request_id, provider_request_id)
                .unwrap_or_else(|| {
                    let index = activities.len();
                    activities.push_back(new_streaming_activity_entry(
                        NewStreamingActivityEntryArgs {
                            request_id: request_id.to_string(),
                            profile_label: event.actor.agent_id.as_deref().map_or_else(
                                || "default".to_string(),
                                |agent_id| profile_label(transcript, Some(agent_id)),
                            ),
                            model_id: start.model_id.to_string(),
                            provider_id: start.provider_id.to_string(),
                            user_message: None,
                            user_timestamp: None,
                            request_data: None,
                            transcript_text: String::new(),
                            first_seq: event.seq,
                            first_mono_ms: event.mono_ms,
                        },
                    ));
                    index
                });
        let activity = &mut activities[activity_index];
        activity.provider_id = start.provider_id.to_string();
        activity.model_id = start.model_id.to_string();
        activity.request_data = Some(start.clone());
        activity.request_started_mono_ms = Some(event.mono_ms);
    }

    for fragment in events
        .iter()
        .filter_map(canonical_provider_fragment_for_event)
    {
        let Some(activity) =
            provider_activity_mut(activities, fragment.turn_request_id, fragment.request_id)
        else {
            continue;
        };
        match fragment.kind {
            CanonicalProviderFragmentKind::Reasoning => {
                activity
                    .thinking_first_mono_ms
                    .get_or_insert(fragment.mono_ms);
                activity.thinking_last_mono_ms = Some(fragment.mono_ms);
            }
            CanonicalProviderFragmentKind::Text => {
                activity.first_delta_mono_ms.get_or_insert(fragment.mono_ms);
                if activity.thinking_first_mono_ms.is_some() {
                    activity.thinking_last_mono_ms = Some(fragment.mono_ms);
                }
            }
        }
    }

    let mut active_context_usage = None;
    for event in events {
        let EventV1::ProviderRequestFinished(finish) = &event.payload else {
            continue;
        };
        let Some(activity) = provider_activity_mut(
            activities,
            event.correlation_id.as_deref(),
            finish.request_id.as_str(),
        ) else {
            continue;
        };
        if let Some(usage) = finish.usage.as_ref() {
            activity.usage = Some(ActivityUsage {
                prompt_tokens: usage.prompt_tokens,
                completion_tokens: usage.completion_tokens,
                total_tokens: usage.total_tokens,
            });
            let context_tokens = if usage.prompt_tokens > 0 {
                usage.prompt_tokens
            } else {
                usage.total_tokens
            };
            active_context_usage = Some(ActiveContextUsage::estimate(context_tokens));
        }
        activity.cache_usage = finish.metadata.as_ref().and_then(|metadata| {
            match (metadata.cache_read_tokens, metadata.cache_write_tokens) {
                (None, None) => None,
                (read_tokens, write_tokens) => Some(ActivityCacheUsage {
                    read_tokens: read_tokens.unwrap_or(0),
                    write_tokens: write_tokens.unwrap_or(0),
                }),
            }
        });
        if activity.status == ActivityStatus::Error {
            activity.error_message = provider_error_detail(finish);
        }
        activity.last_seq = activity.last_seq.max(event.seq);
        activity.last_mono_ms = event.mono_ms;
    }
    (latest_request_budget, active_context_usage)
}
