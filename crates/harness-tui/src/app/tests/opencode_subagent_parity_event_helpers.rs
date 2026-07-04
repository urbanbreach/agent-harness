use super::*;

pub(super) fn child_task_requested_for_evidence(
    seq: u64,
    tool_call_id: &str,
    child_session_id: &str,
    child_request_id: &str,
    background: bool,
) -> EventEnvelopeV1 {
    let description = if child_session_id == "child_b" {
        "inspect sibling beta"
    } else {
        "audit transcript parity"
    };
    envelope(
        seq,
        "req_parent",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: tool_call_id.to_string(),
            tool_id: "task".to_string(),
            args_summary: serde_json::json!({
                "description": description,
                "subagent_type": "researcher",
                "background": background,
            })
            .to_string(),
            args_digest: format!("digest-{tool_call_id}"),
            metadata: Some(ToolCallMetadata {
                canonical_tool_id: Some("task".to_string()),
                lineage: Some(TaskLineageMetadata {
                    parent_tool_call_id: Some(tool_call_id.to_string()),
                    parent_request_id: Some("req_parent".to_string()),
                    child_session_id: Some(child_session_id.to_string()),
                    child_request_id: Some(child_request_id.to_string()),
                    ..TaskLineageMetadata::default()
                }),
                ..ToolCallMetadata::default()
            }),
        }),
    )
}

pub(super) fn lineage() -> TaskLineageMetadata {
    TaskLineageMetadata {
        parent_tool_call_id: Some("tc_task".to_string()),
        parent_request_id: Some("req_parent".to_string()),
        child_session_id: Some("agent_worker".to_string()),
        child_request_id: Some("req_child".to_string()),
        ..TaskLineageMetadata::default()
    }
}

pub(super) fn run_started(seq: u64) -> EventEnvelopeV1 {
    envelope(
        seq,
        "run",
        EventV1::RunStarted(RunStartedEvent {
            run_name: "opencode-subagent-parity".to_string(),
            workspace_root: "inline-harness-parity".to_string(),
        }),
    )
}

pub(super) fn agent_spawned_with_parent(
    seq: u64,
    agent_id: &str,
    profile: &str,
    parent_agent_id: Option<&str>,
) -> EventEnvelopeV1 {
    envelope(
        seq,
        "agent_spawned",
        EventV1::AgentSpawned(AgentSpawnedEvent {
            agent_id: agent_id.to_string(),
            profile: profile.to_string(),
            parent_agent_id: parent_agent_id.map(str::to_string),
        }),
    )
}
