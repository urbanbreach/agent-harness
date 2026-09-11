fn semantic_transcript_finish(seq: u64) -> EventEnvelopeV1 {
    envelope(
        seq,
        worker(),
        Some("turn-1"),
        EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
            request_id: "provider-1".into(),
            tool_call_count: 1,
            parts: vec![
                harness_core::session::AssistantPart::Reasoning {
                    text: "final reasoning".to_string(),
                },
                harness_core::session::AssistantPart::Text {
                    text: "final answer".to_string(),
                },
                harness_core::session::AssistantPart::ToolCall(
                    harness_core::session::AssistantToolCall {
                        tool_call_id: "toolcall_000001".into(),
                        provider_tool_call_id: Some("provider-tool-1".to_string()),
                        tool_id: "read".to_string(),
                        args_summary: r#"{"path":"Cargo.toml"}"#.to_string(),
                        args_digest: "digest-tool-1".to_string(),
                        provider_call_id: Some("provider-call-1".to_string()),
                    },
                ),
            ],
            provenance: None,
            assistant_message: None,
        }),
    )
}

fn semantic_transcript_events() -> Vec<EventEnvelopeV1> {
    vec![
        envelope(
            1,
            user(),
            Some("turn-1"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "turn-1".into(),
                text: "question".to_string(),
            }),
        ),
        envelope(
            2,
            worker(),
            Some("turn-1"),
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "provider-1".into(),
                provider_id: "mock".to_string(),
                model_id: "model".to_string(),
                prompt_summary: "question".to_string(),
                request_digest: "request-digest".to_string(),
                metadata: None,
            }),
        ),
        envelope(
            3,
            worker(),
            Some("turn-1"),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "provider-1".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("output-digest".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        semantic_transcript_finish(4),
        tool_requested(
            5,
            "turn-1",
            "toolcall_000001",
            "read",
            r#"{"path":"Cargo.toml"}"#,
            None,
        ),
    ]
}

#[test]
fn semantic_commit_preserves_tools_waiting_for_permission() {
    // The interactive scenario queues an edit before the provider responds.
    // Its response may also mention the tool; neither case can discard the
    // coordinator's request or approval state.
    for response_mentions_tool in [false, true] {
        let mut events = semantic_transcript_events();
        let requested = events.pop().unwrap_or_abort();
        events.insert(1, requested);
        events.insert(
            2,
            envelope(
                0,
                system(),
                Some("turn-1"),
                EventV1::PermissionRequested(PermissionRequestedEvent {
                    permission_id: "permission-1".to_string(),
                    kind: "tool".to_string(),
                    tool_call_id: Some("toolcall_000001".into()),
                    summary: "Allow the queued tool?".to_string(),
                    request_digest: "permission-digest".to_string(),
                    timeout_ms: 30_000,
                    default_decision: PermissionDecision::Deny,
                }),
            ),
        );
        if let EventV1::AssistantMessageFinished(commit) =
            &mut events.last_mut().unwrap_or_abort().payload
        {
            commit.parts.retain(|part| {
                response_mentions_tool
                    || !matches!(part, harness_core::session::AssistantPart::ToolCall(_))
            });
            commit.tool_call_count = usize::from(response_mentions_tool);
        }
        events.extend([
            envelope(
                0,
                system(),
                Some("turn-1"),
                EventV1::PermissionResolved(PermissionResolvedEvent {
                    permission_id: "permission-1".to_string(),
                    decision: PermissionDecision::Allow,
                    reason: None,
                }),
            ),
            envelope(
                0,
                system(),
                Some("turn-1"),
                EventV1::ToolCallStarted(ToolCallStartedEvent {
                    tool_call_id: "toolcall_000001".into(),
                }),
            ),
            envelope(
                0,
                system(),
                Some("turn-1"),
                EventV1::ToolCallFinished(ToolCallFinishedEvent {
                    tool_call_id: "toolcall_000001".into(),
                    status: ToolCallStatus::Succeeded,
                    output_summary: Some("file contents".to_string()),
                    output_digest: Some("result-digest".to_string()),
                    output_json: None,
                    metadata: None,
                }),
            ),
        ]);
        for (index, event) in events.iter_mut().enumerate() {
            event.seq = u64::try_from(index + 1).unwrap_or_abort();
        }

        let projection = project_transcript(&events).unwrap_or_abort();
        let tools = assistant_message(&projection, "turn-1")
            .parts
            .iter()
            .filter_map(|part| match part {
                ProjectedPart::ToolCall(tool) => Some(tool),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(tools.len(), 1);
        let tool = tools[0];
        assert_eq!(tool.tool_id, "read");
        assert_eq!(tool.args_summary, r#"{"path":"Cargo.toml"}"#);
        assert_eq!(tool.requested_seq, Some(2));
        assert_eq!(tool.state, ProjectedToolCallState::Succeeded);
        assert_eq!(tool.output_summary.as_deref(), Some("file contents"));
        assert_eq!(tool.permissions.len(), 1);
        assert_eq!(tool.permissions[0].decision, Some(PermissionDecision::Allow));
    }
}

#[test]
fn semantic_transcript_preserves_reasoning_text_and_tool_order() {
    // arrange
    let events = semantic_transcript_events();

    // act
    let projection = project_transcript(&events).unwrap_or_abort();
    let parts = &assistant_message(&projection, "turn-1").parts;
    let semantic_parts = parts
        .iter()
        .filter_map(|part| match part {
            ProjectedPart::Reasoning(part) => Some(("reasoning", part.text.as_str())),
            ProjectedPart::Text(part) => Some(("text", part.text.as_str())),
            ProjectedPart::ToolCall(part) => Some(("tool", part.tool_id.as_str())),
            _ => None,
        })
        .collect::<Vec<_>>();

    // assert
    assert_eq!(
        semantic_parts,
        vec![
            ("reasoning", "final reasoning"),
            ("text", "final answer"),
            ("tool", "read")
        ]
    );
}

#[test]
fn semantic_event_inventory_covers_required_durable_facts() {
    // arrange
    let events = semantic_transcript_events();

    // act
    let has_provider_delta = events.iter().any(|event| {
        matches!(
            event.payload,
            EventV1::ProviderStreamDelta(_) | EventV1::ProviderReasoningDelta(_)
        )
    });
    let serialized_finish = events
        .iter()
        .find(|event| matches!(event.payload, EventV1::AssistantMessageFinished(_)))
        .map(|event| serde_json::to_value(event).unwrap_or_abort())
        .unwrap_or(serde_json::Value::Null);
    let committed_part_count = serialized_finish
        .pointer("/payload/data/parts")
        .and_then(serde_json::Value::as_array)
        .map_or(0, Vec::len);

    // assert
    assert!(!has_provider_delta);
    assert_eq!(
        committed_part_count, 3,
        "assistant finish must retain the semantic part inventory"
    );
}

#[test]
fn semantic_transcript_does_not_duplicate_live_fragments() {
    // arrange
    let mut events = semantic_transcript_events();
    events.insert(
        2,
        envelope(
            3,
            worker(),
            Some("turn-1"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "provider-1".into(),
                delta: "stale fragment".to_string(),
            }),
        ),
    );
    for (index, event) in events.iter_mut().enumerate() {
        event.seq = u64::try_from(index + 1).unwrap_or_abort();
    }

    // act
    let projection = project_transcript(&events).unwrap_or_abort();
    let assistant = assistant_message(&projection, "turn-1");
    let text = assistant
        .parts
        .iter()
        .filter_map(|part| match part {
            ProjectedPart::Text(part) => Some(part.text.as_str()),
            _ => None,
        })
        .collect::<String>();
    let tool_count = assistant
        .parts
        .iter()
        .filter(|part| matches!(part, ProjectedPart::ToolCall(_)))
        .count();

    // assert
    assert_eq!((text.as_str(), tool_count), ("final answer", 1));
}

#[test]
fn semantic_transcript_omits_interrupted_empty_assistant() {
    // arrange
    let mut events = semantic_transcript_events();
    events.truncate(2);

    // act
    let projection = project_transcript(&events).unwrap_or_abort();

    // assert
    assert!(projection
        .messages
        .iter()
        .all(|message| message.role != ProjectedMessageRole::Assistant));
}
