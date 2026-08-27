#[test]
fn compaction_v2_tool_pair_stays_atomic() {
    // arrange
    // act
    // assert
    let tool_call_id = ToolCallId::new("tool-1");
    let events = vec![
        envelope(
            1,
            worker(),
            Some("turn-tool"),
            EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                request_id: "provider-tool".into(),
                tool_call_count: 1,
                parts: vec![AssistantPart::ToolCall(CanonicalAssistantToolCall {
                    tool_call_id: tool_call_id.clone(),
                    provider_tool_call_id: None,
                    tool_id: "read".to_string(),
                    args_summary: "{}".to_string(),
                    args_digest: "args-digest".to_string(),
                    provider_call_id: None,
                })],
                provenance: None,
                assistant_message: None,
            }),
        ),
        envelope(
            2,
            worker(),
            Some("turn-tool"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: tool_call_id.clone(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("ok".to_string()),
                output_digest: Some("result-digest".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
    ];

    let projection = project_conversation(&events, &[]).unwrap_or_abort();
    let pair = match projection.messages.as_slice() {
        [ConversationMessage::Assistant(assistant), ConversationMessage::ToolResult(result)] => {
            Some((
                assistant
                    .tool_calls
                    .first()
                    .map(|call| call.tool_call_id.clone()),
                result.tool_call_id.clone(),
            ))
        }
        _ => None,
    };

    assert_eq!(pair, Some((Some(tool_call_id.clone()), tool_call_id)));
}
