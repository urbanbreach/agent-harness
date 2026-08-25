use super::*;
use crate::conversation::ConversationMessage;
use crate::event::{
    ActorKind, AssistantMessageFinishedEvent, EventActor, ToolCallFinishedEvent,
    ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, UserMessageSubmittedEvent,
    SCHEMA_VERSION,
};
use crate::ids::{RequestId, ToolCallId};

fn event(
    seq: u64,
    actor: EventActor,
    correlation_id: &str,
    causation_id: Option<&str>,
    payload: EventV1,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-{seq}"),
        seq,
        run_id: "run_tool_pair_isolation".into(),
        mono_ms: seq,
        ts: None,
        actor,
        correlation_id: Some(correlation_id.to_string()),
        causation_id: causation_id.map(str::to_string),
        stream_key: None,
        payload,
    }
}

fn user(seq: u64, agent_id: &str, request_id: &str) -> EventEnvelopeV1 {
    event(
        seq,
        EventActor::new(ActorKind::Worker, Some(agent_id.to_string())),
        request_id,
        None,
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: RequestId::new(request_id),
            text: format!("prompt {request_id}"),
        }),
    )
}

fn requested(seq: u64, agent_id: &str, request_id: &str) -> EventEnvelopeV1 {
    event(
        seq,
        EventActor::new(ActorKind::Worker, Some(agent_id.to_string())),
        request_id,
        None,
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: ToolCallId::new("duplicate-call"),
            tool_id: "shell.run".to_string(),
            args_summary: "{}".to_string(),
            args_digest: format!("digest-{request_id}"),
            metadata: None,
        }),
    )
}

fn assistant(seq: u64, agent_id: &str, request_id: &str) -> EventEnvelopeV1 {
    event(
        seq,
        EventActor::new(ActorKind::Worker, Some(agent_id.to_string())),
        request_id,
        None,
        EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
            request_id: RequestId::new(&format!("provider-{request_id}")),
            tool_call_count: 1,
            parts: Vec::new(),
            provenance: None,
            assistant_message: None,
        }),
    )
}

fn lifecycle(seq: u64, request_id: &str, output: Option<&str>) -> EventEnvelopeV1 {
    let payload = match output {
        Some(output) => EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: ToolCallId::new("duplicate-call"),
            status: ToolCallStatus::Succeeded,
            output_summary: Some(output.to_string()),
            output_digest: None,
            output_json: None,
            metadata: None,
        }),
        None => EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: ToolCallId::new("duplicate-call"),
        }),
    };
    event(
        seq,
        EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        request_id,
        Some(&format!("evt-{}", seq.saturating_sub(1))),
        payload,
    )
}

fn tool_results(context: &ProviderContext) -> Vec<&str> {
    context
        .preserved_turns
        .iter()
        .flat_map(|turn| &turn.messages)
        .filter_map(|message| match message {
            ConversationMessage::ToolResult(result) => result.output_summary.as_deref(),
            _ => None,
        })
        .collect()
}

#[test]
fn compaction_v2_cross_agent_duplicate_tool_result_fails_closed() {
    let events = vec![
        user(1, "alpha", "turn-alpha"),
        assistant(2, "alpha", "turn-alpha"),
        requested(3, "alpha", "turn-alpha"),
        user(4, "beta", "turn-beta"),
        assistant(5, "beta", "turn-beta"),
        requested(6, "beta", "turn-beta"),
        lifecycle(7, "turn-beta", None),
        lifecycle(8, "turn-beta", Some("foreign beta result")),
    ];

    let context = reconstruct_provider_context_from_events(&events, "alpha").unwrap();

    assert!(
        tool_results(&context).is_empty(),
        "a same-ID completion owned by another agent must not enter alpha context"
    );
}

#[test]
fn compaction_v2_cross_turn_duplicate_tool_result_fails_closed() {
    let events = vec![
        user(1, "alpha", "turn-one"),
        assistant(2, "alpha", "turn-one"),
        requested(3, "alpha", "turn-one"),
        user(4, "alpha", "turn-two"),
        assistant(5, "alpha", "turn-two"),
        requested(6, "alpha", "turn-two"),
        lifecycle(7, "turn-two", None),
        lifecycle(8, "turn-two", Some("ambiguous turn-two result")),
    ];

    let context = reconstruct_provider_context_from_events(&events, "alpha").unwrap();

    assert!(
        tool_results(&context).is_empty(),
        "a duplicate ID spanning turns must fail closed instead of attaching by ID"
    );
}
