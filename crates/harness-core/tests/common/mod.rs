use std::fs;
use std::path::Path;
use std::time::Duration;

use harness_core::event::{EventEnvelopeV1, EventV1};

pub fn load_events(events_path: &Path) -> Vec<EventEnvelopeV1> {
    let body = fs::read_to_string(events_path).expect("read events file");
    body.lines()
        .map(|line| serde_json::from_str::<EventEnvelopeV1>(line).expect("parse event jsonl line"))
        .collect()
}

#[allow(dead_code)]
pub async fn wait_for_tool_call_finish(events_path: &Path, tool_call_id: &str) {
    for _ in 0..40 {
        if load_events(events_path).iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(payload) if payload.tool_call_id == tool_call_id
            )
        }) {
            return;
        }

        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    panic!("timed out waiting for tool call {tool_call_id} to finish");
}
