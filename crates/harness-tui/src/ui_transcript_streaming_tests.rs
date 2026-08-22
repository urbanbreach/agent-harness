use super::*;
use harness_core::event::{EventEnvelopeV1, EventV1};

fn event(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: format!("evt_streaming_unit_{seq:04}"),
        seq,
        run_id: "run_streaming_unit".into(),
        mono_ms: seq,
        ts: None,
        actor: harness_core::event::EventActor::new(
            harness_core::event::ActorKind::System,
            Some("streaming-unit".to_string()),
        ),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: None,
        payload,
    }
}

fn start_turn(app: &mut AppState, seq: u64, request_id: &str, prompt: &str) {
    app.ingest_event(event(
        seq,
        request_id,
        EventV1::UserMessageSubmitted(harness_core::event::UserMessageSubmittedEvent {
            request_id: request_id.into(),
            text: prompt.to_string(),
        }),
    ));
    app.ingest_event(event(
        seq + 1,
        request_id,
        EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
            request_id: request_id.into(),
            provider_id: "mock".to_string(),
            model_id: "model-stream".to_string(),
            prompt_summary: prompt.to_string(),
            request_digest: format!("digest-{request_id}"),
            metadata: None,
        }),
    ));
}

fn delta(app: &mut AppState, seq: u64, request_id: &str, text: &str) {
    app.ingest_event(event(
        seq,
        request_id,
        EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
            request_id: request_id.into(),
            delta: text.to_string(),
        }),
    ));
}

fn reasoning_delta(app: &mut AppState, seq: u64, request_id: &str, text: &str) {
    app.ingest_event(event(
        seq,
        request_id,
        EventV1::ProviderReasoningDelta(harness_core::event::ProviderReasoningDeltaEvent {
            request_id: request_id.into(),
            delta: text.to_string(),
        }),
    ));
}

fn finish_turn(app: &mut AppState, seq: u64, request_id: &str) {
    app.ingest_event(event(
        seq,
        request_id,
        EventV1::ProviderRequestFinished(harness_core::event::ProviderRequestFinishedEvent {
            request_id: request_id.into(),
            finish_reason: "stop".to_string(),
            output_digest: None,
            usage: None,
            metadata: None,
        }),
    ));
}

#[test]
fn tool_boundary_settles_prior_body_while_trailing_body_streams() {
    let request_id = "req_body_boundary";
    let mut app = AppState::new_live(None, false, None);
    start_turn(&mut app, 1, request_id, "inspect then answer");
    delta(&mut app, 3, request_id, "before tool");
    app.ingest_event(event(
        4,
        request_id,
        EventV1::ToolCallRequested(harness_core::event::ToolCallRequestedEvent {
            tool_call_id: "tool_boundary".into(),
            tool_id: "read".to_string(),
            args_summary: r#"{"filePath":"src/lib.rs"}"#.to_string(),
            args_digest: "digest-tool-boundary".to_string(),
            metadata: None,
        }),
    ));
    delta(&mut app, 5, request_id, "after tool");

    let sections = build_transcript_sections(&app);
    let body_parts = sections[0]
        .assistant_parts
        .iter()
        .filter_map(|part| match part {
            TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(text)) => {
                Some((text.as_str(), false))
            }
            TranscriptAssistantPart::Body(TranscriptBodyBlock::StreamingRichText(text)) => {
                Some((text.as_str(), true))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        body_parts,
        vec![("before tool", false), ("after tool", true)]
    );
}

#[test]
fn provider_finish_settles_trailing_body() {
    let request_id = "req_body_finish";
    let mut app = AppState::new_live(None, false, None);
    start_turn(&mut app, 1, request_id, "finish body");
    delta(&mut app, 3, request_id, "settle me");
    finish_turn(&mut app, 4, request_id);

    let sections = build_transcript_sections(&app);

    assert!(matches!(
        sections[0].assistant_parts.as_slice(),
        [TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(text))]
            if text == "settle me"
    ));
}

#[test]
fn reasoning_transition_settles_the_preceding_body() {
    let request_id = "req_body_reasoning_boundary";
    let mut app = AppState::new_live(None, false, None);
    start_turn(&mut app, 1, request_id, "inspect then reason");
    app.ingest_event(event(
        3,
        request_id,
        EventV1::ToolCallRequested(harness_core::event::ToolCallRequestedEvent {
            tool_call_id: "tool_reasoning_boundary".into(),
            tool_id: "read".to_string(),
            args_summary: r#"{"filePath":"src/lib.rs"}"#.to_string(),
            args_digest: "digest-tool-reasoning-boundary".to_string(),
            metadata: None,
        }),
    ));
    delta(&mut app, 4, request_id, "body before reasoning");
    reasoning_delta(&mut app, 5, request_id, "reasoning after body");

    let sections = build_transcript_sections(&app);

    assert!(matches!(
        sections[0].assistant_parts.as_slice(),
        [
            TranscriptAssistantPart::ToolCall(_),
            TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(body)),
            TranscriptAssistantPart::Reasoning(reasoning),
        ] if body == "body before reasoning" && reasoning.text == "reasoning after body"
    ));
}

#[test]
fn provider_finish_settles_every_interleaved_body() {
    let request_id = "req_interleaved_body_finish";
    let mut app = AppState::new_live(None, false, None);
    start_turn(&mut app, 1, request_id, "inspect reason and answer");
    app.ingest_event(event(
        3,
        request_id,
        EventV1::ToolCallRequested(harness_core::event::ToolCallRequestedEvent {
            tool_call_id: "tool_interleaved_finish".into(),
            tool_id: "read".to_string(),
            args_summary: r#"{"filePath":"src/lib.rs"}"#.to_string(),
            args_digest: "digest-tool-interleaved-finish".to_string(),
            metadata: None,
        }),
    ));
    delta(&mut app, 4, request_id, "first body");
    reasoning_delta(&mut app, 5, request_id, "reasoning");
    delta(&mut app, 6, request_id, "second body");
    finish_turn(&mut app, 7, request_id);

    let sections = build_transcript_sections(&app);
    let body_parts = sections[0]
        .assistant_parts
        .iter()
        .filter_map(|part| match part {
            TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(text)) => {
                Some((text.as_str(), false))
            }
            TranscriptAssistantPart::Body(TranscriptBodyBlock::StreamingRichText(text)) => {
                Some((text.as_str(), true))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        body_parts,
        vec![("first body", false), ("second body", false)]
    );
}

#[test]
fn trailing_append_rerenders_only_the_active_turn_section() {
    let mut app = AppState::new_live(None, false, None);
    start_turn(&mut app, 1, "req_done", "first turn");
    delta(&mut app, 3, "req_done", "settled first response");
    finish_turn(&mut app, 4, "req_done");
    start_turn(&mut app, 5, "req_live", "second turn");
    delta(&mut app, 7, "req_live", "streaming response");
    let theme = Theme::default();
    let _ = build_measured_transcript_layout_for_width(&app, &theme, 100);
    reset_transcript_section_render_count_for_test();

    delta(&mut app, 8, "req_live", " grows");
    let _ = build_measured_transcript_layout_for_width(&app, &theme, 100);

    assert_eq!(transcript_section_render_count_for_test(), 1);
}

#[test]
fn animation_tick_reuses_all_measured_turn_sections() {
    // Given: one settled turn and one active streaming turn in the transcript cache.
    let mut app = AppState::new_live(None, false, None);
    start_turn(&mut app, 1, "req_done_tick", "first turn");
    delta(&mut app, 3, "req_done_tick", "settled first response");
    finish_turn(&mut app, 4, "req_done_tick");
    start_turn(&mut app, 5, "req_live_tick", "second turn");
    delta(&mut app, 7, "req_live_tick", "streaming response");
    let theme = Theme::default();
    let _ = build_measured_transcript_layout_for_width(&app, &theme, 100);
    reset_transcript_section_render_count_for_test();

    // When: only the active animation phase advances.
    app.advance_animation_tick();
    let _ = build_measured_transcript_layout_for_width(&app, &theme, 100);

    // Then: animation paint state reuses both settled and active measured sections.
    assert_eq!(transcript_section_render_count_for_test(), 0);
}

#[test]
fn answer_phase_collapses_reasoning_expanded_while_running() {
    // Given: a running reasoning trace that the user deliberately expanded.
    let request_id = "req_reasoning_disclosure";
    let mut app = AppState::new_live(None, false, None);
    start_turn(&mut app, 1, request_id, "reason then answer");
    app.ingest_event(event(
        3,
        request_id,
        EventV1::ProviderReasoningDelta(harness_core::event::ProviderReasoningDeltaEvent {
            request_id: request_id.into(),
            delta: "expanded reasoning".to_string(),
        }),
    ));
    app.transcript_view.selected_activity_index = 0;
    assert!(app.toggle_selected_transcript_fold());
    assert!(app.reasoning_expanded(request_id));

    // When: the first answer delta closes the reasoning phase.
    delta(&mut app, 4, request_id, "final answer");

    // Then: finished reasoning returns to its default collapsed state.
    assert!(!app.reasoning_expanded(request_id));
}
