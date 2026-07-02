use crate::app::ActivityEntry;
use crate::text::has_trimmed_content;

pub(super) fn activity_has_thinking_text(activity: &ActivityEntry) -> bool {
    has_renderable_thinking_text(&activity.thinking_text)
}

pub(super) fn has_renderable_thinking_text(text: &str) -> bool {
    has_trimmed_content(&text.replace("[REDACTED]", ""))
}

pub(super) fn turn_event_matches_activity(
    event: &harness_core::event::EventEnvelopeV1,
    request_id: &str,
) -> bool {
    match &event.payload {
        harness_core::event::EventV1::ProviderReasoningDelta(data) => {
            provider_event_matches_activity(event, &data.request_id, request_id)
        }
        harness_core::event::EventV1::ProviderStreamDelta(data) => {
            provider_event_matches_activity(event, &data.request_id, request_id)
        }
        harness_core::event::EventV1::TaskCompleted(_)
        | harness_core::event::EventV1::ToolCallRequested(_) => {
            event.correlation_id.as_deref() == Some(request_id)
        }
        _ => false,
    }
}

pub(super) fn provider_event_matches_activity(
    event: &harness_core::event::EventEnvelopeV1,
    provider_request_id: &str,
    activity_request_id: &str,
) -> bool {
    provider_request_id == activity_request_id
        || event.correlation_id.as_deref() == Some(activity_request_id)
}
