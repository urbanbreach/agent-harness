use super::*;
use harness_core::event::UserMessageSubmittedEvent;

#[test]
fn transcript_test_activity_helper_has_required_defaults() {
    let entry = transcript_section_model_test_activity(
        "request-helper",
        ActivityStatus::Done,
        "assistant reply",
    );

    assert_eq!(entry.request_id, "request-helper");
    assert_eq!(entry.status, ActivityStatus::Done);
    assert_eq!(entry.transcript_text, "assistant reply");
    assert!(entry.tool_calls.is_empty());
    assert_eq!(entry.first_seq, 1);
    assert_eq!(entry.last_seq, 1);
}

#[test]
fn transcript_test_tool_call_helper_has_queued_defaults() {
    let tool_call = transcript_section_model_test_tool_call("tool-helper", "fs.read");

    assert_eq!(tool_call.tool_call_id, "tool-helper");
    assert_eq!(tool_call.tool_id, "fs.read");
    assert_eq!(tool_call.status, ToolCallDisplayStatus::Queued);
    assert!(tool_call.output_summary.is_none());
    assert!(tool_call.artifact_refs.is_empty());
}

#[test]
fn transcript_test_line_texts_joins_spans() {
    let texts = transcript_test_line_texts(vec![
        Line::from(vec![Span::raw("hello"), Span::raw(" world")]),
        Line::from(vec![Span::raw("again")]),
    ]);

    assert_eq!(texts, vec!["hello world", "again"]);
}

#[test]
fn transcript_section_model_preserves_activity_order() {
    exact_test_transcript_section_model_preserves_activity_order();
}

#[test]
fn transcript_section_model_keeps_nested_tool_and_error_blocks() {
    exact_test_transcript_section_model_keeps_nested_tool_and_error_blocks();
}

#[cfg(test)]
#[test]
fn transcript_reasoning_precedes_answer_and_tool_rows() {
    exact_test_transcript_reasoning_precedes_answer_and_tool_rows();
}

#[test]
fn transcript_follow_mode_uses_measured_surface_heights() {
    exact_test_transcript_follow_mode_uses_measured_surface_heights();
}

#[test]
fn failed_tool_cards_parse_legacy_error_copy() {
    super::failed_tool_cards_parse_legacy_error_copy();
}

#[test]
fn failed_tool_cards_normalize_lowercase_error_prefixes_and_tool_separators() {
    super::failed_tool_cards_normalize_lowercase_error_prefixes_and_tool_separators();
}

#[test]
fn denied_tool_cards_use_denied_subtitle() {
    super::denied_tool_cards_use_denied_subtitle();
}

#[test]
fn denied_tool_cards_keep_denied_subtitle_when_reason_contains_colon() {
    super::denied_tool_cards_keep_denied_subtitle_when_reason_contains_colon();
}

#[test]
fn generic_failed_tool_messages_do_not_split_arbitrary_prefixes() {
    super::generic_failed_tool_messages_do_not_split_arbitrary_prefixes();
}

#[test]
fn failed_tool_cards_fallback_when_error_details_are_missing() {
    super::failed_tool_cards_fallback_when_error_details_are_missing();
}

#[test]
fn transcript_pending_permission_stays_after_last_activity() {
    exact_test_transcript_pending_permission_stays_after_last_activity();
}

#[test]
fn transcript_layout_cache_invalidates_when_animation_frame_changes() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "request-streaming-cache".to_string(),
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status: ActivityStatus::Streaming,
        user_message: None,
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: String::new(),
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
    }]);
    app.selected_activity_index = 0;

    let initial_lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    assert!(initial_lines
        .iter()
        .any(|line| line.contains("⠋ Assistant · gpt-5.4-mini · active")));

    app.advance_transcript_animation_phase();

    let updated_lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    assert!(updated_lines
        .iter()
        .any(|line| line.contains("⠙ Assistant · gpt-5.4-mini · active")));
}

#[test]
fn transcript_layout_cache_invalidates_when_theme_changes() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "request-theme-cache".to_string(),
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status: ActivityStatus::Done,
        user_message: Some(harness_core::event::UserMessageSubmittedEvent {
            request_id: "request-theme-cache".to_string(),
            text: "theme-sensitive prompt".to_string(),
        }),
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: "reply".to_string(),
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
    }]);
    app.selected_activity_index = 0;

    let initial_layout = build_measured_transcript_layout_for_width(&app, app.theme(), 80);
    let initial_surface = initial_layout.sections[0].surfaces[0].surface;

    let mut alternate_theme = *app.theme();
    alternate_theme.surface.panel = Color::Rgb(0x22, 0x33, 0x44);
    app.set_theme_for_test(alternate_theme);

    let updated_layout = build_measured_transcript_layout_for_width(&app, app.theme(), 80);
    let updated_surface = updated_layout.sections[0].surfaces[0].surface;

    assert_ne!(initial_surface, updated_surface);
    assert_eq!(updated_surface, alternate_theme.surface.panel);
}

#[test]
fn pending_permission_sections_render_warning_turn_container() {
    let mut app = AppState::default();
    app.ingest_event(harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: "evt_pending_permission_group".to_string(),
        seq: 1,
        run_id: "run_pending_permission_group".to_string(),
        mono_ms: 0,
        ts: None,
        actor: harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Supervisor,
            None,
        ),
        correlation_id: Some("tool_call_pending_permission_group".to_string()),
        causation_id: None,
        stream_key: Some("tool_call_pending_permission_group".to_string()),
        payload: harness_core::event::EventV1::PermissionRequested(
            harness_core::event::PermissionRequestedEvent {
                permission_id: "perm_pending_permission_group".to_string(),
                kind: "edit_fs".to_string(),
                tool_call_id: Some("tool_call_pending_permission_group".to_string()),
                summary: "Apply hashline edit to demo.txt".to_string(),
                request_digest: "digest-perm".to_string(),
                timeout_ms: 30_000,
                default_decision: harness_core::event::PermissionDecision::Deny,
            },
        ),
    });

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert_eq!(lines[0], "Waiting for first turn…");
    assert!(lines
        .iter()
        .all(|line| !line.contains("permission checkpoint")
            && !line.contains("Apply hashline edit to demo.txt")));
}

#[test]
fn streaming_assistant_footer_uses_reserved_active_label() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "request-streaming-header".to_string(),
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status: ActivityStatus::Streaming,
        user_message: None,
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: String::new(),
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
    }]);
    app.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    let footer_row = lines
        .iter()
        .position(|line| line.contains("Assistant · gpt-5.4-mini · active"))
        .expect("streaming assistant footer row");
    assert_eq!(footer_row, 0);
    assert_eq!(lines[footer_row], "   ⠋ Assistant · gpt-5.4-mini · active");
}

#[test]
fn only_latest_turn_renders_footer_metadata() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![
        ActivityEntry {
            request_id: "request-old-footer".to_string(),
            profile_label: "default".to_string(),
            model_id: "gpt-old".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Done,
            user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                request_id: "request-old-footer".to_string(),
                text: "first".to_string(),
            }),
            user_timestamp: Some("2026-03-19T09:44:00Z".to_string()),
            request_data: None,
            thinking_text: String::new(),
            transcript_text: "first reply".to_string(),
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
        },
        ActivityEntry {
            request_id: "request-new-footer".to_string(),
            profile_label: "default".to_string(),
            model_id: "gpt-new".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Done,
            user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                request_id: "request-new-footer".to_string(),
                text: "second".to_string(),
            }),
            user_timestamp: Some("2026-03-19T09:45:00Z".to_string()),
            request_data: None,
            thinking_text: String::new(),
            transcript_text: "second reply".to_string(),
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 2,
            last_seq: 2,
            first_mono_ms: 2,
            last_mono_ms: 2,
        },
    ]);
    app.selected_activity_index = 1;
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "show timestamps".chars() {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(ch),
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("Assistant ·"))
            .count(),
        1
    );
    assert!(lines.iter().all(|line| !line.contains("gpt-old")));
    assert!(lines.iter().any(|line| line.contains("gpt-new")));
    assert!(lines.iter().all(|line| !line.contains("09:44")));
    assert!(lines.iter().any(|line| line.contains("09:45")));
}

#[test]
fn tool_only_turns_render_standalone_assistant_footer() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-tool-only-footer",
        ActivityStatus::Done,
        "",
    );
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-tool-only-footer".to_string(),
        tool_id: "fs.read".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"path":"src/ui.rs"}"#.to_string(),
        args_digest: "digest-tool-only-footer".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("24 lines read from src/ui.rs".to_string()),
        output_digest: Some("digest-tool-only-output".to_string()),
        output_json: None,
        truncated_output: Some("24 lines read from src/ui.rs".to_string()),
        edit: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: Some(2),
        permissions: Vec::new(),
        first_seq: 10,
        last_seq: 11,
        first_mono_ms: 10,
        last_mono_ms: 11,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert!(lines.iter().any(|line| line.contains("Read src/ui.rs")));
    assert!(lines.iter().any(|line| line.contains("Assistant ·")));
}

#[test]
fn completed_latest_turn_keeps_footer_after_streaming_finishes() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-footer-finish",
        ActivityStatus::Streaming,
        "completed reply",
    );
    entry.user_timestamp = Some("2026-03-19T09:45:00Z".to_string());
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.selected_activity_index = 0;
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "show timestamps".chars() {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(ch),
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));

    let streaming_lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    assert!(streaming_lines
        .iter()
        .any(|line| line.contains("Assistant · gpt-5.4-mini · active")));

    app.activities[0].status = ActivityStatus::Done;
    app.mark_transcript_dirty_for_test();

    let completed_lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert!(completed_lines
        .iter()
        .any(|line| line.contains("Assistant · gpt-5.4-mini")));
    assert!(completed_lines.iter().any(|line| line.contains("09:45")));
    assert!(completed_lines
        .iter()
        .all(|line| !line.contains("Assistant · gpt-5.4-mini · active")));
}

#[test]
fn latest_completed_footer_follows_rendered_assistant_parts() {
    fn event(
        seq: u64,
        correlation_id: &str,
        payload: harness_core::event::EventV1,
    ) -> harness_core::event::EventEnvelopeV1 {
        harness_core::event::EventEnvelopeV1 {
            schema_version: harness_core::event::SCHEMA_VERSION,
            event_id: format!("evt_footer_rendered_parts_{seq:04}"),
            seq,
            run_id: "run_footer_rendered_parts".to_string(),
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
        "req_footer_rendered_parts",
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_footer_rendered_parts".to_string(),
                provider_id: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "Keep footer visible".to_string(),
                request_digest: "digest-footer-rendered-parts".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(event(
        2,
        "req_footer_rendered_parts",
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_footer_rendered_parts".to_string(),
                delta: "assistant reply from ordered events".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        3,
        "req_footer_rendered_parts",
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_footer_rendered_parts".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-footer-rendered-parts-out".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));

    app.activities[0].transcript_text.clear();

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        96,
    ));

    assert!(lines
        .iter()
        .any(|line| line.contains("assistant reply from ordered events")));
    assert!(lines
        .iter()
        .any(|line| line.contains("Assistant · gpt-5.4-mini")));
}

#[test]
fn user_message_surface_keeps_timestamp_in_latest_footer_only() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "request-user-padding".to_string(),
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status: ActivityStatus::Done,
        user_message: Some(harness_core::event::UserMessageSubmittedEvent {
            request_id: "request-user-padding".to_string(),
            text: "hello".to_string(),
        }),
        user_timestamp: Some("2026-03-19T09:45:00Z".to_string()),
        request_data: None,
        thinking_text: String::new(),
        transcript_text: "reply".to_string(),
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
    }]);
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "show timestamps".chars() {
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char(ch),
            crossterm::event::KeyModifiers::NONE,
        ));
    }
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert!(!lines.iter().any(|line| line.contains("› You")));
    assert!(lines
        .iter()
        .all(|line| !(line.starts_with('┃') && line.contains("09:45"))));
    assert!(lines.iter().any(|line| line.contains("09:45")));
    assert!(lines
        .iter()
        .any(|line| line == "   ▪ Assistant · gpt-5.4-mini · 0ms · 09:45"));
    assert!(lines.iter().any(|line| line.contains("hello")));
    assert!(lines.iter().any(|line| line.contains("reply")));
}

#[test]
fn reasoning_summary_renders_as_nested_inset_block() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-reasoning-inset",
        ActivityStatus::Done,
        "answer",
    );
    entry.thinking_text = "Matching harness response spacing".to_string();
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert!(lines
        .iter()
        .any(|line| line.contains("Matching harness response spacing")));
    assert!(lines.iter().any(|line| line.is_empty()));
    assert!(lines.iter().any(|line| line.contains("answer")));
}

#[test]
fn fenced_code_blocks_render_frameless_with_highlighting() {
    let mut app = AppState::default();
    app.activities =
        std::collections::VecDeque::from(vec![transcript_section_model_test_activity(
            "request-code-panel",
            ActivityStatus::Done,
            "Before\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\nAfter",
        )]);
    app.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        100,
    ));

    let function_row = lines
        .iter()
        .find(|line| line.contains("fn main()"))
        .expect("function row");
    let println_row = lines
        .iter()
        .find(|line| line.contains("println!(\"hi\")"))
        .expect("println row");

    assert!(lines.iter().any(|line| line.contains("Before")));
    assert!(
        !function_row.contains('┃') && !println_row.contains('┃'),
        "fenced code should keep syntax-highlighted content in flow without a nested frame\n{lines:#?}"
    );
    assert!(lines.iter().any(|line| line.contains("After")));
}

#[test]
fn transcript_turn_sections_keep_exactly_one_blank_row_between_sections() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![
        transcript_section_model_test_activity("request-a", ActivityStatus::Done, "first"),
        transcript_section_model_test_activity("request-b", ActivityStatus::Done, "second"),
    ]);
    app.selected_activity_index = 1;

    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 80);

    assert_eq!(layout.sections.len(), 2);
    assert_eq!(layout.sections[0].leading_gap_height, 0);
    assert_eq!(layout.sections[1].leading_gap_height, 1);
}

#[test]
fn assistant_tool_surfaces_keep_same_trailing_gap_as_text_boxes() {
    let mut activity = transcript_section_model_test_activity(
        "request-shell-alignment",
        ActivityStatus::Done,
        "I’ll run a harmless shell command.",
    );
    activity.user_message = Some(UserMessageSubmittedEvent {
        request_id: "request-shell-alignment".to_string(),
        text: "test out some tools".to_string(),
    });

    let mut shell_call = transcript_section_model_test_tool_call("tc-shell-alignment", "bash");
    shell_call.args_summary = r#"{"command":"printf 'bash smoke test ok\n'","description":"Run harmless shell smoke test"}"#.to_string();
    shell_call.status = ToolCallDisplayStatus::Succeeded;
    shell_call.output_summary = Some("bash smoke test ok".to_string());
    activity.tool_calls.push(shell_call);

    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![activity]);
    app.selected_activity_index = 0;

    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 80);
    let surfaces = &layout.sections[0].surfaces;
    let user_surface = surfaces
        .iter()
        .find(|surface| surface.show_outer_rail)
        .expect("user surface");
    let tool_surface = surfaces
        .iter()
        .find(|surface| {
            transcript_test_line_texts(surface.lines.clone())
                .iter()
                .any(|line| line.contains("bash smoke test ok"))
        })
        .expect("assistant tool surface");
    let tool_lines = transcript_test_line_texts(tool_surface.lines.clone());
    let tool_interactions = tool_surface
        .interaction_rows
        .as_ref()
        .expect("tool surface interaction rows");

    assert_eq!(user_surface.width, 78);
    assert_eq!(tool_surface.width, user_surface.width);
    let title_row = tool_lines
        .iter()
        .position(|line| line.contains("# Run harmless shell smoke test"))
        .expect("title line");
    let command_row = tool_lines
        .iter()
        .position(|line| line.contains("$ printf 'bash smoke test ok"))
        .expect("command line");
    let output_row = tool_lines
        .iter()
        .enumerate()
        .find_map(|(index, line)| {
            (!line.contains("$ printf") && line.contains("bash smoke test ok")).then_some(index)
        })
        .expect("output line");
    let title_column = tool_lines[title_row]
        .find("# Run harmless shell smoke test")
        .expect("title column");
    let command_column = tool_lines[command_row]
        .find("$ printf 'bash smoke test ok")
        .expect("command column");
    let output_column = tool_lines[output_row]
        .find("bash smoke test ok")
        .expect("output column");
    assert_eq!(title_column, command_column);
    assert_eq!(output_column, command_column);
    assert_eq!(tool_interactions[title_row], None);
    assert_eq!(tool_interactions[command_row], None);
    assert_eq!(tool_interactions[output_row], None);
    assert!(
        tool_lines.iter().any(|line| line.starts_with(&format!(
            "{TRANSCRIPT_COMMAND_TOOL_INDENT}{HARNESS_SPLIT_RAIL_GLYPH}"
        )) && line.contains("bash smoke test ok")),
        "command card rail should align with transcript text box edge\n{tool_lines:#?}"
    );
    assert!(
        tool_surface
            .lines
            .iter()
            .all(|line| line.width() <= usize::from(tool_surface.width)),
        "tool card lines should be built for the same visual width as the rendered surface"
    );

    let area = Rect::new(0, 0, 100, 30);
    let snapshot = transcript_selection_debug_snapshot(&app, area)
        .expect("shell command card selection snapshot");
    let title_row = snapshot
        .rows
        .iter()
        .position(|line| line.contains("# Run harmless shell smoke test"))
        .expect("selectable title row");
    let output_row = snapshot
        .rows
        .iter()
        .position(|line| line.contains("bash smoke test ok"))
        .expect("selectable output row");
    let copied = transcript_selection_text(
        &app,
        area,
        TranscriptSelection {
            anchor: TranscriptSelectionCell {
                row: title_row,
                column: 0,
            },
            focus: TranscriptSelectionCell {
                row: output_row,
                column: usize::from(snapshot.viewport.width.saturating_sub(1)),
            },
        },
    )
    .expect("shell command card selection copies text");
    assert!(
        copied.starts_with("# Run harmless shell smoke test"),
        "copied shell card text should skip visual rail/padding: {copied:?}"
    );
    assert!(copied.contains("$ printf 'bash smoke test ok"));
    assert!(copied.contains("bash smoke test ok"));
    assert!(!copied.contains(HARNESS_SPLIT_RAIL_GLYPH));
}

#[test]
fn assistant_tool_surface_spacing_matches_shell_rhythm() {
    assert_eq!(
        transcript_surface_leading_gap(
            Some(TranscriptRenderSurfaceKind::AssistantBody),
            TranscriptRenderSurfaceKind::AssistantTool,
        ),
        0,
        "assistant text should hand off to tool rows without an extra blank terminal row"
    );
    assert_eq!(
        transcript_surface_leading_gap(
            Some(TranscriptRenderSurfaceKind::AssistantTool),
            TranscriptRenderSurfaceKind::AssistantBody,
        ),
        1,
        "tool rows should still leave a single section break before the next assistant text block"
    );
    assert_eq!(
        transcript_surface_leading_gap(
            Some(TranscriptRenderSurfaceKind::AssistantReasoning),
            TranscriptRenderSurfaceKind::AssistantBody,
        ),
        0,
        "reasoning-to-body spacing is carried by the nested blank row inside the body surface"
    );
    assert_eq!(
        transcript_surface_leading_gap(
            Some(TranscriptRenderSurfaceKind::AssistantTool),
            TranscriptRenderSurfaceKind::AssistantReasoning,
        ),
        0,
        "tool-to-reasoning spacing is carried by the reasoning block itself so the rendered gap stays single-row"
    );
}

#[test]
fn reasoning_to_answer_transition_uses_single_blank_row() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-reasoning-gap",
        ActivityStatus::Done,
        "answer",
    );
    entry.thinking_text = "reasoning".to_string();
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    let reasoning_row = lines
        .iter()
        .position(|line| line.contains("reasoning"))
        .expect("reasoning row");
    let answer_row = lines
        .iter()
        .position(|line| line.contains("answer"))
        .expect("answer row");

    assert_eq!(
        answer_row,
        reasoning_row + 2,
        "reasoning and answer should be separated by exactly one blank terminal row\n{lines:#?}"
    );
    assert!(lines[reasoning_row + 1].is_empty());
}

#[test]
fn streaming_assistant_footer_spinner_uses_deterministic_braille_frames() {
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "request-streaming-spinner".to_string(),
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status: ActivityStatus::Streaming,
        user_message: None,
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        transcript_text: String::new(),
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
    }]);
    app.selected_activity_index = 0;

    let first = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    app.advance_transcript_animation_phase();
    let second = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert_eq!(first[0], "   ⠋ Assistant · gpt-5.4-mini · active");
    assert_eq!(second[0], "   ⠙ Assistant · gpt-5.4-mini · active");
}

#[path = "ui_transcript_lifecycle_tests.rs"]
mod lifecycle_tests;
