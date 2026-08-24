fn semantic_resume_envelope(
    seq: u64,
    actor: EventActor,
    correlation_id: Option<&str>,
    payload: EventV1,
) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-semantic-{seq:04}"),
        seq,
        run_id: "run_resume_fixture".into(),
        mono_ms: seq,
        ts: None,
        actor,
        correlation_id: correlation_id.map(str::to_string),
        causation_id: None,
        stream_key: None,
        payload,
    }
}

fn semantic_resume_finish(seq: u64) -> EventEnvelopeV1 {
    semantic_resume_envelope(
        seq,
        EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
        Some("turn-1"),
        EventV1::AssistantMessageFinished(harness_core::event::AssistantMessageFinishedEvent {
            request_id: "provider-1".into(),
            tool_call_count: 0,
            parts: vec![
                harness_core::session::AssistantPart::Reasoning {
                    text: "restart reasoning".to_string(),
                },
                harness_core::session::AssistantPart::Text {
                    text: "restart answer".to_string(),
                },
            ],
            provenance: None,
            assistant_message: None,
        }),
    )
}

fn semantic_resume_events() -> Vec<EventEnvelopeV1> {
    let worker = EventActor::new(ActorKind::Worker, Some("agent_000001".to_string()));
    vec![
        semantic_resume_envelope(
            1,
            EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            None,
            EventV1::RunStarted(RunStartedEvent {
                run_name: "semantic-resume".into(),
                workspace_root: "/workspace/project".to_string(),
            }),
        ),
        semantic_resume_envelope(
            2,
            EventActor::new(ActorKind::User, None),
            Some("turn-1"),
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "turn-1".into(),
                text: "question".to_string(),
            }),
        ),
        semantic_resume_envelope(
            3,
            worker.clone(),
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
        semantic_resume_envelope(
            4,
            worker,
            Some("turn-1"),
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "provider-1".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("output-digest".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
        semantic_resume_finish(5),
        semantic_resume_envelope(
            6,
            EventActor::new(ActorKind::System, Some("coordinator".to_string())),
            None,
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        ),
    ]
}

fn restarted_assistant_parts() -> Vec<harness_core::session::AssistantPart> {
    let snapshot = harness_core::session::legacy::LegacyEventLogAdapter::new()
        .project(&semantic_resume_events())
        .unwrap_or_abort();
    let path = snapshot.session.active_path().unwrap_or_abort();
    path.iter()
        .find_map(|entry| match &entry.payload {
            harness_core::session::SessionEntryPayload::AssistantMessage { parts, .. } => {
                Some(parts.clone())
            }
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn semantic_restart_restores_context_without_side_effects() {
    // arrange
    let before = semantic_resume_events();

    // act
    let first = restarted_assistant_parts();
    let second = restarted_assistant_parts();

    // assert
    assert_eq!(first, second, "replay must be deterministic");
    assert_eq!(
        first,
        vec![
            harness_core::session::AssistantPart::Reasoning {
                text: "restart reasoning".to_string(),
            },
            harness_core::session::AssistantPart::Text {
                text: "restart answer".to_string(),
            },
        ]
    );
    assert_eq!(
        semantic_resume_events(),
        before,
        "replay must not mutate input"
    );
}

#[test]
fn semantic_restart_context_matches_uninterrupted_context() {
    // arrange
    let uninterrupted = vec![
        harness_core::session::AssistantPart::Reasoning {
            text: "restart reasoning".to_string(),
        },
        harness_core::session::AssistantPart::Text {
            text: "restart answer".to_string(),
        },
    ];

    // act
    let restarted = restarted_assistant_parts();

    // assert
    assert_eq!(restarted, uninterrupted);
}

#[test]
fn semantic_restart_preserves_tool_pairing_and_request_digest() {
    // arrange
    let mut events = semantic_resume_events();
    if let EventV1::AssistantMessageFinished(finished) = &mut events[4].payload {
        finished.tool_call_count = 1;
        finished
            .parts
            .push(harness_core::session::AssistantPart::ToolCall(
                harness_core::session::AssistantToolCall {
                    tool_call_id: "canonical-tool-1".into(),
                    provider_tool_call_id: Some("provider-tool-1".to_string()),
                    tool_id: "read".to_string(),
                    args_summary: r#"{"path":"Cargo.toml"}"#.to_string(),
                    args_digest: "args-digest".to_string(),
                    provider_call_id: Some("provider-call-1".to_string()),
                },
            ));
    }
    events.insert(
        5,
        semantic_resume_envelope(
            6,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("turn-1"),
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "canonical-tool-1".into(),
                tool_id: "read".to_string(),
                args_summary: r#"{"path":"Cargo.toml"}"#.to_string(),
                args_digest: "args-digest".to_string(),
                metadata: None,
            }),
        ),
    );
    events.insert(
        6,
        semantic_resume_envelope(
            7,
            EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
            Some("turn-1"),
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "canonical-tool-1".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("Cargo.toml".to_string()),
                output_digest: Some("output-digest".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
    );
    for (index, event) in events.iter_mut().enumerate() {
        event.seq = u64::try_from(index + 1).unwrap_or_abort();
        event.event_id = format!("evt-semantic-{:04}", event.seq);
        event.mono_ms = event.seq;
    }
    let started_digest = events.iter().find_map(|event| match &event.payload {
        EventV1::ProviderRequestStarted(started) => Some(started.request_digest.clone()),
        _ => None,
    });

    // act
    let snapshot = harness_core::session::legacy::LegacyEventLogAdapter::new()
        .project(&events)
        .unwrap_or_abort();
    let path = snapshot.session.active_path().unwrap_or_abort();
    let assistant = path
        .iter()
        .find(|entry| {
            matches!(
                entry.payload,
                harness_core::session::SessionEntryPayload::AssistantMessage { .. }
            )
        })
        .unwrap_or_abort();
    let assistant_tool_call_id = match &assistant.payload {
        harness_core::session::SessionEntryPayload::AssistantMessage { parts, .. } => parts
            .iter()
            .find_map(|part| match part {
                harness_core::session::AssistantPart::ToolCall(tool_call) => {
                    Some(tool_call.tool_call_id.as_str())
                }
                _ => None,
            })
            .unwrap_or_default(),
        _ => "",
    };
    let paired = path.iter().any(|entry| {
        matches!(
            &entry.payload,
            harness_core::session::SessionEntryPayload::ToolResult {
                tool_call_id,
                requesting_assistant_entry_id,
                ..
            } if tool_call_id.as_str() == assistant_tool_call_id
                && requesting_assistant_entry_id == &assistant.id
        )
    });

    // assert
    assert_eq!(
        (started_digest.as_deref(), assistant_tool_call_id, paired),
        (Some("request-digest"), "canonical-tool-1", true)
    );
}

#[test]
fn semantic_interrupted_history_has_no_canonical_assistant_entry() {
    // arrange
    let mut events = semantic_resume_events();
    events.truncate(3);
    events.push(semantic_resume_envelope(
        4,
        EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        None,
        EventV1::RunFailed(harness_core::event::RunFailedEvent {
            error: "provider interrupted".to_string(),
        }),
    ));

    // act
    let snapshot = harness_core::session::legacy::LegacyEventLogAdapter::new()
        .project(&events)
        .unwrap_or_abort();
    let assistant_count = snapshot
        .session
        .active_path()
        .unwrap_or_abort()
        .iter()
        .filter(|entry| {
            matches!(
                entry.payload,
                harness_core::session::SessionEntryPayload::AssistantMessage { .. }
            )
        })
        .count();

    // assert
    assert_eq!(assistant_count, 0);
}

#[test]
fn legacy_interrupted_history_preserves_partial_assistant_and_warning() {
    // arrange
    let mut events = semantic_resume_events();
    events.truncate(3);
    events.push(semantic_resume_envelope(
        4,
        EventActor::new(ActorKind::Worker, Some("agent_000001".to_string())),
        Some("turn-1"),
        EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
            request_id: "provider-1".into(),
            delta: "historical partial".to_string(),
        }),
    ));
    events.push(semantic_resume_envelope(
        5,
        EventActor::new(ActorKind::System, Some("coordinator".to_string())),
        None,
        EventV1::RunFailed(harness_core::event::RunFailedEvent {
            error: "provider interrupted".to_string(),
        }),
    ));

    // act
    let snapshot = harness_core::session::legacy::LegacyEventLogAdapter::new()
        .project(&events)
        .unwrap_or_abort();
    let partial = snapshot
        .session
        .active_path()
        .unwrap_or_abort()
        .iter()
        .find_map(|entry| match &entry.payload {
            harness_core::session::SessionEntryPayload::AssistantMessage { parts, .. } => {
                parts.first()
            }
            _ => None,
        });

    // assert
    assert!(matches!(
        partial,
        Some(harness_core::session::AssistantPart::Text { text }) if text == "historical partial"
    ));
    assert!(snapshot.warnings.contains(
        &harness_core::session::legacy::LegacyWarning::MissingFinalAssistantContent {
            request_id: "provider-1".to_string(),
        }
    ));
}
