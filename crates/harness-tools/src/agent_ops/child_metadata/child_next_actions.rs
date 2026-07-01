use serde_json::{json, Value};

use super::super::AgentSpawnRequest;

pub(super) fn child_next_actions(
    request: &AgentSpawnRequest,
    agent_id: &str,
    request_id: &str,
) -> Value {
    let mut actions = Vec::new();
    actions.push(json!({
        "action": "inspect_child_session",
        "tool": "session_info",
        "parameters": { "session": agent_id },
    }));
    actions.push(json!({
        "action": "read_child_session",
        "tool": "session_read",
        "parameters": { "session": agent_id },
    }));
    if request.run_in_background {
        actions.push(json!({
            "action": "check_status",
            "tool": "background_output",
            "parameters": { "request_id": request_id, "block": false },
        }));
        actions.push(json!({
            "action": "wait_for_result",
            "tool": "background_output",
            "parameters": { "request_id": request_id, "block": true },
        }));
        actions.push(json!({
            "action": "cancel",
            "tool": "background_cancel",
            "parameters": {
                "request_id": request_id,
                "reason": "cancelled by parent request"
            },
        }));
        actions.push(json!({
            "action": "cancel_compat",
            "tool": "background_output",
            "parameters": {
                "request_id": request_id,
                "cancel": true,
                "reason": "cancelled by parent request"
            },
        }));
    }
    actions.push(json!({
        "action": "continue_task",
        "tool": "task",
        "parameters": {
            "session_id": agent_id,
            "subagent_type": request.profile_name,
            "description": format!("Continue {}", request.description),
            "prompt": "Continue this child task with additional instructions.",
            "run_in_background": false,
            "load_skills": []
        },
    }));
    Value::Array(actions)
}
