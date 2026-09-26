use harness_core::event::BackgroundTaskNotificationEvent;

use harness_core::event::{EventEnvelopeV1, EventV1};

pub(super) fn background_notification_for_request<'a>(
    events: &'a [EventEnvelopeV1],
    request_id: &str,
) -> Option<&'a BackgroundTaskNotificationEvent> {
    events.iter().rev().find_map(|event| match &event.payload {
        EventV1::BackgroundTaskNotification(data)
            if data
                .delivered_turn_request_id
                .as_deref()
                .unwrap_or(&data.child_request_id)
                == request_id =>
        {
            Some(data)
        }
        _ => None,
    })
}
