use std::path::Path;

use harness_core::event::{EventEnvelopeV1, EventV1, ToolCallFinishedEvent, ToolCallStatus};
use tokio::time::{sleep, Duration, Instant};

pub(crate) use event_reader::read_events;

#[path = "event_reader.rs"]
mod event_reader;

pub(crate) async fn wait_for_tool_call_finish(path: &Path, tool_call_id: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if read_events(path).iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(payload) if payload.tool_call_id == tool_call_id
            )
        }) {
            return;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for tool call {tool_call_id}"
        );
        sleep(Duration::from_millis(20)).await;
    }
}

pub(crate) async fn wait_for_succeeded_tool_call_finish(
    events_path: &Path,
    tool_call_id: &str,
    timeout: Duration,
) {
    let deadline = Instant::now() + timeout;

    loop {
        if events_path.exists()
            && read_events(events_path).iter().any(|event| {
                matches!(
                    &event.payload,
                    EventV1::ToolCallFinished(data)
                        if data.tool_call_id == tool_call_id
                            && data.status == ToolCallStatus::Succeeded
                )
            })
        {
            return;
        }

        if Instant::now() >= deadline {
            panic!(
                "timed out waiting for ToolCallFinished for {tool_call_id} in {}",
                events_path.display()
            );
        }

        sleep(Duration::from_millis(20)).await;
    }
}

pub(crate) async fn wait_for_request_terminal(path: &Path, request_id: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if read_events(path).iter().any(|event| {
            event.correlation_id.as_deref() == Some(request_id)
                && matches!(
                    &event.payload,
                    EventV1::TaskCompleted(_) | EventV1::TaskCancelled(_)
                )
        }) {
            return;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for request {request_id} terminal event"
        );
        sleep(Duration::from_millis(20)).await;
    }
}

pub(crate) fn find_finished(
    events: &[EventEnvelopeV1],
    tool_call_id: &str,
) -> ToolCallFinishedEvent {
    events
        .iter()
        .find_map(|event| match &event.payload {
            EventV1::ToolCallFinished(payload) if payload.tool_call_id == tool_call_id => {
                Some(payload.clone())
            }
            _ => None,
        })
        .expect("tool call finished event")
}
