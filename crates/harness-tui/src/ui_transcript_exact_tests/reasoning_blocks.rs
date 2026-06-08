use super::super::*;

fn event(
    seq: u64,
    mono_ms: u64,
    correlation_id: &str,
    payload: harness_core::event::EventV1,
) -> harness_core::event::EventEnvelopeV1 {
    harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: format!("evt_reasoning_block_{seq:04}"),
        seq,
        run_id: "run_reasoning_block".to_string(),
        mono_ms,
        ts: Some("2026-03-22T15:30:00Z".to_string()),
        actor: harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_parent".to_string()),
        ),
        correlation_id: Some(correlation_id.to_string()),
        causation_id: None,
        stream_key: None,
        payload,
    }
}

#[test]
fn reasoning_block_uses_event_local_duration_before_tool_and_body() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(event(
        1,
        0,
        "req_reasoning_block",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_reasoning_block".to_string(),
                text: "Check docs".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        2,
        10,
        "req_reasoning_block",
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_reasoning_block".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Check docs".to_string(),
                request_digest: "digest-reasoning-block".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        3,
        100,
        "req_reasoning_block",
        harness_core::event::EventV1::ProviderReasoningDelta(
            harness_core::event::ProviderReasoningDeltaEvent {
                request_id: "req_reasoning_block".to_string(),
                delta: "Check tool timing.".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        4,
        350,
        "req_reasoning_block",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_reasoning_read".to_string(),
                tool_id: "read".to_string(),
                args_summary: r#"{"filePath":"README.md"}"#.to_string(),
                args_digest: "digest-reasoning-read-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        5,
        500,
        "req_reasoning_block",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_reasoning_read".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("read README".to_string()),
                output_digest: Some("digest-reasoning-read-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        6,
        9_000,
        "req_reasoning_block",
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_reasoning_block".to_string(),
                delta: "The README documents the quick start.".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        7,
        10_000,
        "req_reasoning_block",
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_reasoning_block".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-reasoning-block-output".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));

    let sections = build_transcript_sections(&app);
    assert_eq!(sections.len(), 1);
    let turn = &sections[0];
    assert_eq!(turn.header.duration_ms, Some(9_990));
    assert!(matches!(
        turn.assistant_parts.as_slice(),
        [
            TranscriptAssistantPart::Reasoning(_),
            TranscriptAssistantPart::ToolCall(_),
            TranscriptAssistantPart::Body(_),
        ]
    ));
    let TranscriptAssistantPart::Reasoning(reasoning) = &turn.assistant_parts[0] else {
        panic!("first assistant part should be reasoning");
    };
    assert_eq!(reasoning.status, ActivityStatus::Done);
    assert_eq!(reasoning.started_mono_ms, Some(100));
    assert_eq!(reasoning.duration_ms, Some(250));

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        96,
    ));

    let reasoning_row = lines
        .iter()
        .position(|line| line.contains("Check tool timing."))
        .expect("reasoning row");
    let tool_row = lines
        .iter()
        .position(|line| line.contains("Read README.md"))
        .expect("tool row");
    let body_row = lines
        .iter()
        .position(|line| line.contains("The README documents the quick start."))
        .expect("body row");

    assert!(reasoning_row < tool_row);
    assert!(tool_row < body_row);
    assert!(lines[reasoning_row].contains("Thought: Check tool timing. · 250ms"));
    assert!(!lines[reasoning_row].contains("Thinking:"));
    assert!(!lines[reasoning_row].contains("10.0s"));
}

#[test]
fn hidden_pre_tool_reasoning_stream_does_not_render_as_body() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(event(
        1,
        0,
        "req_hidden_reasoning",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_hidden_reasoning".to_string(),
                text: "Inspect the README".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        2,
        10,
        "req_hidden_reasoning",
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_hidden_reasoning".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Inspect the README".to_string(),
                request_digest: "digest-hidden-reasoning".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        3,
        100,
        "req_hidden_reasoning",
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_hidden_reasoning".to_string(),
                delta: "Hidden pre-tool reasoning.".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        4,
        250,
        "req_hidden_reasoning",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_hidden_read".to_string(),
                tool_id: "read".to_string(),
                args_summary: r#"{"filePath":"README.md"}"#.to_string(),
                args_digest: "digest-hidden-read-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        5,
        400,
        "req_hidden_reasoning",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_hidden_read".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("read README".to_string()),
                output_digest: Some("digest-hidden-read-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));

    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "hide thinking".chars() {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(ch),
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));

    let sections = build_transcript_sections(&app);
    assert_eq!(sections.len(), 1);
    let turn = &sections[0];
    assert_eq!(turn.thinking, None);
    assert!(matches!(
        turn.assistant_parts.as_slice(),
        [TranscriptAssistantPart::ToolCall(_)]
    ));

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        96,
    ));

    assert!(lines.iter().any(|line| line.contains("Read README.md")));
    assert!(lines
        .iter()
        .all(|line| !line.contains("Hidden pre-tool reasoning.")));
}
