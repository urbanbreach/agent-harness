// allow: SIZE_OK — TUI app state (session projection + interaction)
use super::*;

impl SessionProjection {
    pub(super) fn update_live_presentation_for_event(
        &mut self,
        event: &EventEnvelopeV1,
        historical: bool,
    ) {
        if let Some(fragment) = canonical_provider_fragment_for_event(event) {
            self.update_compatibility_provider_fragment(event, fragment);
            return;
        }
        if self.update_live_lifecycle_event(event, historical) {
            return;
        }
        if self.update_live_orchestration_event(event) {
            return;
        }
        self.update_live_tool_event(event);
    }

    fn update_compatibility_provider_fragment(
        &mut self,
        event: &EventEnvelopeV1,
        fragment: CanonicalProviderFragment<'_>,
    ) {
        self.note_child_agent_request(event, fragment.request_id);
        let turn_id = Self::canonical_provider_turn_id(event, fragment.request_id);
        self.note_child_agent_request(event, turn_id);
        let index = self
            .activity_index_for_provider_event(event, fragment.request_id)
            .unwrap_or_else(|| {
                let index = self.activities.len();
                self.activities.push_back(new_streaming_activity_entry(
                    NewStreamingActivityEntryArgs {
                        request_id: turn_id.to_string(),
                        profile_label: self.profile_label_for_event(event),
                        model_id: String::new(),
                        provider_id: String::new(),
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
        if let Some(activity) = self.activities.get_mut(index) {
            activity.status = ActivityStatus::Streaming;
            match fragment.kind {
                CanonicalProviderFragmentKind::Reasoning => {
                    activity.thinking_text.push_str(fragment.delta);
                    activity.note_thinking_mono(event.mono_ms);
                }
                CanonicalProviderFragmentKind::Text => {
                    if activity.transcript_text.is_empty() && activity.tool_calls.is_empty() {
                        activity.finish_thinking_mono(event.mono_ms);
                    }
                    activity.first_delta_mono_ms.get_or_insert(event.mono_ms);
                    activity.transcript_text.push_str(fragment.delta);
                }
            }
            activity.bump_revision();
            mark_activity_event(activity, event.seq, event.mono_ms);
        }
        self.enforce_transcript_memory_cap();
    }
}
