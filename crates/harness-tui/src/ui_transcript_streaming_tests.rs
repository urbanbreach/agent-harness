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
    assert!(app.move_transcript_entry(true));
    assert!(app.toggle_selected_transcript_fold());
    assert!(app.reasoning_expanded(request_id));

    // When: the first answer delta closes the reasoning phase.
    delta(&mut app, 4, request_id, "final answer");

    // Then: finished reasoning returns to its default collapsed state.
    assert!(!app.reasoning_expanded(request_id));
}

#[test]
fn steering_user_row_stays_in_turn_output_order_after_settlement(
) -> Result<(), Box<dyn std::error::Error>> {
    use harness_core::event::{
        ActorKind, AssistantMessageFinishedEvent, EventActor, LiveEventEnvelope, LiveEventV1,
        RuntimeEvent, TaskCompletedEvent, TaskCompletionMetadata, TaskScheduleState,
        TaskScheduledEvent, TaskTerminalScope,
    };
    let live_text = |app: &mut AppState, id: &str, mono_ms, text: &str| {
        app.ingest_runtime_event(RuntimeEvent::Live(Box::new(LiveEventEnvelope {
            event_id: format!("live-{mono_ms}"),
            run_id: "run_streaming_unit".into(),
            mono_ms,
            ts: None,
            actor: EventActor::new(ActorKind::Worker, None),
            correlation_id: Some("running".into()),
            causation_id: None,
            stream_key: None,
            payload: LiveEventV1::ProviderTextDelta {
                request_id: id.into(),
                delta: text.into(),
            },
        })));
    };
    let commit = |app: &mut AppState, seq, id: &str, text: &str| {
        app.ingest_event(event(
            seq,
            "running",
            EventV1::AssistantMessageFinished(AssistantMessageFinishedEvent {
                request_id: id.into(),
                tool_call_count: 0,
                parts: vec![harness_core::session::AssistantPart::Text { text: text.into() }],
                provenance: None,
                assistant_message: None,
            }),
        ));
    };
    let mut app = AppState::new_live(None, false, None);
    app.set_reduced_motion(true);
    start_turn(&mut app, 1, "running", "original prompt");
    app.ingest_event(event(
        3,
        "running",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "running".into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:model-stream".into()),
            metadata: None,
        }),
    ));
    live_text(&mut app, "running", 4, "before steering");
    commit(&mut app, 4, "running", "before steering");
    app.handle_paste("steering sentinel");
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));
    assert!(app
        .activities
        .iter()
        .all(|activity| activity.status != ActivityStatus::Queued));
    app.ingest_event(event(
        5,
        "running",
        EventV1::UserMessageSubmitted(harness_core::event::UserMessageSubmittedEvent {
            request_id: "steer_message".into(),
            text: "steering sentinel".into(),
        }),
    ));
    // Delivery must merge the echo immediately, before any canonical settlement.
    assert_eq!(app.activities.len(), 1);
    assert_eq!(app.activities[0].status, ActivityStatus::Streaming);
    assert_eq!(app.queued_prompt_count, 0);
    app.ingest_event(event(
        6,
        "running",
        EventV1::ProviderRequestStarted(harness_core::event::ProviderRequestStartedEvent {
            request_id: "next_request".into(),
            provider_id: "mock".into(),
            model_id: "model-stream".into(),
            prompt_summary: "continue".into(),
            request_digest: "next-digest".into(),
            metadata: None,
        }),
    ));
    live_text(&mut app, "next_request", 7, "after steering");
    for settled in [false, true] {
        if settled {
            commit(&mut app, 7, "next_request", "after steering");
            app.ingest_event(event(
                8,
                "running",
                EventV1::ProviderRequestFinished(
                    harness_core::event::ProviderRequestFinishedEvent {
                        request_id: "next_request".into(),
                        finish_reason: "stop".into(),
                        output_digest: None,
                        usage: None,
                        metadata: None,
                    },
                ),
            ));
            app.ingest_event(event(
                9,
                "running",
                EventV1::TaskCompleted(TaskCompletedEvent {
                    task_id: "running".into(),
                    result_summary: "after steering".into(),
                    result_digest: "turn-result".into(),
                    metadata: Some(TaskCompletionMetadata {
                        task_scope: Some(TaskTerminalScope::AgentTurn),
                        ..Default::default()
                    }),
                }),
            ));
        }
        assert_eq!(app.activities.len(), 1);
        assert_eq!(app.queued_prompt_count, 0);
        let sections = build_transcript_sections(&app);
        let turn = sections.first().ok_or("missing turn")?;
        assert_eq!(turn.request_id, "running");
        assert_eq!(
            turn.user_message
                .as_ref()
                .ok_or("missing original prompt")?
                .text,
            "original prompt"
        );
        let theme = Theme::default();
        let surfaces = super::ui_transcript_render::build_transcript_render_surfaces(
            turn,
            &theme,
            100,
            theme.surface.canvas,
        );
        let rows = surfaces
            .iter()
            .filter_map(|surface| surface.source_text.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(
            rows,
            [
                "original prompt",
                "before steering",
                "steering sentinel",
                "after steering"
            ]
        );
        assert_eq!(
            surfaces
                .iter()
                .filter(|surface| surface.kind == TranscriptRenderSurfaceKind::User)
                .count(),
            2
        );
        assert!(!surfaces
            .iter()
            .flat_map(|surface| &surface.lines)
            .flat_map(|line| &line.spans)
            .any(|span| span.content.contains("QUEUED")));
    }
    app.ingest_event(event(
        10,
        "late_message",
        EventV1::UserMessageSubmitted(harness_core::event::UserMessageSubmittedEvent {
            request_id: "late_message".into(),
            text: "late sentinel".into(),
        }),
    ));
    assert_eq!(app.activities.len(), 2);
    assert_eq!(
        app.activities.back().ok_or("missing late turn")?.request_id,
        "late_message"
    );
    Ok(())
}
