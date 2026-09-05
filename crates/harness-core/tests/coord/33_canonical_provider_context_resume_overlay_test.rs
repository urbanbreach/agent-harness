use super::*;

#[test]
fn canonical_lowerer_inserts_transient_wire_tool_pair_before_pending_prompt() {
    // Given: canonical durable history and one transient failed tool turn using a provider wire id.
    let mut profile = agent_profiles().remove("default").unwrap_or_abort();
    profile.toolset = vec!["shell.run".to_string()];
    let tools = harness_core::agent::build_provider_tool_defs_for_model(
        &profile,
        test_tool_registry().as_ref(),
        "mock:model-1",
    )
    .unwrap_or_abort();
    let target = drift_target(
        "target-a-variant",
        "high",
        "low",
        "auto",
        serde_json::json!({"budget": 1024}),
    );
    let model = harness_core::agent::AgentModelRef {
        provider_id: target.provider.clone(),
        model_id: target.model.clone(),
    };
    let runtime_selection = harness_core::agent::canonical_runtime_selection(
        harness_core::agent::CanonicalRuntimeSelectionInput {
            profile: &profile,
            model: &model,
            settings: harness_core::agent::AgentModelSettings::from(&target),
            resolved_limits: target.limits,
            tools: &tools,
        },
    )
    .unwrap_or_abort();
    let entry_id = harness_core::ids::EntryId::new("overlay-leaf");
    let view = harness_core::session::CanonicalProviderView {
        owner: harness_core::session::ProviderViewOwner::root(
            "agent_000001",
            harness_core::ids::SessionId::new("run_000001"),
        ),
        selected_leaf: entry_id.clone(),
        active_entry_ids: vec![entry_id.clone()],
        entries: vec![harness_core::session::SessionEntry {
            id: entry_id,
            parent_id: None,
            turn_id: Some(harness_core::ids::TurnId::new("turn-history")),
            run_id: harness_core::ids::RunId::new("run_000001"),
            payload: harness_core::session::SessionEntryPayload::UserMessage {
                text: "durable history".to_string(),
                attachments: Vec::new(),
            },
        }],
        pending_prompt: Some(harness_core::session::CanonicalPendingPrompt {
            turn_id: harness_core::ids::TurnId::new("turn-pending"),
            text: "continue now".to_string(),
            attachments: Vec::new(),
        }),
        latest_compaction_summary: None,
        tool_pairs: Vec::new(),
        attachments: Vec::new(),
        usage_boundaries: Vec::new(),
        watermark: Some(harness_core::session::RecordSequence::new(3)),
        runtime_selection,
    };
    let wire_id = harness_core::ids::ToolCallId::new("call_denied");
    let overlay = harness_core::agent::ProviderConversationTurn {
        user_prompt: "failed operation".to_string(),
        status: harness_core::agent::ProviderConversationTurnStatus::Failed,
        failure_stage: Some("tool_failure".to_string()),
        messages: vec![
            harness_core::conversation::ConversationMessage::User(
                harness_core::conversation::ConversationUserMessage {
                    request_id: harness_core::ids::RequestId::new("turn-failed"),
                    text: "failed operation".to_string(),
                    seq: None,
                    agent_id: Some("agent_000001".to_string()),
                },
            ),
            harness_core::conversation::ConversationMessage::Assistant(
                harness_core::conversation::ConversationAssistantMessage {
                    request_id: harness_core::ids::RequestId::new("turn-failed"),
                    agent_id: Some("agent_000001".to_string()),
                    text: String::new(),
                    tool_calls: vec![harness_core::conversation::ConversationToolCall {
                        tool_call_id: wire_id.clone(),
                        tool_id: "shell.run".to_string(),
                        args_summary: r#"{"cmd":"blocked"}"#.to_string(),
                        args_digest: "digest".to_string(),
                        seq: None,
                        metadata: None,
                    }],
                    stop_reason: None,
                    first_seq: None,
                    last_seq: None,
                    provider_id: Some("mock".to_string()),
                    model_id: Some("model-1".to_string()),
                    output_digest: None,
                },
            ),
            harness_core::conversation::ConversationMessage::ToolResult(Box::new(
                harness_core::conversation::ConversationToolResultMessage {
                    request_id: harness_core::ids::RequestId::new("turn-failed"),
                    tool_call_id: wire_id,
                    tool_id: Some("shell.run".to_string()),
                    status: harness_core::event::ToolCallStatus::Failed,
                    output_summary: Some("permission denied".to_string()),
                    output_digest: Some("result-digest".to_string()),
                    output_json: None,
                    seq: None,
                    metadata: None,
                },
            )),
        ],
        ..harness_core::agent::ProviderConversationTurn::default()
    };

    // When: the provider boundary lowers canonical history with the transient operational overlay.
    let lowered = harness_core::agent::lower_provider_continuation(
        harness_core::agent::LowerProviderContinuationInput {
            view: &view,
            transient_operational_turns: &[overlay],
            profile: &profile,
            tools: Some(tools),
            tool_choice: Some(harness_providers::ToolChoice::Auto),
            fresh_request_id: "request-fresh",
        },
    )
    .unwrap_or_abort();

    // Then: the wire id is paired exactly once and the pending prompt remains last.
    let calls = lowered
        .messages
        .iter()
        .flat_map(|message| message.assistant_tool_calls.iter().flatten())
        .collect::<Vec<_>>();
    let results = lowered
        .messages
        .iter()
        .filter(|message| message.tool_call_id.as_deref() == Some("call_denied"))
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].tool_call_id, "call_denied");
    assert_eq!(results.len(), 1);
    assert_eq!(lowered.messages.last().unwrap_or_abort().content, "continue now");
    assert_eq!(lowered.request.messages, lowered.messages);
}
