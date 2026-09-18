use harness_core::event::BackgroundTaskNotificationEvent;

use harness_core::event::{EventEnvelopeV1, EventV1};

pub(super) fn background_task_notification_text(data: &BackgroundTaskNotificationEvent) -> String {
    let status = data.status.as_str();
    let task_id = background_notification_safe_field(data.task_id.as_str());
    format!("Background task {} · {}", status.replace('_', " "), task_id)
}

fn background_notification_safe_field(value: &str) -> String {
    const MAX_CHARS: usize = 120;

    let mut sanitized = String::new();
    for character in value.chars() {
        sanitized.push(if character.is_control() || character == '\t' {
            ' '
        } else {
            character
        });
    }

    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        return "unknown".to_string();
    }

    let mut capped = String::new();
    for (index, character) in trimmed.chars().enumerate() {
        if index == MAX_CHARS {
            capped.push('…');
            break;
        }
        capped.push(character);
    }
    capped
}

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
