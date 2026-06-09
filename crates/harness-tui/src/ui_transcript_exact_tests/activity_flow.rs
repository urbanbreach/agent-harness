use super::super::*;
use super::task_detail_blocks_text;

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
    app.selected_activity_index = 1;

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
            child_session_id: None,
            hovered_target: None,
            header: TranscriptToolCallHeader {
                tool_id: "shell.run".to_string(),
                title: "false".to_string(),
                subtitle: None,
                path_metadata: None,
                icon: Some("$"),
                status: ToolCallDisplayStatus::Failed,
                visual_style: TranscriptToolCallVisualStyle::Inline,
                struck_out: false,
                disclosure_state: Some(TranscriptToolCallDisclosureState::Collapsed),
            },
            detail_blocks: vec![TranscriptToolCallDetailBlock::Message {
                text: "command failed".to_string(),
                tone: TranscriptToolCallDetailTone::Error,
            }],
            expanded: false,
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
        .expect("reasoning row");
    let answer_row = lines
        .iter()
        .position(|line| line.contains("assistant answer"))
        .expect("assistant answer row");
    let tool_row = lines
        .iter()
        .enumerate()
        .skip(answer_row + 1)
        .find_map(|(index, line)| line.contains("Read src/ui.rs").then_some(index))
        .expect("tool row");

    assert!(reasoning_row < answer_row);
    assert!(answer_row < tool_row);
    assert!(lines
        .iter()
        .any(|line| line.contains("working through the plan")));
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
            run_id: "run_turn_order".to_string(),
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
                request_id: "req_ordered_turn".to_string(),
                text: "Check transcript parity".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        2,
        "req_ordered_turn",
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_ordered_turn".to_string(),
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
                request_id: "req_ordered_turn".to_string(),
                delta: "I’ll inspect the MCP result first.\n".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        4,
        "req_ordered_turn",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_docs".to_string(),
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
            tool_call_id: "tc_docs".to_string(),
        }),
    ));
    app.ingest_event(event(
        6,
        "req_ordered_turn",
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_docs".to_string(),
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
                request_id: "req_ordered_turn".to_string(),
                delta: "It returns the crate metadata inline afterward.".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        8,
        "req_ordered_turn",
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_ordered_turn".to_string(),
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
        .expect("opening answer row");
    let tool_row = lines
        .iter()
        .position(|line| line.contains("docs-rs_search") && line.contains("ratatui"))
        .expect("tool row");
    let closing_row = lines
        .iter()
        .position(|line| line.contains("It returns the crate metadata inline afterward."))
        .expect("closing answer row");

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
    fn event(
        seq: u64,
        correlation_id: &str,
        payload: harness_core::event::EventV1,
    ) -> harness_core::event::EventEnvelopeV1 {
        harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt_reasoning_after_tool_{seq:04}"),
            seq,
            run_id: "run_reasoning_after_tool".to_string(),
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
                request_id: "req_reasoning_after_tool".to_string(),
                text: "Inspect tokio docs".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        2,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_reasoning_after_tool".to_string(),
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
                request_id: "req_reasoning_after_tool".to_string(),
                delta: "Inspecting Tokio docs first.".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        4,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_tokio_docs".to_string(),
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
                tool_call_id: "tc_tokio_docs".to_string(),
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
                request_id: "req_reasoning_after_tool".to_string(),
                delta: "Now I can answer with the exact API.".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        7,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_reasoning_after_tool".to_string(),
                delta: "Use tokio::spawn for spawned tasks.".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        8,
        "req_reasoning_after_tool",
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_reasoning_after_tool".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-reasoning-after-tool-output".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));

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
        .expect("first reasoning row");
    let tool_row = lines
        .iter()
        .position(|line| line.contains("docs-rs_docs_rs_search_in_crate") && line.contains("spawn"))
        .expect("tool row");
    let second_reasoning_row = lines
        .iter()
        .position(|line| line.contains("Now I can answer with the exact API."))
        .expect("second reasoning row");
    let answer_row = lines
        .iter()
        .position(|line| line.contains("Use tokio::spawn for spawned tasks."))
        .expect("answer row");

    assert!(first_reasoning_row < tool_row);
    assert!(tool_row < second_reasoning_row);
    assert!(second_reasoning_row < answer_row);
    assert!(
        lines[second_reasoning_row.saturating_sub(1)]
            .trim()
            .is_empty()
            || lines[second_reasoning_row.saturating_sub(1)].trim() == "┃"
    );
}

#[test]
fn task_completion_summary_does_not_duplicate_streamed_assistant_text() {
    fn event(
        seq: u64,
        correlation_id: &str,
        payload: harness_core::event::EventV1,
    ) -> harness_core::event::EventEnvelopeV1 {
        harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt_duplicate_body_{seq:04}"),
            seq,
            run_id: "run_duplicate_body".to_string(),
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
                request_id: "req_duplicate_body".to_string(),
                text: "Say hello".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        2,
        "req_duplicate_body",
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_duplicate_body".to_string(),
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
                request_id: "req_duplicate_body".to_string(),
                delta: "Hello!".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        4,
        "req_duplicate_body",
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_duplicate_body".to_string(),
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
            task_id: "task_duplicate_body".to_string(),
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
    fn event(
        seq: u64,
        correlation_id: &str,
        payload: harness_core::event::EventV1,
    ) -> harness_core::event::EventEnvelopeV1 {
        harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt_tool_task_body_{seq:04}"),
            seq,
            run_id: "run_tool_task_body".to_string(),
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
                request_id: "req_tool_task_body".to_string(),
                text: "Inspect tokio docs".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        2,
        "req_tool_task_body",
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_tool_task_body".to_string(),
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
                tool_call_id: "tc_docs_tokio".to_string(),
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
                tool_call_id: "tc_docs_tokio".to_string(),
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
            task_id: "task_docs_tokio".to_string(),
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
fn task_row_hides_raw_task_result_payload_until_expanded() {
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
    let (title, icon, visual_style, _) =
        build_agent_spawn_tool_row(&tool_call, None, &mut detail_blocks, 0);
    assert_eq!(title, "review streaming states");
    assert_eq!(icon, Some("✓"));
    assert_eq!(visual_style, TranscriptToolCallVisualStyle::TaskInline);
    let detail_text = task_detail_blocks_text(&detail_blocks);
    assert!(detail_text.contains("└ 0 toolcalls · 4.0s"));
    assert!(!detail_text.contains("task_id:"));
    assert!(!detail_text.contains("request_id:"));
    assert!(!detail_text.contains("<task_result>"));
    assert!(!detail_text.contains(".sisyphus/evidence"));
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
    assert_eq!(title, "review queued background completion wakeups");
    assert_ne!(title, "Delegating...");
    assert!(icon.is_some_and(|value| value != "~"));

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
    assert_eq!(title, "General Task");
}

#[test]
fn background_output_tool_row_confirms_checked_child_result() {
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
    .expect("background_output checks stay visible when tool details are hidden");
    assert_eq!(
        visible_without_details.header.title,
        "Checked background output"
    );
}

#[test]
fn inline_metadata_collapse_removes_terminal_controls() {
    assert_eq!(
        collapse_inline_whitespace("researcher\u{1b}]0;owned\u{7} task\nsummary"),
        "researcher ]0;owned task summary"
    );
}

#[test]
fn task_row_profile_label_matches_harness_titlecase() {
    assert_eq!(subagent_profile_label(""), "General");
    assert_eq!(subagent_profile_label("general"), "General");
    assert_eq!(subagent_profile_label("foo-bar"), "Foo-Bar");
    assert_eq!(subagent_profile_label("foo_bar"), "Foo_bar");
    assert_eq!(subagent_profile_label("gPT worker"), "GPT Worker");
}
