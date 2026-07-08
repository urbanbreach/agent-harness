use harness_core::event::BackgroundTaskNotificationEvent;

use super::ActivityEntry;

pub(super) fn background_task_notification_text(data: &BackgroundTaskNotificationEvent) -> String {
    let status = data.status.as_str();
    let task_id = background_notification_safe_field(data.task_id.as_str());
    let child_request_id = background_notification_safe_field(&data.child_request_id);
    let child_session_id = background_notification_safe_field(data.child_session_id.as_str());

    format!(
        "<system-reminder>\n[BACKGROUND TASK {}]\nID: {}\nRequest ID: {}\nStatus: {}\n\nBackground task {}. Use background_output(request_id=\"{}\") for full details or task(session_id=\"{}\") to continue analysis from the child session.\n</system-reminder>",
        status.to_ascii_uppercase(),
        task_id,
        child_request_id,
        status,
        status.replace('_', " "),
        child_request_id,
        child_session_id,
    )
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

pub(super) fn activity_is_background_notification_reminder(activity: &ActivityEntry) -> bool {
    activity
        .user_message
        .as_ref()
        .is_some_and(|message| message.text.contains("[BACKGROUND TASK "))
}
