#[test]
fn canonical_provider_view_preserves_unicode_attachments_usage_and_provenance() {
    // Given: one selected Unicode attachment, provider usage, hidden reasoning, and a complete tool pair.
    let attachment =
        AttachmentMetadata::from_bytes("資料-猫", "image/png", None, b"private image bytes", None);
    let usage = CompletionUsage {
        prompt_tokens: 31,
        completion_tokens: 7,
        total_tokens: 38,
    };
    let entries = vec![
        entry(
            "media-user",
            None,
            SessionEntryPayload::UserMessage {
                text: "inspect 猫".to_string(),
                attachments: vec![attachment.clone()],
            },
        ),
        entry(
            "media-assistant",
            Some("media-user"),
            SessionEntryPayload::AssistantMessage {
                parts: vec![
                    AssistantPart::Reasoning {
                        text: "hidden chain of thought".to_string(),
                    },
                    AssistantPart::ToolCall(AssistantToolCall {
                        tool_call_id: ToolCallId::new("call-media"),
                        provider_tool_call_id: None,
                        tool_id: "read".to_string(),
                        args_summary: "redacted".to_string(),
                        args_digest: "media-args".to_string(),
                        provider_call_id: None,
                    }),
                ],
                provenance: Some(Box::new(ProviderProvenance {
                    provider_id: "mock".to_string(),
                    model_id: "model-a".to_string(),
                    request_id: ProviderRequestId::new("provider-media"),
                    response_id: Some("response-redacted".to_string()),
                    stop_reason: Some("tool_use".to_string()),
                    usage: Some(usage.clone()),
                    runtime_selection: Some(Box::new(runtime_selection())),
                })),
            },
        ),
        entry(
            "media-result",
            Some("media-assistant"),
            SessionEntryPayload::ToolResult {
                tool_call_id: ToolCallId::new("call-media"),
                requesting_assistant_entry_id: EntryId::new("media-assistant"),
                status: ToolResultStatus::Succeeded,
                output_summary: Some("done".to_string()),
                output_digest: Some("media-output".to_string()),
                output_json: None,
            },
        ),
    ];
    let session = session(entries, "media-result");
    let view = session
        .provider_view(ProviderViewInput {
            owner: ProviderViewOwner::root("root-agent", SessionId::new("session-view")),
            selected_leaf: None,
            pending_prompt: None,
            runtime_selection: runtime_selection(),
        })
        .unwrap_or_abort();
    let profile = harness_core::agent::AgentProfile::fallback("default");

    // When: the canonical view is lowered to provider-visible messages.
    let messages = harness_core::agent::canonical_provider_messages(&view, &profile);
    let encoded = serde_json::to_string(&messages).unwrap_or_abort();

    // Then: metadata order/usage/provenance remain typed while bytes, usage text, and reasoning stay hidden.
    assert_eq!(view.attachments[0].attachment, attachment);
    assert_eq!(view.usage_boundaries[0].usage, usage);
    assert_eq!(view.tool_pairs.len(), 1);
    assert!(encoded.contains("資料-猫"));
    assert!(!encoded.contains("private image bytes"));
    assert!(!encoded.contains("hidden chain of thought"));
    assert!(!encoded.contains("prompt_tokens"));
}
