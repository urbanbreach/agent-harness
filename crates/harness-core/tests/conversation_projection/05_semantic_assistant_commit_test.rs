use harness_core::UnwrapOrAbort;

fn semantic_conversation_events(include_legacy_delta: bool) -> Vec<EventEnvelopeV1> {
    let mut events = vec![
        envelope(
            1,
            EventActor::new(ActorKind::User, None),
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
    ];
    if include_legacy_delta {
        events.push(envelope(
            3,
            worker(),
            Some("turn-1"),
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "provider-1".into(),
                delta: "stale fragment".to_string(),
            }),
        ));
    }
    let finish_seq = if include_legacy_delta { 4 } else { 3 };
    events.push(envelope(
        finish_seq,
        worker(),
        Some("turn-1"),
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "provider-1".into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("output-digest".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
    events.push(envelope(
        finish_seq + 1,
        worker(),
        Some("turn-1"),
        EventV1::AssistantMessageFinished(harness_core::event::AssistantMessageFinishedEvent {
            request_id: "provider-1".into(),
            tool_call_count: 0,
            parts: vec![
                harness_core::session::AssistantPart::Reasoning {
                    text: "final reasoning".to_string(),
                },
                harness_core::session::AssistantPart::Text {
                    text: "final answer".to_string(),
                },
            ],
            provenance: None,
            assistant_message: None,
        }),
    ));
    events
}

fn projected_assistant_text(events: &[EventEnvelopeV1]) -> String {
    project_conversation(events, &[])
        .unwrap_or_abort()
        .messages
        .into_iter()
        .find_map(|message| match message {
            ConversationMessage::Assistant(assistant) => Some(assistant.text),
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn semantic_conversation_rebuilds_final_commit_without_deltas() {
    // arrange
    let events = semantic_conversation_events(false);

    // act
    let assistant_text = projected_assistant_text(&events);

    // assert
    assert_eq!(
        assistant_text, "final answer",
        "semantic assistant completion must rebuild text without deltas"
    );
}

#[test]
fn semantic_conversation_final_commit_replaces_transitional_deltas() {
    // arrange
    let events = semantic_conversation_events(true);

    // act
    let assistant_text = projected_assistant_text(&events);

    // assert
    assert_eq!(
        assistant_text, "final answer",
        "final semantic content must replace transitional fragments"
    );
}

#[test]
fn legacy_conversation_still_rebuilds_delta_only_history() {
    // arrange
    let mut events = semantic_conversation_events(true);
    let _ = events.pop();

    // act
    let assistant_text = projected_assistant_text(&events);

    // assert
    assert_eq!(assistant_text, "stale fragment");
}

#[test]
fn semantic_conversation_omits_interrupted_empty_assistant() {
    // arrange
    let mut events = semantic_conversation_events(false);
    events.truncate(2);

    // act
    let projection = project_conversation(&events, &[]).unwrap_or_abort();

    // assert
    assert!(projection
        .messages
        .iter()
        .all(|message| !matches!(message, ConversationMessage::Assistant(_))));
}
