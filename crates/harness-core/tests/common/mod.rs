use std::fs;
use std::path::Path;

use harness_core::config::PermissionMode;
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1};
use harness_core::perm::PermissionPolicy;

#[allow(dead_code)]
pub fn supervisor_actor() -> EventActor {
    supervisor_actor_with_id("agent_supervisor")
}

#[allow(dead_code)]
pub fn supervisor_actor_with_id(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Supervisor, Some(agent_id.to_string()))
}

#[allow(dead_code)]
pub fn allow_all_permission_policy() -> PermissionPolicy {
    PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Allow,
        PermissionMode::Allow,
    )
}

#[allow(dead_code)]
pub fn shell_denied_permission_policy() -> PermissionPolicy {
    PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Allow,
    )
}

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

        tokio::task::yield_now().await;
    }

    panic!("timed out waiting for tool call {tool_call_id} to finish");
}
