use super::super::*;
use super::task_detail_blocks_text;
use crate::UnwrapOrAbort;

#[cfg(test)]
pub(crate) fn exact_test_transcript_section_model_preserves_activity_order() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![
        transcript_section_model_test_activity("request-a", ActivityStatus::Done, "first reply"),
        transcript_section_model_test_activity(
            "request-b",
            ActivityStatus::Streaming,
            "second reply",
        ),
    ]);
    app.transcript_view.selected_activity_index = 1;

    let sections = build_transcript_sections(&app);

    let turn_ids = sections
        .iter()
        .map(|section| section.request_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(turn_ids, vec!["request-a", "request-b"]);
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_section_model_keeps_nested_tool_and_error_blocks() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-tools",
        ActivityStatus::Error,
        "assistant body",
    );
    entry.thinking_text = "tool planning".to_string();
    entry.error_message = Some("tool call failed".to_string());
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-1".to_string(),
        tool_id: "shell.run".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"cmd":"false"}"#.to_string(),
        args_digest: "digest".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Failed,
        output_summary: Some("command failed".to_string()),
        output_digest: Some("out-digest".to_string()),
        output_json: None,
        truncated_output: Some("command failed".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);
    assert!(app.toggle_selected_transcript_fold());
    assert!(app.toggle_selected_transcript_fold());

    let sections = build_transcript_sections(&app);

    assert_eq!(sections.len(), 1);
    let turn = &sections[0];

    assert_eq!(
        turn.thinking.as_ref().map(|block| block.text.as_str()),
        Some("tool planning")
    );
    assert_eq!(
        turn.body_blocks,
        vec![TranscriptBodyBlock::RichText("assistant body".to_string())]
    );
    assert_eq!(
        turn.error.as_ref().map(|error| error.text.as_str()),
        Some("tool call failed")
    );
    assert_eq!(turn.tool_calls.len(), 1);
    assert_eq!(
        turn.tool_calls[0],
        TranscriptToolCallSection {
            tool_call_id: "call-1".to_string(),
            coalesced_tool_call_ids: vec!["call-1".to_string()],
            child_session_id: None,
            hovered_target: None,
            header: TranscriptToolCallHeader {
                tool_id: "shell.run".to_string(),
                title: "Shell".to_string(),
                subtitle: Some("Failed".to_string()),
                path_metadata: None,
                icon: None,
                status: ToolCallDisplayStatus::Failed,
                visual_style: TranscriptToolCallVisualStyle::Block,
                struck_out: false,
                disclosure_state: Some(TranscriptToolCallDisclosureState::Collapsed),
            },
            detail_blocks: vec![TranscriptToolCallDetailBlock::BashPanel {
                command: "false".to_string(),
                output: "command failed".to_string(),
                description: None,
                expand_hint: None,
                tone: TranscriptToolCallDetailTone::Error,
            }],
            details_collapsed_by_default: true,
            details_preview_visible: true,
            animation_phase: 0,
            expanded: false,
            rail_motion: ToolRailMotion::Settled,
        }
    );
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_reasoning_precedes_answer_and_tool_rows() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-answer-first",
        ActivityStatus::Done,
        "assistant answer",
    );
    entry.thinking_text = "working through the plan".to_string();
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-1".to_string(),
        tool_id: "fs.read".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"path":"src/ui.rs"}"#.to_string(),
        args_digest: "digest".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("24 lines read".to_string()),
        output_digest: Some("out-digest".to_string()),
        output_json: None,
        truncated_output: Some("24 lines read".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);
    assert!(app.toggle_selected_transcript_fold());

    let lines = build_transcript_lines_for_width(&app, &Theme::default(), 80)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    let reasoning_row = lines
        .iter()
        .position(|line| line.contains("working through the plan"))
        .unwrap_or_else(|| panic!("reasoning row missing: {lines:#?}"));
    let answer_row = lines
        .iter()
        .position(|line| line.contains("assistant answer"))
        .unwrap_or_else(|| panic!("answer row missing: {lines:#?}"));
    let tool_row = lines
        .iter()
        .enumerate()
        .skip(answer_row + 1)
        .find_map(|(index, line)| {
            (line.contains("src/ui.rs")
                || line.contains("24 lines")
                || line.to_ascii_lowercase().contains("read"))
            .then_some(index)
        })
        .unwrap_or_else(|| panic!("tool row missing: {lines:#?}"));

    assert!(reasoning_row < answer_row);
    assert!(answer_row < tool_row);
    assert!(lines
        .iter()
        .any(|line| line.contains("working through the plan")));
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_user_and_reasoning_match_reference_entry_body() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-entry-body",
        ActivityStatus::Done,
        "assistant answer",
    );
    entry.user_message = Some(harness_core::event::UserMessageSubmittedEvent {
        request_id: "request-entry-body".into(),
        text: "Explain transcript parity".to_string(),
    });
    entry.thinking_text = "Thinking: comparing reference entry body".to_string();
    app.activities = std::collections::VecDeque::from(vec![entry]);
    assert!(app.toggle_selected_transcript_fold());

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        96,
    ));
    let rendered = lines.join("\n");

    assert!(rendered.contains("❯ Explain transcript parity"));
    assert!(!rendered.contains("█Explain transcript parity"));
    assert!(!rendered.contains("┃  Explain transcript parity"));
    assert!(rendered.contains("Thinking: comparing reference entry body"));
    assert!(
        !rendered.contains("Thinking: Thinking:"),
        "reasoning should rewrite the reference leading Thinking: marker instead of adding a second label\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_redacted_only_reasoning_matches_reference_empty_body() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-redacted-reasoning",
        ActivityStatus::Done,
        "assistant answer",
    );
    entry.thinking_text = "[REDACTED]".to_string();
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        96,
    ));
    let rendered = lines.join("\n");

    assert!(rendered.contains("assistant answer"));
    assert!(
        !rendered.contains("Thinking:"),
        "reference behavior suppresses reasoning entries whose body is empty after [REDACTED] removal\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_latest_assistant_footer_stays_after_trailing_tool_rows() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-tool-after-body",
        ActivityStatus::Done,
        "I need to inspect the file first.",
    );
    let mut tool_call = transcript_section_model_test_tool_call("call-read", "fs.read");
    tool_call.args_summary = r#"{"path":"src/ui.rs"}"#.to_string();
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.output_summary = Some("24 lines read".to_string());
    tool_call.output_digest = Some("out-digest".to_string());
    tool_call.truncated_output = Some("24 lines read".to_string());
    tool_call.first_seq = 2;
    tool_call.last_seq = 3;
    entry.tool_calls.push(tool_call);
    entry.last_seq = 3;
    app.activities = std::collections::VecDeque::from(vec![entry]);

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        96,
    ));

    let body_row = lines
        .iter()
        .position(|line| line.contains("I need to inspect the file first."))
        .unwrap_or_abort();
    let tool_row = lines
        .iter()
        .position(|line| line.contains("Read 1 file"))
        .unwrap_or_abort();
    let footer_row = lines
        .iter()
        .position(|line| line.contains("Worked for") || line.contains("gpt-5.4-mini"))
        .unwrap_or_abort();

    assert!(
        body_row < tool_row,
        "tool row should render after the assistant prose\n{lines:#?}"
    );
    assert!(
        tool_row < footer_row,
        "assistant footer should stay pinned after trailing tool rows\n{lines:#?}"
    );
    assert!(
        lines[tool_row.saturating_sub(1)].trim().is_empty(),
        "reference separator rows insert one blank row for assistant block text followed by inline tool rows\n{lines:#?}"
    );
    if std::env::var_os("HARNESS_TUI_SPACING_RENDER_CAPTURE").is_some() {
        println!(
            "# Assistant body to inline tool spacing\n{}",
            lines.join("\n")
        );
    }
}

#[cfg(test)]
pub(crate) fn exact_test_transcript_tool_rows_follow_chronological_turn_order() {
    fn event(
        seq: u64,
        correlation_id: &str,
        payload: harness_core::event::EventV1,
    ) -> harness_core::event::EventEnvelopeV1 {
        harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt_turn_order_{seq:04}"),
            seq,
            run_id: "run_turn_order".into(),
            mono_ms: seq * 100,
            ts: Some("2026-03-22T14:36:00Z".to_string()),
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

    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(event(
        1,
        "req_ordered_turn",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_ordered_turn".into(),
                text: "Check transcript parity".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        2,
        "req_ordered_turn",
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_ordered_turn".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Check transcript parity".to_string(),
                request_digest: "digest-turn-order".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        3,
        "req_ordered_turn",
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_ordered_turn".into(),
                delta: "I’ll inspect the MCP result first.\n".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        4,
        "req_ordered_turn",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_docs".into(),
                tool_id: "mcp.docs-rs.search".to_string(),
                args_summary: r#"{"query":"ratatui"}"#.to_string(),
                args_digest: "digest-docs-search".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        5,
        "req_ordered_turn",
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tc_docs".into(),
        }),
    ));
    app.ingest_event(event(
        6,
        "req_ordered_turn",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_docs".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("ratatui 0.29.0".to_string()),
                output_digest: Some("digest-docs-result".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        7,
        "req_ordered_turn",
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_ordered_turn".into(),
                delta: "It returns the crate metadata inline afterward.".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        8,
        "req_ordered_turn",
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_ordered_turn".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-turn-order-output".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));

    let lines = build_transcript_lines_for_width(&app, &Theme::default(), 96)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    let opening_row = lines
        .iter()
        .position(|line| line.contains("I’ll inspect the MCP result first."))
        .unwrap_or_abort();
    let tool_row = lines
        .iter()
        .position(|line| line.contains("docs-rs_search") && line.contains("ratatui"))
        .unwrap_or_abort();
    let closing_row = lines
        .iter()
        .position(|line| line.contains("It returns the crate metadata inline afterward."))
        .unwrap_or_abort();

    assert!(
        opening_row < tool_row,
        "tool row should stay after the initial assistant prose"
    );
    assert!(
        tool_row < closing_row,
        "tool row should stay before the dependent assistant follow-up"
    );
}

#[test]
fn reasoning_after_tool_renders_in_new_block_below_tool() {
    // arrange
    // act
    // assert
    fn event(
        seq: u64,
        correlation_id: &str,
        payload: harness_core::event::EventV1,
    ) -> harness_core::event::EventEnvelopeV1 {
        harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt_reasoning_after_tool_{seq:04}"),
            seq,
            run_id: "run_reasoning_after_tool".into(),
            mono_ms: seq * 100,
            ts: Some("2026-03-22T15:00:00Z".to_string()),
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

    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(event(
        1,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_reasoning_after_tool".into(),
                text: "Inspect tokio docs".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        2,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_reasoning_after_tool".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Inspect tokio docs".to_string(),
                request_digest: "digest-reasoning-after-tool".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        3,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ProviderReasoningDelta(
            harness_core::event::ProviderReasoningDeltaEvent {
                request_id: "req_reasoning_after_tool".into(),
                delta: "Inspecting Tokio docs first.".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        4,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_tokio_docs".into(),
                tool_id: "mcp.docs-rs.search_in_crate".to_string(),
                args_summary: r#"{"query":"spawn"}"#.to_string(),
                args_digest: "digest-tokio-docs-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        5,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_tokio_docs".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("spawn docs".to_string()),
                output_digest: Some("digest-tokio-docs-output".to_string()),
                output_json: Some(serde_json::json!({
                    "server": { "id": "docs-rs" },
                    "payload": { "tool": "docs_rs_search_in_crate" }
                })),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        6,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ProviderReasoningDelta(
            harness_core::event::ProviderReasoningDeltaEvent {
                request_id: "req_reasoning_after_tool".into(),
                delta: "Now I can answer with the exact API.".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        7,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_reasoning_after_tool".into(),
                delta: "Use tokio::spawn for spawned tasks.".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        8,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_reasoning_after_tool".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-reasoning-after-tool-output".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));
    assert!(app.toggle_selected_transcript_fold());

    let sections = build_transcript_sections(&app);
    assert_eq!(sections.len(), 1);
    let turn = &sections[0];

    assert!(matches!(
        turn.assistant_parts.as_slice(),
        [
            TranscriptAssistantPart::Reasoning(_),
            TranscriptAssistantPart::ToolCall(_),
            TranscriptAssistantPart::Reasoning(_),
            TranscriptAssistantPart::Body(_)
        ]
    ));

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        96,
    ));
    let first_reasoning_row = lines
        .iter()
        .position(|line| line.contains("Inspecting Tokio docs first."))
        .unwrap_or_abort();
    let tool_row = lines
        .iter()
        .position(|line| line.contains("docs-rs_docs_rs_search_in_crate") && line.contains("spawn"))
        .unwrap_or_abort();
    let second_reasoning_row = lines
        .iter()
        .position(|line| line.contains("Now I can answer with the exact API."))
        .unwrap_or_abort();
    let answer_row = lines
        .iter()
        .position(|line| line.contains("Use tokio::spawn for spawned tasks."))
        .unwrap_or_abort();

    assert!(first_reasoning_row < tool_row);
    assert!(tool_row < second_reasoning_row);
    assert!(second_reasoning_row < answer_row);
    let line_before_second_reasoning = &lines[second_reasoning_row.saturating_sub(1)];
    assert!(
        line_before_second_reasoning.contains("Thinking")
            || line_before_second_reasoning.trim().is_empty(),
        "reasoning block should start with a header or a separator; got: {line_before_second_reasoning:?}"
    );
}

#[test]
fn task_completion_summary_does_not_duplicate_streamed_assistant_text() {
    // arrange
    // act
    // assert
    fn event(
        seq: u64,
        correlation_id: &str,
        payload: harness_core::event::EventV1,
    ) -> harness_core::event::EventEnvelopeV1 {
        harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt_duplicate_body_{seq:04}"),
            seq,
            run_id: "run_duplicate_body".into(),
            mono_ms: seq * 100,
            ts: Some("2026-03-22T14:40:00Z".to_string()),
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

    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(event(
        1,
        "req_duplicate_body",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_duplicate_body".into(),
                text: "Say hello".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        2,
        "req_duplicate_body",
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_duplicate_body".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Say hello".to_string(),
                request_digest: "digest-duplicate-body".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        3,
        "req_duplicate_body",
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_duplicate_body".into(),
                delta: "Hello!".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        4,
        "req_duplicate_body",
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_duplicate_body".into(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-duplicate-body-output".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        5,
        "req_duplicate_body",
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_duplicate_body".to_string().into(),
            result_summary: "Hello!".to_string(),
            result_digest: "digest-task-duplicate-body".to_string(),
            metadata: None,
        }),
    ));

    let sections = build_transcript_sections(&app);
    assert_eq!(sections.len(), 1);
    let turn = &sections[0];

    let body_parts = turn
        .assistant_parts
        .iter()
        .filter_map(|part| match part {
            TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(text)) => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(body_parts, vec!["Hello!"]);
}

#[test]
fn tool_task_completion_summary_does_not_render_as_assistant_body() {
    // arrange
    // act
    // assert
    fn event(
        seq: u64,
        correlation_id: &str,
        payload: harness_core::event::EventV1,
    ) -> harness_core::event::EventEnvelopeV1 {
        harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt_tool_task_body_{seq:04}"),
            seq,
            run_id: "run_tool_task_body".into(),
            mono_ms: seq * 100,
            ts: Some("2026-03-22T14:45:00Z".to_string()),
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

    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(event(
        1,
        "req_tool_task_body",
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_tool_task_body".into(),
                text: "Inspect tokio docs".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        2,
        "req_tool_task_body",
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_tool_task_body".into(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Inspect tokio docs".to_string(),
                request_digest: "digest-tool-task-body".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        3,
        "req_tool_task_body",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_docs_tokio".into(),
                tool_id: "mcp.docs-rs.search_in_crate".to_string(),
                args_summary: r#"{"crate_name":"tokio","query":"spawn"}"#.to_string(),
                args_digest: "digest-docs-tokio-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        4,
        "req_tool_task_body",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_docs_tokio".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("fn spawn\nstruct JoinHandle".to_string()),
                output_digest: Some("digest-docs-tokio-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        5,
        "req_tool_task_body",
        harness_core::event::EventV1::TaskCompleted(harness_core::event::TaskCompletedEvent {
            task_id: "task_docs_tokio".to_string().into(),
            result_summary: "fn spawn\nstruct JoinHandle".to_string(),
            result_digest: "digest-task-docs-tokio".to_string(),
            metadata: Some(harness_core::event::TaskCompletionMetadata {
                lineage: Some(harness_core::event::TaskLineageMetadata {
                    parent_tool_call_id: Some("tc_docs_tokio".to_string()),
                    ..harness_core::event::TaskLineageMetadata::default()
                }),
                task_scope: Some(harness_core::event::TaskTerminalScope::ToolCall),
                timing: None,
                hook_executions: Vec::new(),
            }),
        }),
    ));

    let rendered = build_transcript_lines_for_width(&app, &Theme::default(), 96)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(rendered.contains("docs-rs_search_in_crate"));
    assert!(rendered.contains("tokio"));
    assert!(rendered.contains("spawn"));
    assert!(
        !rendered.contains("fn spawn\nstruct JoinHandle")
            && !rendered.contains("struct JoinHandle"),
        "tool task completion summary must stay out of assistant body\n{rendered}"
    );
}

#[test]
fn task_row_renders_task_result_markdown_without_wrappers() {
    // arrange
    let tool_call = crate::app::ToolCallEntry {
        tool_call_id: "tc_noisy_task".to_string(),
        tool_id: "task".to_string(),
        canonical_tool_id: Some("task".to_string()),
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"description":"review streaming states","subagent_type":"explore"}"#
            .to_string(),
        args_digest: "digest-noisy-task-args".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some(
            "task_id: agent_000002 (for resuming to continue this task if needed)\
             \nrequest_id: req_000004\
             \n<task_result>\n.sisyphus/evidence/task-7-streaming-states-review.json\n</task_result>"
                .to_string(),
        ),
        output_digest: Some("digest-noisy-task-output".to_string()),
        output_json: None,
        truncated_output: None,
        edit: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: Some(4_000),
        permissions: Vec::new(),
        first_seq: 1,
        last_seq: 2,
        first_mono_ms: 1,
        last_mono_ms: 2,
        first_timestamp: None,
        last_timestamp: None,
    };

    let mut detail_blocks = Vec::new();
    // act
    let (title, icon, visual_style, _) =
        build_agent_spawn_tool_row(&tool_call, None, &mut detail_blocks, 0);
    // assert
    assert_eq!(title, "Task — review streaming states");
    assert_eq!(icon, Some("✓"));
    assert_eq!(visual_style, TranscriptToolCallVisualStyle::TaskInline);
    let detail_text = task_detail_blocks_text(&detail_blocks);
    assert!(detail_text.contains(".sisyphus/evidence/task-7-streaming-states-review.json"));
    assert!(!detail_text
        .split_whitespace()
        .any(|token| token.starts_with("sk-") && token.len() > 12));
    assert!(!detail_text.contains("task_id:"));
    assert!(!detail_text.contains("request_id:"));
    assert!(!detail_text.contains("<task_result>"));
}

#[test]
fn task_row_title_uses_partial_args_or_child_prompt_before_terminal_output() {
    let mut tool_call = crate::app::ToolCallEntry {
        tool_call_id: "tc_running_task".to_string(),
        tool_id: "task".to_string(),
        canonical_tool_id: Some("task".to_string()),
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"description":"review queued background completion wakeups","subagent_type":"explore","prompt":"long prompt"…"#
            .to_string(),
        args_digest: "digest-running-task-args".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Running,
        output_summary: None,
        output_digest: None,
        output_json: None,
        truncated_output: None,
        edit: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: 1,
        last_seq: 2,
        first_mono_ms: 1,
        last_mono_ms: 2,
        first_timestamp: None,
        last_timestamp: None,
    };

    let mut detail_blocks = Vec::new();
    let (title, icon, _, _) = build_agent_spawn_tool_row(&tool_call, None, &mut detail_blocks, 0);
    assert_eq!(title, "Task — review queued background completion wakeups");
    assert_ne!(title, "Delegating...");
    assert_eq!(icon, Some("⠋"));

    tool_call.args_summary = "{}".to_string();
    let task_row = crate::app::OrchestrationTaskRow {
        task_id: "task_child".to_string(),
        queue_key: Some("provider_model:mock:model-1".to_string()),
        state: crate::app::OrchestrationTaskState::Running,
        warning: None,
        owner_kind: harness_core::event::ActorKind::Worker,
        owner_agent_id: Some("agent_child".to_string()),
        request_id: Some("req_child".to_string()),
        parent_tool_call_id: None,
        parent_request_id: None,
        child_session_id: Some("agent_child".to_string()),
        child_request_id: Some("req_child".to_string()),
        result_summary: Some("inspect task behavior".to_string()),
        child_tool_call_count: 0,
        current_child_tool_title: None,
        timing_elapsed_ms: None,
        first_seq: 1,
        last_seq: 2,
        first_mono_ms: 1,
        last_mono_ms: 2,
        first_timestamp: None,
        last_timestamp: None,
    };
    let (title, _, _, _) =
        build_agent_spawn_tool_row(&tool_call, Some(&task_row), &mut Vec::new(), 0);
    assert_eq!(title, "Task — inspect task behavior");

    let fallback_task_row = crate::app::OrchestrationTaskRow {
        result_summary: None,
        ..task_row
    };
    let (title, icon, _, _) =
        build_agent_spawn_tool_row(&tool_call, Some(&fallback_task_row), &mut Vec::new(), 0);
    assert_eq!(title, "Task");
    assert_eq!(icon, Some("⠋"));
}

#[test]
fn background_output_tool_row_confirms_checked_child_result() {
    // arrange
    // act
    // assert
    let mut tool_call =
        transcript_section_model_test_tool_call("tc-background-output", "background_output");
    tool_call.status = ToolCallDisplayStatus::Succeeded;
    tool_call.args_summary = r#"{"request_id":"req_child"}"#.to_string();
    tool_call.output_json = Some(serde_json::json!({
        "request_id": "req_child",
        "task_id": "agent_child",
        "status": "completed",
        "duration_ms": 1600,
        "child_tool_call_count": 2
    }));

    let section = build_transcript_tool_call_section(
        &tool_call,
        &AppState::default(),
        None,
        true,
        false,
        false,
        false,
        None,
    );

    assert_eq!(section.header.title, "Checked background output");
    assert_eq!(section.header.icon, Some("↻"));
    assert_eq!(
        section.header.subtitle.as_deref(),
        Some("req_child · completed · 2 child tool calls · 1.6s")
    );

    let visible_without_details = build_tool_call_section(
        &tool_call,
        &AppState::default(),
        false,
        true,
        false,
        false,
        false,
        None,
    )
    .unwrap_or_abort();
    assert_eq!(
        visible_without_details.header.title,
        "Checked background output"
    );
}

#[test]
fn inline_metadata_collapse_removes_terminal_controls() {
    // arrange
    // act
    // assert
    assert_eq!(
        collapse_inline_whitespace("researcher\u{1b}]0;owned\u{7} task\nsummary"),
        "researcher ]0;owned task summary"
    );
}

fn surface_line_text(surface: &MeasuredTranscriptSurface) -> String {
    surface
        .lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
pub(crate) fn exact_test_selected_turn_with_tool_stays_rail_free() {
    // Given: a selected completed turn with Thought chrome and a succeeded tool.
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-tool-selected",
        ActivityStatus::Done,
        "DONE",
    );
    entry.thinking_text = "planning".to_string();
    let mut tool = transcript_section_model_test_tool_call("call-list-1", "list");
    tool.status = ToolCallDisplayStatus::Succeeded;
    tool.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    tool.args_summary = r#"{"path":"."}"#.to_string();
    tool.output_json = Some(serde_json::json!({ "entry_count": 1 }));
    tool.output_summary = Some("Listed 1 dir".to_string());
    entry.tool_calls.push(tool);
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    // When: measuring the selected turn surfaces
    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 120);
    assert_eq!(layout.sections.len(), 1);
    let surfaces = &layout.sections[0].surfaces;
    // Then: selection does not restore the removed legacy rail on any surface.
    assert_eq!(
        surfaces
            .iter()
            .filter(|surface| surface.selected_rail)
            .count(),
        0
    );
}

#[cfg(test)]
pub(crate) fn exact_test_selected_turn_without_tool_stays_rail_free() {
    // Given: a selected completed turn with Thought chrome and no tools.
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-thought-selected",
        ActivityStatus::Done,
        "HELLO_PARITY_OK",
    );
    entry.thinking_text = "planning".to_string();
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    // When: measuring the selected turn surfaces
    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 120);
    assert_eq!(layout.sections.len(), 1);
    let surfaces = &layout.sections[0].surfaces;
    // Then: selection does not restore the removed legacy rail on Thought.
    assert_eq!(
        surfaces
            .iter()
            .filter(|surface| surface.selected_rail)
            .count(),
        0
    );
}

#[cfg(test)]
pub(crate) fn exact_test_done_body_after_tool_keeps_separate_wall_clock_row() {
    // Given: a completed tool turn with single-line DONE body + footer wall clock
    // Tool seq must precede body (last_seq) so assistant_parts order is Tool → Body
    // (matches the reference diff / live_diff event order).
    let mut app = AppState::default();
    let mut entry =
        transcript_section_model_test_activity("request-done-clock", ActivityStatus::Done, "DONE");
    entry.thinking_text = "planning".to_string();
    entry.user_timestamp = Some("2026-03-19T12:00:00Z".to_string());
    entry.first_seq = 1;
    entry.last_seq = 30;
    let mut tool = transcript_section_model_test_tool_call("call-list-done", "list");
    tool.status = ToolCallDisplayStatus::Succeeded;
    tool.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    tool.args_summary = r#"{"path":"."}"#.to_string();
    tool.output_json = Some(serde_json::json!({ "entry_count": 1 }));
    tool.output_summary = Some("Listed 1 dir".to_string());
    tool.first_seq = 10;
    entry.tool_calls.push(tool);
    app.activities = std::collections::VecDeque::from(vec![entry]);

    // When: measuring transcript surfaces
    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 120);
    let lines: Vec<String> = layout.sections[0]
        .surfaces
        .iter()
        .flat_map(|surface| {
            surface_line_text(surface)
                .lines()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();

    // Then: the reference diff state keeps wall clock on its own row between the tool and DONE body.
    let done_line = lines
        .iter()
        .find(|line| line.contains("DONE"))
        .unwrap_or_else(|| panic!("missing DONE body line\n{lines:#?}"));
    assert!(
        !done_line.contains("12:00 PM"),
        "DONE after tools must keep wall clock on a separate row\n{done_line:?}\nall={lines:#?}"
    );
    let clock_only = lines.iter().any(|line| {
        let trimmed = line.trim();
        trimmed == "12:00 PM" || (trimmed.ends_with("12:00 PM") && !trimmed.contains("DONE"))
    });
    assert!(
        clock_only,
        "Tool→Body single-line must keep a dedicated clock-only row\nall={lines:#?}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_body_after_thought_packs_wall_clock_on_same_line() {
    // Given: a completed no-tool turn (Thought → body) with footer wall clock
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-hello-clock",
        ActivityStatus::Done,
        "HELLO_PARITY_OK",
    );
    entry.thinking_text = "planning".to_string();
    entry.user_timestamp = Some("2026-03-19T12:00:00Z".to_string());
    app.activities = std::collections::VecDeque::from(vec![entry]);

    // When: measuring transcript surfaces
    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 120);
    let lines: Vec<String> = layout.sections[0]
        .surfaces
        .iter()
        .flat_map(|surface| {
            surface_line_text(surface)
                .lines()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();

    // Then: the reference completed state packs wall clock on the single-line body row.
    let hello_idx = lines
        .iter()
        .position(|line| line.contains("HELLO_PARITY_OK"))
        .unwrap_or_else(|| panic!("missing HELLO body line\n{lines:#?}"));
    let hello_line = &lines[hello_idx];
    assert!(
        hello_line.contains("12:00 PM"),
        "Thought→Body must pack wall clock onto the body line\n{hello_line:?}\nall={lines:#?}"
    );
    let clock_only = lines.iter().any(|line| {
        let trimmed = line.trim();
        (trimmed == "12:00 PM" || trimmed.ends_with("12:00 PM")) && !trimmed.contains("HELLO")
    });
    assert!(
        !clock_only,
        "No-tool single-line body must not keep a dedicated clock-only row\nall={lines:#?}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_tool_turn_without_thinking_omits_thought() {
    // Given: a completed tool turn with empty thinking text (reference tool state)
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-tool-no-thought",
        ActivityStatus::Done,
        "COUNT=2",
    );
    entry.thinking_text.clear();
    let mut tool = transcript_section_model_test_tool_call("call-list-no-thought", "list");
    tool.status = ToolCallDisplayStatus::Succeeded;
    tool.lifecycle_state = Some(harness_core::event::ToolCallLifecycleState::Completed);
    tool.args_summary = r#"{"path":"."}"#.to_string();
    tool.output_json = Some(serde_json::json!({ "entry_count": 1 }));
    tool.output_summary = Some("Listed 1 dir".to_string());
    entry.tool_calls.push(tool);
    app.activities = std::collections::VecDeque::from(vec![entry]);

    // When: measuring transcript surfaces
    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 120);
    let rendered = layout.sections[0]
        .surfaces
        .iter()
        .map(surface_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    // Then: the reference tool state omits Thought when there was no reasoning.
    assert!(
        !rendered.contains("Thought for"),
        "completed tool turns without thinking must omit Thought chrome\n{rendered}"
    );
    assert!(
        rendered.contains("Listed") || rendered.contains("◈"),
        "tool surface must still render\n{rendered}"
    );
    assert!(
        rendered.contains("COUNT=2"),
        "assistant body must still render\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_no_tool_turn_without_thinking_keeps_thought() {
    // Given: a completed answer-only turn with empty thinking
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-complete-no-thought",
        ActivityStatus::Done,
        "HELLO_PARITY_OK",
    );
    entry.thinking_text.clear();
    app.activities = std::collections::VecDeque::from(vec![entry]);

    // When: measuring transcript surfaces
    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 120);
    let rendered = layout.sections[0]
        .surfaces
        .iter()
        .map(surface_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    // Then: no Thought chrome for turns without reasoning (pinned reference freeze)
    assert!(
        !rendered.contains("Thought for"),
        "completed no-tool turns without reasoning must not show Thought chrome\n{rendered}"
    );
    assert!(
        rendered.contains("HELLO_PARITY_OK"),
        "body must still render\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_pending_question_has_no_selected_rail() {
    // Given: a selected streaming turn with pending question tool (Waiting on answers)
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-question-no-rail",
        ActivityStatus::Streaming,
        "",
    );
    entry.thinking_text = "**plan**".to_string();
    let mut tool =
        transcript_section_model_test_tool_call("call-question-no-rail", "user.question");
    tool.status = ToolCallDisplayStatus::PendingPermission;
    tool.args_summary = serde_json::json!({
        "questions": [{
            "question": "Pick one",
            "header": "Choice",
            "options": [{"label": "A", "description": "Option A"}]
        }]
    })
    .to_string();
    entry.tool_calls.push(tool);
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    // When: measuring transcript surfaces
    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 120);
    let surfaces = &layout.sections[0].surfaces;
    let selected: Vec<_> = surfaces
        .iter()
        .filter(|surface| surface.selected_rail)
        .map(surface_line_text)
        .collect();
    let rendered = surfaces
        .iter()
        .map(surface_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    // Then: the reference question state paints no ❙ while Waiting on answers.
    assert!(
        selected.is_empty(),
        "pending question turns must not paint selected rail\nselected={selected:#?}\n{rendered}"
    );
    assert!(
        rendered.contains("Ask Pick one") || rendered.contains("Ask "),
        "Ask chrome must still render\n{rendered}"
    );
    assert!(
        rendered.contains("Waiting on answers"),
        "Waiting chrome must still render\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_pending_edit_permission_has_no_selected_rail() {
    // Given: selected streaming turn with pending write under permission (Creating demo.txt)
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-perm-no-rail",
        ActivityStatus::Streaming,
        "",
    );
    entry.thinking_text = "**plan**".to_string();
    entry.first_mono_ms = 0;
    entry.last_mono_ms = 19_000;
    let mut tool = transcript_section_model_test_tool_call("call-write-perm-no-rail", "fs.write");
    tool.status = ToolCallDisplayStatus::PendingPermission;
    tool.args_summary = r#"{"path":"demo.txt","content":"parity-ok\n"}"#.to_string();
    entry.tool_calls.push(tool);
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    // When: measuring transcript surfaces
    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 120);
    let surfaces = &layout.sections[0].surfaces;
    let selected: Vec<_> = surfaces
        .iter()
        .filter(|surface| surface.selected_rail)
        .map(surface_line_text)
        .collect();
    let rendered = surfaces
        .iter()
        .map(surface_line_text)
        .collect::<Vec<_>>()
        .join("\n");

    // Then: freeze PERM paints no ❙ on Creating while Allow Edit dock is open
    assert!(
        selected.is_empty(),
        "pending edit permission turns must not paint selected rail\nselected={selected:#?}\n{rendered}"
    );
    assert!(
        rendered.contains("Creating demo.txt"),
        "Creating chrome must still render\n{rendered}"
    );
    assert!(
        rendered.contains("Run Write `demo.txt`"),
        "Run Write chrome must still render\n{rendered}"
    );
}

#[cfg(test)]
pub(crate) fn exact_test_pending_edit_permission_packs_dual_run_write_duration() {
    // Given: pending write permission turn with 19s elapsed
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-perm-dual-19s",
        ActivityStatus::Streaming,
        "",
    );
    entry.thinking_text = "**plan**".to_string();
    entry.first_mono_ms = 0;
    entry.last_mono_ms = 19_000;
    entry.usage = Some(crate::app::ActivityUsage {
        prompt_tokens: 8_000,
        completion_tokens: 2_100,
        total_tokens: 10_100,
    });
    let mut tool = transcript_section_model_test_tool_call("call-write-dual-19s", "fs.write");
    tool.status = ToolCallDisplayStatus::PendingPermission;
    tool.args_summary = r#"{"path":"demo.txt","content":"parity-ok\n"}"#.to_string();
    entry.tool_calls.push(tool);
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    // When: rendering transcript lines
    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        120,
    ));
    let rendered = lines.join("\n");
    let run_line = lines
        .iter()
        .find(|line| line.contains("Run Write"))
        .cloned()
        .unwrap_or_default();

    // Then: freeze packs inline 19s after path and right-meta 19s
    assert!(
        run_line.contains("Run Write `demo.txt` 19s"),
        "Run Write left must pack inline duration 19s\n{run_line}\n{rendered}"
    );
    assert!(
        run_line.contains("19s") && run_line.contains("⇣10.1k") && run_line.contains("[stop]"),
        "Run Write right meta must keep 19s ⇣10.1k [stop]\n{run_line}\n{rendered}"
    );
    let nineteen_count = run_line.matches("19s").count();
    assert!(
        nineteen_count >= 2,
        "freeze dual-duration packing needs 19s on both left and right\n{run_line}"
    );
}
