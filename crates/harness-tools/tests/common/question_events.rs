use std::path::Path;

use harness_core::event::{EventEnvelopeV1, EventV1};
use tokio::time::{sleep, Duration, Instant};

pub(crate) use event_reader::read_events;

#[path = "event_reader.rs"]
mod event_reader;

pub(crate) async fn wait_for_question_permission(
    path: &Path,
    previous: Option<&str>,
    timeout: Duration,
) -> String {
    let deadline = Instant::now() + timeout;
    loop {
        let events = read_events(path);
        let permission_id = if let Some(previous) = previous {
            events
                .into_iter()
                .rev()
                .find_map(|event| question_permission_id(event, Some(previous)))
        } else {
            events
                .into_iter()
                .find_map(|event| question_permission_id(event, None))
        };

        if let Some(permission_id) = permission_id {
            return permission_id;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for question permission"
        );
        sleep(Duration::from_millis(20)).await;
    }
}

fn question_permission_id(event: EventEnvelopeV1, previous: Option<&str>) -> Option<String> {
    match event.payload {
        EventV1::PermissionRequested(data)
            if data.kind == "question"
                && previous.is_none_or(|value| value != data.permission_id) =>
        {
            Some(data.permission_id)
        }
        _ => None,
    }
}
