use harness_core::UnwrapOrAbort;
use std::fs;
use std::path::Path;

use harness_core::config::PermissionMode;
use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1};
use harness_core::perm::PermissionPolicy;

pub fn supervisor_actor() -> EventActor {
    supervisor_actor_with_id("agent_supervisor")
}

pub fn supervisor_actor_with_id(agent_id: &str) -> EventActor {
    EventActor::new(ActorKind::Supervisor, Some(agent_id.to_string()))
}

pub fn allow_all_permission_policy() -> PermissionPolicy {
    PermissionPolicy::allow_all()
}

pub fn shell_denied_permission_policy() -> PermissionPolicy {
    PermissionPolicy::new(
        PermissionMode::Allow,
        PermissionMode::Deny,
        PermissionMode::Allow,
    )
}

#[allow(clippy::panic, reason = "test helper panics on corrupted fixture data")]
pub fn load_events(events_path: &Path) -> Vec<EventEnvelopeV1> {
    let body = fs::read_to_string(events_path).unwrap_or_abort();
    let lines: Vec<&str> = body.lines().collect();
    let mut events = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        match serde_json::from_str::<EventEnvelopeV1>(line) {
            Ok(event) => events.push(event),
            Err(_) if i == lines.len() - 1 => continue,
            Err(e) => panic!("Failed to parse event at line {}: {}", i + 1, e),
        }
    }
    events
}

#[allow(clippy::panic, reason = "test code must panic gracefully")]
pub async fn wait_for_tool_call_finish(events_path: &Path, tool_call_id: &str) {
    for _ in 0..40 {
        if load_events(events_path).iter().any(|event| {
            matches!(
                &event.payload,
                EventV1::ToolCallFinished(payload) if payload.tool_call_id.as_str() == tool_call_id
            )
        }) {
            return;
        }

        tokio::task::yield_now().await;
    }

    panic!("abort");
}
