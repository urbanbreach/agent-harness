use super::*;
use crate::session::AssistantPart;

pub(crate) fn event(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("e-{seq}"),
        seq,
        run_id: "run".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::Worker, Some("agent".into())),
        correlation_id: Some("turn".into()),
        causation_id: None,
        stream_key: None,
        payload,
    }
}

#[test]
fn replay_commits_replace_deltas_and_keep_tool_results_attached(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut events = vec![
        event(
            1,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "turn".into(),
                text: "read".into(),
            }),
        ),
        event(
            2,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "p".into(),
                provider_id: "mock".into(),
                model_id: "model".into(),
                prompt_summary: "read".into(),
                request_digest: "r".into(),
                metadata: None,
            }),
        ),
        event(
            3,
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "p".into(),
                delta: "draft".into(),
            }),
        ),
        event(
            4,
            EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                request_id: "p".into(),
                tool_call_count: 1,
                parts: vec![
                    AssistantPart::Text {
                        text: "settled".into(),
                    },
                    AssistantPart::ToolCall(crate::session::AssistantToolCall {
                        tool_call_id: "tool".into(),
                        provider_tool_call_id: None,
                        tool_id: "read".into(),
                        args_summary: "x".into(),
                        args_digest: "d".into(),
                        provider_call_id: None,
                    }),
                ],
                provenance: None,
                assistant_message: None,
            }),
        ),
        event(
            5,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tool".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("contents".into()),
                output_digest: None,
                output_json: None,
                metadata: None,
            }),
        ),
    ];
    events.push(event(6, EventV1::CompactionWritten(serde_json::from_value(serde_json::json!({
        "checkpoint_id":"legacy", "agent_id":"agent", "artifact_path":"artifacts/summary.md",
        "artifact_bytes":7, "artifact_digest":"digest", "trigger_reason":"manual", "through_seq":1, "preserved_turns":1
    }))?)));
    let projection = project_transcript(&events)?;
    assert_eq!(projection.artifacts.len(), 1);
    assert_eq!(projection.artifacts[0].path, "artifacts/summary.md");
    assert!(projection.compaction_checkpoints.is_empty());
    let message = projection
        .messages
        .iter()
        .find(|m| m.role == ProjectedMessageRole::Assistant)
        .ok_or("assistant missing")?;
    assert_eq!(
        message.request_id.as_ref().map(|r| r.as_str()),
        Some("turn")
    );
    assert!(matches!(&message.parts[0], ProjectedPart::Text(text) if text.text == "settled"));
    assert!(
        matches!(&message.parts[1], ProjectedPart::ToolCall(tool) if tool.state == ProjectedToolCallState::Succeeded && tool.output_summary.as_deref() == Some("contents"))
    );
    let mut reversed = events;
    reversed.reverse();
    assert!(project_transcript(&reversed).is_err());
    Ok(())
}
