use harness_core::UnwrapOrAbort;
#[test]
fn provider_boundary_sanitizes_unknown_historical_tool_ids() {
    // arrange
    // act
    // assert
    let mut profile = boundary_profile();
    profile.toolset.clear();
    let prior_context = ProviderContext {
        compacted_summary: None,
        preserved_turns: vec![ProviderConversationTurn {
            user_prompt: "use unknown tools".to_string(),
            assistant_response: "done".to_string(),
            request_id: Some("req_unknown_tools".into()),
            messages: vec![
                ConversationMessage::User(ConversationUserMessage {
                    request_id: "req_unknown_tools".into(),
                    text: "use unknown tools".to_string(),
                    seq: None,
                    agent_id: Some("agent_1".to_string()),
                }),
                ConversationMessage::Assistant(ConversationAssistantMessage {
                    request_id: "req_unknown_tools".into(),
                    agent_id: Some("agent_1".to_string()),
                    text: String::new(),
                    tool_calls: vec![
                        ConversationToolCall {
                            tool_call_id: "toolcall_dot".into(),
                            tool_id: "unknown.tool".to_string(),
                            args_summary: r#"{"value":1}"#.to_string(),
                            args_digest: "digest-dot".to_string(),
                            seq: None,
                            metadata: None,
                        },
                        ConversationToolCall {
                            tool_call_id: "toolcall_slash".into(),
                            tool_id: "unknown/tool".to_string(),
                            args_summary: r#"{"value":2}"#.to_string(),
                            args_digest: "digest-slash".to_string(),
                            seq: None,
                            metadata: None,
                        },
                    ],
                    stop_reason: None,
                    first_seq: None,
                    last_seq: None,
                    provider_id: None,
                    model_id: None,
                    output_digest: None,
                }),
                ConversationMessage::ToolResult(Box::new(ConversationToolResultMessage {
                    request_id: "req_unknown_tools".into(),
                    tool_call_id: "toolcall_dot".into(),
                    tool_id: Some("unknown.tool".to_string()),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("dot ok".to_string()),
                    output_digest: None,
                    output_json: None,
                    seq: None,
                    metadata: None,
                })),
                ConversationMessage::ToolResult(Box::new(ConversationToolResultMessage {
                    request_id: "req_unknown_tools".into(),
                    tool_call_id: "toolcall_slash".into(),
                    tool_id: Some("unknown/tool".to_string()),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("slash ok".to_string()),
                    output_digest: None,
                    output_json: None,
                    seq: None,
                    metadata: None,
                })),
                ConversationMessage::Assistant(ConversationAssistantMessage {
                    request_id: "req_unknown_tools".into(),
                    agent_id: Some("agent_1".to_string()),
                    text: "done".to_string(),
                    tool_calls: Vec::new(),
                    stop_reason: None,
                    first_seq: None,
                    last_seq: None,
                    provider_id: None,
                    model_id: None,
                    output_digest: None,
                }),
            ],
            ..ProviderConversationTurn::default()
        }],
        checkpoint: None,
    };

    let messages = build_provider_context_messages(&profile, &prior_context, "next");
    let assistant_tool_calls = messages
        .iter()
        .find_map(|message| message.assistant_tool_calls.as_ref())
        .unwrap_or_abort();
    assert_eq!(assistant_tool_calls[0].function_name, "unknown_tool");
    assert_eq!(assistant_tool_calls[1].function_name, "unknown_tool");
    assert_eq!(
        messages
            .iter()
            .filter(|message| {
                message.role == MessageRole::Tool && message.name.as_deref() == Some("unknown_tool")
            })
            .count(),
        2
    );
}
