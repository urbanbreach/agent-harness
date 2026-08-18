use super::super::*;

use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderReasoningDeltaEvent,
    ProviderRequestFinishedEvent, ProviderRequestStartedEvent, ProviderStreamDeltaEvent,
    ToolCallFinishedEvent, ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};
use ratatui::{backend::TestBackend, Terminal};

fn event(seq: u64, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_semantic_entry_{seq:04}"),
        seq,
        run_id: "run_semantic_entry".into(),
        mono_ms: seq.saturating_mul(100),
        ts: Some("2026-08-14T12:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::Worker, Some("agent_parent".to_string())),
        correlation_id: Some("req_semantic_entry".to_string()),
        causation_id: None,
        stream_key: None,
        payload,
    }
}

fn mixed_streaming_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    for envelope in [
        event(
            1,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_semantic_entry".into(),
                text: "Inspect the transcript composition".to_string(),
            }),
        ),
        event(
            2,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_semantic_entry".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Inspect the transcript composition".to_string(),
                request_digest: "digest-semantic-entry".to_string(),
                metadata: None,
            }),
        ),
        event(
            3,
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_semantic_entry".into(),
                delta: "I will inspect the source first.\n".to_string(),
            }),
        ),
        event(
            4,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tool-read".into(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/ui.rs"}"#.to_string(),
                args_digest: "digest-read".to_string(),
                metadata: None,
            }),
        ),
        event(
            5,
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tool-read".into(),
            }),
        ),
        event(
            6,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tool-read".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("24 lines read".to_string()),
                output_digest: Some("digest-read-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        event(
            7,
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_semantic_entry".into(),
                delta: "The first result is useful.\n".to_string(),
            }),
        ),
        event(
            8,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tool-failed".into(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"false"}"#.to_string(),
                args_digest: "digest-failed".to_string(),
                metadata: None,
            }),
        ),
        event(
            9,
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tool-failed".into(),
            }),
        ),
        event(
            10,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tool-failed".into(),
                status: ToolCallStatus::Failed,
                output_summary: Some("exit status 1".to_string()),
                output_digest: Some("digest-failed-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        event(
            11,
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_semantic_entry".into(),
                delta: "Streaming synthesis remains visible.".to_string(),
            }),
        ),
    ] {
        app.ingest_event(envelope);
    }
    app
}

fn source_identity_app(show_reasoning: bool) -> AppState {
    let envelopes = [
        event(
            1,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_semantic_entry".into(),
                text: "Check semantic identity".to_string(),
            }),
        ),
        event(
            2,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_semantic_entry".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Check semantic identity".to_string(),
                request_digest: "digest-semantic-identity".to_string(),
                metadata: None,
            }),
        ),
        event(
            3,
            EventV1::ProviderReasoningDelta(ProviderReasoningDeltaEvent {
                request_id: "req_semantic_entry".into(),
                delta: "Inserted reasoning.".to_string(),
            }),
        ),
        event(
            4,
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_semantic_entry".into(),
                delta: "Retained body before tool.".to_string(),
            }),
        ),
        event(
            5,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tool-identity".into(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/lib.rs"}"#.to_string(),
                args_digest: "digest-tool-identity".to_string(),
                metadata: None,
            }),
        ),
        event(
            6,
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tool-identity".into(),
            }),
        ),
        event(
            7,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tool-identity".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("1 file read".to_string()),
                output_digest: Some("digest-tool-identity-output".to_string()),
                output_json: None,
                metadata: None,
            }),
        ),
        event(
            8,
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: "req_semantic_entry".into(),
                delta: "Retained body after tool.".to_string(),
            }),
        ),
        event(
            9,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: "req_semantic_entry".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-semantic-identity-output".to_string()),
                usage: None,
                metadata: None,
            }),
        ),
    ];
    let mut app = AppState::new_live(None, false, None);
    for envelope in envelopes {
        app.ingest_event(envelope);
    }
    app.transcript_view.show_transcript_thinking = show_reasoning;
    app
}

fn semantic_entry_id_for_text(app: &AppState, text: &str) -> TranscriptVisualEntryId {
    let sections = build_transcript_sections(app);
    semantic_entry_id_for_text_in_section(&sections[0], text)
}

fn semantic_entry_id_for_text_in_section(
    section: &TranscriptTurnSection,
    text: &str,
) -> TranscriptVisualEntryId {
    let theme = Theme::default();
    let entries = build_transcript_render_surfaces(section, &theme, 100, theme.surface.shell);
    entries
        .into_iter()
        .find(|entry| {
            let rendered = entry
                .lines
                .iter()
                .flat_map(|line| line.spans.iter())
                .map(|span| span.content.as_ref())
                .collect::<String>();
            rendered.contains(text)
        })
        .unwrap_or_else(|| panic!("semantic entry with retained text {text:?}"))
        .metadata
        .id
}

fn render_text(app: &AppState, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| crate::ui::render_app(frame, app))
        .expect("render mixed transcript");
    terminal
        .backend()
        .buffer()
        .content
        .chunks(usize::from(width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_semantic_entry_viewport(width: u16, height: u16) {
    let theme = Theme::default();
    let mut app = mixed_streaming_app();
    let streaming_frame = render_text(&app, width, height);
    assert_eq!(streaming_frame.lines().count(), usize::from(height));

    let sections = build_transcript_sections(&app);
    let streaming_entries =
        build_transcript_render_surfaces(&sections[0], &theme, width, theme.surface.shell);
    let user = streaming_entries
        .iter()
        .find(|entry| entry.kind == TranscriptRenderSurfaceKind::User)
        .expect("user entry");
    assert_eq!(
        user.surface, theme.surface.card,
        "ordinary user composition must paint the elevated card surface at {width}x{height}"
    );
    assert_eq!(
        streaming_entries
            .iter()
            .filter(|entry| entry.show_outer_rail || entry.selected_rail)
            .count(),
        3,
        "settled and independently active semantic group rails must remain visible at {width}x{height}"
    );
    assert_eq!(
        streaming_entries
            .iter()
            .filter(|entry| entry.metadata.accent != TranscriptVisualEntryAccent::Hidden)
            .count(),
        2,
        "independently active semantic groups must retain distinct accent ownership at {width}x{height}"
    );

    let streaming_document =
        transcript_test_line_texts(build_transcript_lines_for_width(&app, &theme, width))
            .join("\n");
    for expected in [
        "Inspect the transcript composition",
        "I will inspect the source first.",
        "Read 1 file",
        "false",
        "Streaming synthesis remains visible.",
    ] {
        assert!(
            streaming_document.contains(expected),
            "missing {expected:?} at {width}x{height}:\n{streaming_document}"
        );
    }

    app.ingest_event(event(
        12,
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req_semantic_entry".into(),
            finish_reason: "stop".to_string(),
            output_digest: Some("digest-semantic-entry-output".to_string()),
            usage: None,
            metadata: None,
        }),
    ));
    let settled_frame = render_text(&app, width, height);
    assert_eq!(settled_frame.lines().count(), usize::from(height));

    let sections = build_transcript_sections(&app);
    let settled_entries =
        build_transcript_render_surfaces(&sections[0], &theme, width, theme.surface.shell);
    assert_eq!(
        settled_entries
            .iter()
            .filter(|entry| entry.show_outer_rail || entry.selected_rail)
            .count(),
        2,
        "settled semantic groups must retain their dim state rails at {width}x{height}"
    );
    assert_eq!(
        settled_entries
            .iter()
            .filter(|entry| entry.metadata.accent != TranscriptVisualEntryAccent::Hidden)
            .count(),
        1,
        "settled semantic entries retain only the selected accent owner at {width}x{height}"
    );
    assert!(
        settled_entries.iter().all(|entry| {
            entry.surface == theme.surface.shell
                || (entry.kind == TranscriptRenderSurfaceKind::User
                    && entry.surface == theme.surface.card)
                || entry.kind == TranscriptRenderSurfaceKind::Compaction
        }),
        "settled user entries must stay elevated while tool and assistant entries use the base surface at {width}x{height}"
    );
}

macro_rules! semantic_entry_viewport_case {
    ($name:ident, $width:literal, $height:literal) => {
        #[test]
        fn $name() {
            assert_semantic_entry_viewport($width, $height);
        }
    };
}

semantic_entry_viewport_case!(semantic_entries_compose_at_60x20, 60, 20);
semantic_entry_viewport_case!(semantic_entries_compose_at_79x24, 79, 24);
semantic_entry_viewport_case!(semantic_entries_compose_at_80x24, 80, 24);
semantic_entry_viewport_case!(semantic_entries_compose_at_100x30, 100, 30);
semantic_entry_viewport_case!(semantic_entries_compose_at_120x40, 120, 40);
semantic_entry_viewport_case!(semantic_entries_compose_at_132x40, 132, 40);

#[test]
fn retained_assistant_entries_keep_source_ids_after_earlier_insertion_and_replay() {
    let baseline = source_identity_app(false);
    let inserted = source_identity_app(true);
    let replayed = source_identity_app(true);
    let mut reordered = build_transcript_sections(&inserted)
        .into_iter()
        .next()
        .expect("inserted turn");
    let body_index = |needle: &str| {
        reordered
            .assistant_parts
            .iter()
            .position(|part| match part {
                TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(text))
                | TranscriptAssistantPart::Body(TranscriptBodyBlock::StreamingRichText(text)) => {
                    text.contains(needle)
                }
                TranscriptAssistantPart::Reasoning(_)
                | TranscriptAssistantPart::ToolCall(_)
                | TranscriptAssistantPart::Error(_)
                | TranscriptAssistantPart::Compaction(_) => false,
            })
            .expect("retained body part")
    };
    let before_index = body_index("before tool");
    let after_index = body_index("after tool");
    reordered.assistant_parts.swap(before_index, after_index);
    reordered
        .assistant_part_source_ids
        .swap(before_index, after_index);

    for text in ["Retained body before tool.", "Retained body after tool."] {
        let baseline_id = semantic_entry_id_for_text(&baseline, text);
        let inserted_id = semantic_entry_id_for_text(&inserted, text);
        let replayed_id = semantic_entry_id_for_text(&replayed, text);
        let reordered_id = semantic_entry_id_for_text_in_section(&reordered, text);

        assert_eq!(
            inserted_id, baseline_id,
            "an earlier semantic part must not change the retained entry ID for {text:?}"
        );
        assert_eq!(
            replayed_id, inserted_id,
            "replay must reproduce the retained entry ID for {text:?}"
        );
        assert_eq!(
            reordered_id, inserted_id,
            "reordering retained semantic parts must preserve the entry ID for {text:?}"
        );
    }
}

#[test]
fn concurrent_active_entries_keep_independent_semantic_group_rails() {
    let theme = Theme::default();
    let mut app = AppState::new_live(None, false, None);
    for envelope in [
        event(
            1,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: "req_semantic_entry".into(),
                text: "Run concurrent tools".to_string(),
            }),
        ),
        event(
            2,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: "req_semantic_entry".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Run concurrent tools".to_string(),
                request_digest: "digest-concurrent-tools".to_string(),
                metadata: None,
            }),
        ),
        event(
            3,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tool-read-active".into(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/lib.rs"}"#.to_string(),
                args_digest: "digest-read-active".to_string(),
                metadata: None,
            }),
        ),
        event(
            4,
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tool-read-active".into(),
            }),
        ),
        event(
            5,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tool-shell-active".into(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"cargo check"}"#.to_string(),
                args_digest: "digest-shell-active".to_string(),
                metadata: None,
            }),
        ),
        event(
            6,
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tool-shell-active".into(),
            }),
        ),
    ] {
        app.ingest_event(envelope);
    }

    let sections = build_transcript_sections(&app);
    let entries = build_transcript_render_surfaces(&sections[0], &theme, 100, theme.surface.shell);
    let active_tools = entries
        .iter()
        .filter(|entry| {
            matches!(
                entry.kind,
                TranscriptRenderSurfaceKind::AssistantTool
                    | TranscriptRenderSurfaceKind::AssistantCommandTool
            ) && entry.metadata.lifecycle == TranscriptVisualEntryLifecycle::Active
        })
        .count();

    assert_eq!(
        active_tools, 2,
        "both running tools retain active lifecycle"
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.metadata.accent != TranscriptVisualEntryAccent::Hidden)
            .count(),
        2,
        "the active context group and command entry both own visible state rails"
    );
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.tool_rail_motion.is_some())
            .count(),
        2,
        "both active semantic entries must paint their motion rails"
    );
    assert_eq!(
        entries.iter().filter(|entry| entry.selected_rail).count(),
        0,
        "prompt-focused transcripts must not claim a selected assistant rail"
    );
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.kind == TranscriptRenderSurfaceKind::User)
            .expect("user entry")
            .metadata
            .accent,
        TranscriptVisualEntryAccent::Hidden,
        "assistant selection must supersede the turn-selected user entry"
    );
}
