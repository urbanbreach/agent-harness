use super::*;
use crate::UnwrapOrAbort;
use harness_core::event::UserMessageSubmittedEvent;

#[test]
fn active_thinking_user_prompt_surface_matches_the_native_grok_phase() {
    let theme = Theme::harness_chat();

    assert_eq!(
        ui_transcript_render::user_prompt_surface(&theme, false, true),
        Color::Rgb(0x26, 0x26, 0x26)
    );
    assert_eq!(
        ui_transcript_render::user_prompt_surface(&theme, false, false),
        theme.surface.card
    );
}

#[test]
fn transcript_test_activity_helper_has_required_defaults() {
    // arrange
    // act
    // assert
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
    // arrange
    // act
    // assert
    let tool_call = transcript_section_model_test_tool_call("tool-helper", "fs.read");

    assert_eq!(tool_call.tool_call_id, "tool-helper");
    assert_eq!(tool_call.tool_id, "fs.read");
    assert_eq!(tool_call.status, ToolCallDisplayStatus::Queued);
    assert!(tool_call.output_summary.is_none());
    assert!(tool_call.artifact_refs.is_empty());
}

#[test]
fn transcript_test_line_texts_joins_spans() {
    // arrange
    // act
    // assert
    let texts = transcript_test_line_texts(vec![
        Line::from(vec![Span::raw("hello"), Span::raw(" world")]),
        Line::from(vec![Span::raw("again")]),
    ]);

    assert_eq!(texts, vec!["hello world", "again"]);
}

#[test]
fn transcript_section_model_preserves_activity_order() {
    // arrange
    // act
    // assert
    exact_test_transcript_section_model_preserves_activity_order();
}

#[test]
fn transcript_section_model_keeps_nested_tool_and_error_blocks() {
    // arrange
    // act
    // assert
    exact_test_transcript_section_model_keeps_nested_tool_and_error_blocks();
}

#[cfg(test)]
#[test]
fn transcript_reasoning_precedes_answer_and_tool_rows() {
    // arrange
    // act
    // assert
    exact_test_transcript_reasoning_precedes_answer_and_tool_rows();
}

#[test]
fn transcript_follow_mode_uses_measured_surface_heights() {
    // arrange
    // act
    // assert
    exact_test_transcript_follow_mode_uses_measured_surface_heights();
}

#[test]
fn failed_tool_cards_parse_legacy_error_copy() {
    // arrange
    // act
    // assert
    super::failed_tool_cards_parse_legacy_error_copy();
}

#[test]
fn failed_tool_cards_normalize_lowercase_error_prefixes_and_tool_separators() {
    // arrange
    // act
    // assert
    super::failed_tool_cards_normalize_lowercase_error_prefixes_and_tool_separators();
}

#[test]
fn denied_tool_cards_use_denied_subtitle() {
    // arrange
    // act
    // assert
    super::denied_tool_cards_use_denied_subtitle();
}

#[test]
fn denied_tool_cards_keep_denied_subtitle_when_reason_contains_colon() {
    // arrange
    // act
    // assert
    super::denied_tool_cards_keep_denied_subtitle_when_reason_contains_colon();
}

#[test]
fn generic_failed_tool_messages_do_not_split_arbitrary_prefixes() {
    // arrange
    // act
    // assert
    super::generic_failed_tool_messages_do_not_split_arbitrary_prefixes();
}

#[test]
fn failed_tool_cards_fallback_when_error_details_are_missing() {
    // arrange
    // act
    // assert
    super::failed_tool_cards_fallback_when_error_details_are_missing();
}

#[test]
fn transcript_pending_permission_stays_after_last_activity() {
    // arrange
    // act
    // assert
    exact_test_transcript_pending_permission_stays_after_last_activity();
}

#[test]
fn transcript_layout_cache_invalidates_when_spinner_frame_changes() {
    // arrange
    // act
    // assert
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
        thinking_first_mono_ms: None,
        thinking_last_mono_ms: None,
        transcript_text: String::new(),
        first_delta_mono_ms: None,
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
        request_started_mono_ms: None,
        revision: 0,
    }]);
    app.transcript_view.selected_activity_index = 0;

    let initial_lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    assert!(initial_lines
        .iter()
        .all(|line| !line.contains("gpt-5.4-mini") && !line.contains('⠋')));

    for _ in 0..4 {
        app.advance_transcript_animation_phase();
    }

    let updated_lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    assert!(updated_lines
        .iter()
        .all(|line| !line.contains("gpt-5.4-mini") && !line.contains('⠙')));
}

#[test]
fn transcript_measure_cache_key_stable_across_animation_phase_changes() {
    // arrange
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "request-measure-stable".to_string(),
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status: ActivityStatus::Streaming,
        user_message: None,
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        thinking_first_mono_ms: None,
        thinking_last_mono_ms: None,
        transcript_text: String::new(),
        first_delta_mono_ms: None,
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
        request_started_mono_ms: None,
        revision: 0,
    }]);
    app.transcript_view.selected_activity_index = 0;
    // act
    let initial_key = app.transcript_measure_cache_key();
    app.advance_transcript_animation_phase();
    let updated_key = app.transcript_measure_cache_key();
    // assert
    assert_eq!(
        initial_key, updated_key,
        "measure cache key must not change when only animation phase changes"
    );
}

#[test]
fn transcript_layout_cache_does_not_rebuild_on_animation_phase_change() {
    // arrange
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "request-layout-rebuild".to_string(),
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status: ActivityStatus::Streaming,
        user_message: None,
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        thinking_first_mono_ms: None,
        thinking_last_mono_ms: None,
        transcript_text: String::new(),
        first_delta_mono_ms: None,
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
        request_started_mono_ms: None,
        revision: 0,
    }]);
    app.transcript_view.selected_activity_index = 0;
    // act
    AppState::reset_transcript_render_key_metrics_for_test();
    let _ = app.transcript_measure_cache_key();
    // assert
    let builds_after_first = AppState::transcript_render_key_build_count_for_test();
    assert_eq!(builds_after_first, 1, "first call should build once");

    for _ in 0..4 {
        app.advance_transcript_animation_phase();
    }
    let _ = app.transcript_measure_cache_key();
    let builds_after_animation = AppState::transcript_render_key_build_count_for_test();
    assert_eq!(
        builds_after_animation, 1,
        "animation phase change must not rebuild the measure cache key"
    );
}

#[test]
fn transcript_layout_cache_invalidates_when_theme_changes() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "request-theme-cache".to_string(),
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status: ActivityStatus::Done,
        user_message: Some(harness_core::event::UserMessageSubmittedEvent {
            request_id: "request-theme-cache".into(),
            text: "theme-sensitive prompt".to_string(),
        }),
        user_timestamp: None,
        request_data: None,
        thinking_text: String::new(),
        thinking_first_mono_ms: None,
        thinking_last_mono_ms: None,
        transcript_text: "reply".to_string(),
        first_delta_mono_ms: None,
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
        request_started_mono_ms: None,
        revision: 0,
    }]);
    app.transcript_view.selected_activity_index = 0;

    let initial_layout = build_measured_transcript_layout_for_width(&app, app.theme(), 80);
    let initial_surface = initial_layout.sections[0].surfaces[0].surface;

    let mut alternate_theme = *app.theme();
    alternate_theme.surface.card = Color::Rgb(0x22, 0x33, 0x44);
    alternate_theme.surface.selected_card = Color::Rgb(0x22, 0x33, 0x44);
    app.set_theme_for_test(alternate_theme);

    let updated_layout = build_measured_transcript_layout_for_width(&app, app.theme(), 80);
    let updated_surface = updated_layout.sections[0].surfaces[0].surface;

    assert_ne!(initial_surface, updated_surface);
    assert_eq!(updated_surface, alternate_theme.surface.card);
}

#[test]
fn pending_permission_sections_render_warning_turn_container() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    app.ingest_event(harness_core::event::EventEnvelopeV1 {
        schema_version: harness_core::event::SCHEMA_VERSION,
        event_id: "evt_pending_permission_group".to_string(),
        seq: 1,
        run_id: "run_pending_permission_group".into(),
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
                tool_call_id: Some("tool_call_pending_permission_group".into()),
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
fn streaming_assistant_status_is_not_rendered_in_scrollback() {
    // arrange
    // act
    // assert
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
        thinking_first_mono_ms: None,
        thinking_last_mono_ms: None,
        transcript_text: String::new(),
        first_delta_mono_ms: None,
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
        request_started_mono_ms: None,
        revision: 0,
    }]);
    app.transcript_view.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert!(
        lines
            .iter()
            .all(|line| !line.contains("gpt-5.4-mini") && !line.contains("Waiting for response")),
        "live status belongs in the prompt-adjacent row, not the scrollback\n{lines:#?}"
    );
}

#[test]
fn streaming_wait_status_is_not_rendered_in_scrollback() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-streaming-stop-width",
        ActivityStatus::Streaming,
        "",
    );
    entry.last_mono_ms = 5_300;
    entry.usage = Some(crate::app::ActivityUsage {
        prompt_tokens: 1_000,
        completion_tokens: 430,
        total_tokens: 1_430,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        117,
    ));

    assert!(
        lines
            .iter()
            .all(|line| !line.contains("Waiting for response") && !line.contains("[stop]")),
        "live status belongs in the prompt-adjacent row, not the scrollback\n{lines:#?}"
    );
}

#[test]
fn only_latest_turn_renders_footer_metadata() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![
        ActivityEntry {
            request_id: "request-old-footer".to_string(),
            profile_label: "default".to_string(),
            model_id: "gpt-old".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Done,
            user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                request_id: "request-old-footer".into(),
                text: "first".to_string(),
            }),
            user_timestamp: Some("2026-03-19T09:44:00Z".to_string()),
            request_data: None,
            thinking_text: String::new(),
            thinking_first_mono_ms: None,
            thinking_last_mono_ms: None,
            transcript_text: "first reply".to_string(),
            first_delta_mono_ms: None,
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
            request_started_mono_ms: None,
            revision: 0,
        },
        ActivityEntry {
            request_id: "request-new-footer".to_string(),
            profile_label: "default".to_string(),
            model_id: "gpt-new".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Done,
            user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                request_id: "request-new-footer".into(),
                text: "second".to_string(),
            }),
            user_timestamp: Some("2026-03-19T09:45:00Z".to_string()),
            request_data: None,
            thinking_text: String::new(),
            thinking_first_mono_ms: None,
            thinking_last_mono_ms: None,
            transcript_text: "second reply".to_string(),
            first_delta_mono_ms: None,
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 2,
            last_seq: 2,
            first_mono_ms: 2,
            last_mono_ms: 2,
            request_started_mono_ms: None,
            revision: 0,
        },
    ]);
    app.transcript_view.selected_activity_index = 1;
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

    assert!(lines.iter().all(|line| !line.contains("gpt-old")));
    let latest_footer_count = lines
        .iter()
        .filter(|line| {
            line.contains("gpt-new") || line.contains("Worked for") || line.contains('▪')
        })
        .count();
    assert_eq!(
        latest_footer_count, 1,
        "only the latest turn should render footer metadata\n{lines:#?}"
    );
    assert!(
        lines.iter().any(|line| {
            line.contains("gpt-new") || line.contains("Worked for") || line.contains('▪')
        }),
        "latest turn footer missing\n{lines:#?}"
    );
    assert!(lines.iter().all(|line| !line.contains("09:44")));
}

#[test]
fn tool_only_turns_render_standalone_assistant_footer() {
    // arrange
    // act
    // assert
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
    app.transcript_view.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert!(lines.iter().any(|line| line.contains("Read src/ui.rs")));
    assert!(
        lines.iter().any(|line| line.contains("Worked for")
            || line.contains("gpt-5.4-mini")
            || line.contains("▪")),
        "tool-only completed turns should still render an assistant footer\n{lines:#?}"
    );
}

#[test]
fn pending_question_turn_keeps_waiting_status_out_of_scrollback() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-question-waiting",
        ActivityStatus::Streaming,
        "",
    );
    entry.first_mono_ms = 0;
    entry.last_mono_ms = 900;
    // Waiting state: Thought for 0.1s (reasoning span) vs Waiting 0.9s (turn span).
    entry.thinking_text = "**plan**".to_string();
    entry.thinking_first_mono_ms = Some(100);
    entry.thinking_last_mono_ms = Some(200);
    entry.usage = Some(crate::app::ActivityUsage {
        prompt_tokens: 8_000,
        completion_tokens: 2_200,
        total_tokens: 10_200,
    });
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-question-waiting".to_string(),
        tool_id: "user.question".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: serde_json::json!({
            "questions": [{
                "question": "Which color?",
                "header": "Color",
                "options": [{"label": "Red", "description": "Choose red"}]
            }]
        })
        .to_string(),
        args_digest: "digest-question-waiting".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::PendingPermission,
        output_summary: None,
        output_digest: None,
        output_json: None,
        truncated_output: None,
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
    app.transcript_view.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    let rendered = lines.join("\n");

    assert!(
        rendered.contains("Ask Which color?"),
        "pending question tool row should use Ask title\n{rendered}"
    );
    assert!(
        !rendered.contains("Worked for"),
        "pending question turn must not show completed Worked for footer\n{rendered}"
    );
    assert!(
        !rendered.contains("Waiting on answers") && !rendered.contains("[stop]"),
        "live status belongs in the prompt-adjacent row, not the scrollback\n{rendered}"
    );
}

#[test]
fn waiting_on_answers_shows_thought_for_not_thinking() {
    // arrange
    // act
    // assert
    // Waiting state: Thought for (completed chrome) while Waiting on answers.
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-question-thought-for",
        ActivityStatus::Streaming,
        "",
    );
    entry.thinking_text = "**plan**".to_string();
    entry.first_mono_ms = 0;
    entry.last_mono_ms = 900;
    entry.thinking_first_mono_ms = Some(0);
    entry.thinking_last_mono_ms = Some(100);
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-question-thought".to_string(),
        tool_id: "user.question".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: serde_json::json!({
            "questions": [{
                "question": "Pick one",
                "header": "Choice",
                "options": [{"label": "A", "description": "Option A"}]
            }]
        })
        .to_string(),
        args_digest: "digest-question-thought".to_string(),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::PendingPermission,
        output_summary: None,
        output_digest: None,
        output_json: None,
        truncated_output: None,
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
    app.transcript_view.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    let rendered = lines.join("\n");

    assert!(
        rendered.contains("Thought for 0.1s"),
        "waiting-on-answers Thought must pack reasoning-only 0.1s duration\n{rendered}"
    );
    assert!(
        !rendered.contains("Thinking"),
        "waiting-on-answers must not keep streaming Thinking label\n{rendered}"
    );
    assert!(
        !rendered.contains("Waiting on answers") && !rendered.contains("[stop]"),
        "live status belongs in the prompt-adjacent row, not the scrollback\n{rendered}"
    );
}

#[test]
fn completed_latest_turn_keeps_footer_after_streaming_finishes() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-footer-finish",
        ActivityStatus::Streaming,
        "completed reply",
    );
    entry.user_timestamp = Some("2026-03-19T09:45:00Z".to_string());
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;
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
        .all(|line| !line.contains("gpt-5.4-mini")));

    app.activities[0].status = ActivityStatus::Done;
    app.mark_transcript_dirty_for_test();

    let completed_lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert!(
        completed_lines.iter().any(|line| {
            line.contains("Worked for") || line.contains("gpt-5.4-mini") || line.contains('▪')
        }),
        "completed turns should keep a footer (Worked for / model / marker)\n{completed_lines:#?}"
    );
    assert!(completed_lines
        .iter()
        .all(|line| !line.contains("⠋ Assistant")));
}

#[test]
fn latest_completed_footer_follows_rendered_assistant_parts() {
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
            event_id: format!("evt_footer_rendered_parts_{seq:04}"),
            seq,
            run_id: "run_footer_rendered_parts".into(),
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
                request_id: "req_footer_rendered_parts".into(),
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
                request_id: "req_footer_rendered_parts".into(),
                delta: "assistant reply from ordered events".to_string(),
            },
        ),
    ));
    app.ingest_event(event(
        3,
        "req_footer_rendered_parts",
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_footer_rendered_parts".into(),
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
    assert!(
        lines.iter().any(|line| {
            line.contains("Worked for") || line.contains("gpt-5.4-mini") || line.contains('▪')
        }),
        "completed event-projected turns should keep a footer\n{lines:#?}"
    );
}

#[test]
fn user_message_surface_keeps_timestamp_in_latest_footer_only() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "request-user-padding".to_string(),
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status: ActivityStatus::Done,
        user_message: Some(harness_core::event::UserMessageSubmittedEvent {
            request_id: "request-user-padding".into(),
            text: "hello".to_string(),
        }),
        user_timestamp: Some("2026-03-19T09:45:00Z".to_string()),
        request_data: None,
        thinking_text: String::new(),
        thinking_first_mono_ms: None,
        thinking_last_mono_ms: None,
        transcript_text: "reply".to_string(),
        first_delta_mono_ms: None,
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
        request_started_mono_ms: None,
        revision: 0,
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

    assert!(!lines
        .iter()
        .any(|line| line.contains("❯ You") || line.contains("› You")));
    assert!(lines
        .iter()
        .all(|line| !(line.starts_with('┃') && line.contains("09:45"))));
    assert!(
        lines.iter().any(|line| {
            line.contains("Worked for") || line.contains("gpt-5.4-mini") || line.contains('▪')
        }),
        "latest completed turn should keep a footer\n{lines:#?}"
    );
    assert!(lines.iter().any(|line| line.contains("hello")));
    assert!(lines.iter().any(|line| line.contains("reply")));
}

#[test]
fn user_row_wall_clock_right_aligned_matches_freeze_geometry() {
    // arrange
    // act
    // assert
    // Given: a completed turn with user message + ISO user_timestamp (freeze run1-stream-probe)
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "request-user-wall-clock".to_string(),
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status: ActivityStatus::Error,
        user_message: Some(harness_core::event::UserMessageSubmittedEvent {
            request_id: "request-user-wall-clock".into(),
            text: "ping".to_string(),
        }),
        user_timestamp: Some("2026-03-19T09:33:00Z".to_string()),
        request_data: None,
        thinking_text: String::new(),
        thinking_first_mono_ms: None,
        thinking_last_mono_ms: None,
        transcript_text: String::new(),
        first_delta_mono_ms: None,
        usage: None,
        cache_usage: None,
        error_message: Some("API error".to_string()),
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
        request_started_mono_ms: None,
        revision: 0,
    }]);
    app.transcript_view.selected_activity_index = 0;

    // When: render transcript at a wide width
    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        120,
    ));

    // Then: user marker row carries freeze-style wall clock (right-aligned on same line)
    let user_row = lines
        .iter()
        .find(|line| line.contains('❯') && line.contains("ping"))
        .unwrap_or_else(|| panic!("missing user marker row; lines={lines:?}"));
    assert!(
        user_row.contains("9:33 AM"),
        "user row must include freeze-style wall clock; got {user_row:?}"
    );
    let clock_idx = user_row.find("9:33 AM").expect("clock");
    let ping_idx = user_row.find("ping").expect("ping");
    assert!(
        ping_idx < clock_idx,
        "wall clock must sit to the right of the user text; got {user_row:?}"
    );
}

#[test]
fn user_row_packs_all_names_with_wall_clock_at_scroll_geometry() {
    // arrange
    // act
    // assert
    // Given: SCROLL freeze user prompt + wall clock (run1-scroll-proxy-v3)
    let prompt = "List every file in the current directory using a tool, then write a numbered inventory of all names one per line.";
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
        request_id: "request-scroll-user-pack".to_string(),
        profile_label: "default".to_string(),
        model_id: "gpt-5.4-mini".to_string(),
        provider_id: "openai".to_string(),
        status: ActivityStatus::Done,
        user_message: Some(harness_core::event::UserMessageSubmittedEvent {
            request_id: "request-scroll-user-pack".into(),
            text: prompt.to_string(),
        }),
        user_timestamp: Some("2026-03-19T05:54:00Z".to_string()),
        request_data: None,
        thinking_text: String::new(),
        thinking_first_mono_ms: None,
        thinking_last_mono_ms: None,
        transcript_text: String::new(),
        first_delta_mono_ms: None,
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
        request_started_mono_ms: None,
        revision: 0,
    }]);
    app.transcript_view.selected_activity_index = 0;

    // When: measure at scrollbar-reduced scroll width (120 shell − dual gutter 10 − track 1)
    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 109);
    let user_surface = layout
        .sections
        .iter()
        .flat_map(|section| section.surfaces.iter())
        .find(|surface| {
            transcript_test_line_texts(surface.lines.clone())
                .iter()
                .any(|line| line.contains('❯') && line.contains("List every"))
        })
        .expect("user surface");
    let lines = transcript_test_line_texts(user_surface.lines.clone());
    let first_content = lines
        .iter()
        .find(|line| line.contains('❯'))
        .expect("user marker row");

    // Then: freeze packs the wall clock on the first user row and wraps overflow
    // to the next row. With the 3-char transcript content indent, "all names" no
    // longer fits on the first row, so the clock stays on row 1 and "names" wraps.
    assert!(
        first_content.contains("all") && first_content.contains("5:54 AM"),
        "SCROLL freeze packs 'all' + wall clock on first user row; got {first_content:?}\nlines={lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("names one per")),
        "overflow after 'all' must wrap to the next row; got {first_content:?}\nlines={lines:?}"
    );
}

#[test]
fn reasoning_summary_renders_as_nested_inset_block() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-reasoning-inset",
        ActivityStatus::Streaming,
        "answer",
    );
    entry.thinking_text = "Matching harness response spacing".to_string();
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

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
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    app.activities =
        std::collections::VecDeque::from(vec![transcript_section_model_test_activity(
            "request-code-panel",
            ActivityStatus::Done,
            "Before\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\nAfter",
        )]);
    app.transcript_view.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        100,
    ));

    let function_row = lines
        .iter()
        .find(|line| line.contains("fn main()"))
        .unwrap_or_abort();
    let println_row = lines
        .iter()
        .find(|line| line.contains("println!(\"hi\")"))
        .unwrap_or_abort();

    assert!(lines.iter().any(|line| line.contains("Before")));
    assert!(
        !function_row.contains('┃') && !println_row.contains('┃'),
        "fenced code should keep syntax-highlighted content in flow without a nested frame\n{lines:#?}"
    );
    assert!(lines.iter().any(|line| line.contains("After")));
}

#[test]
fn consecutive_assistant_bodies_reuse_the_existing_trailing_blank_row() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![
        transcript_section_model_test_activity("request-a", ActivityStatus::Done, "first"),
        transcript_section_model_test_activity("request-b", ActivityStatus::Done, "second"),
    ]);
    app.transcript_view.selected_activity_index = 1;

    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 80);

    assert_eq!(layout.sections.len(), 2);
    assert_eq!(layout.sections[0].leading_gap_height, 0);
    assert_eq!(
        layout.sections[1].leading_gap_height, 0,
        "the first body already contributes the single Grok separator row"
    );
}

#[test]
fn markdown_headings_get_blank_row_before_when_preceded_by_text() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    app.activities =
        std::collections::VecDeque::from(vec![transcript_section_model_test_activity(
            "request-heading-gap",
            ActivityStatus::Done,
            "Some intro text\n# Heading",
        )]);
    app.transcript_view.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    let intro_row = lines
        .iter()
        .position(|line| line.contains("Some intro text"))
        .unwrap_or_abort();
    let heading_row = lines
        .iter()
        .position(|line| line.contains("Heading"))
        .unwrap_or_abort();

    assert_eq!(
        heading_row,
        intro_row + 2,
        "heading should have exactly 1 blank row between it and the preceding text\n{lines:#?}"
    );
    assert!(
        lines[intro_row + 1].trim().is_empty(),
        "row between intro and heading should be blank\n{lines:#?}"
    );
}

#[test]
fn markdown_paragraphs_get_trailing_blank_row_for_margin_bottom() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![
        transcript_section_model_test_activity(
            "request-para-margin-first",
            ActivityStatus::Done,
            "First paragraph",
        ),
        transcript_section_model_test_activity(
            "request-para-margin-second",
            ActivityStatus::Done,
            "Second paragraph",
        ),
    ]);
    app.transcript_view.selected_activity_index = 1;

    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 80);
    let body_surface = layout.sections[0]
        .surfaces
        .iter()
        .find(|surface| {
            transcript_test_line_texts(surface.lines.clone())
                .iter()
                .any(|line| line.contains("First paragraph"))
        })
        .unwrap_or_abort();
    let body_lines = transcript_test_line_texts(body_surface.lines.clone());

    assert!(
        body_lines.last().is_some_and(|line| line.trim().is_empty()),
        "body surface should end with a blank row for paragraph margin-bottom\n{body_lines:#?}"
    );
}

#[test]
fn code_block_bottom_margin_is_two_blank_rows() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![
        transcript_section_model_test_activity(
            "request-code-margin",
            ActivityStatus::Done,
            "```rust\nfn main() {}\n```",
        ),
        transcript_section_model_test_activity(
            "request-code-margin-second",
            ActivityStatus::Done,
            "second",
        ),
    ]);
    app.transcript_view.selected_activity_index = 1;

    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 100);
    let body_surface = layout.sections[0]
        .surfaces
        .iter()
        .find(|surface| {
            transcript_test_line_texts(surface.lines.clone())
                .iter()
                .any(|line| line.contains("fn main()"))
        })
        .unwrap_or_abort();
    let body_lines = transcript_test_line_texts(body_surface.lines.clone());

    let code_end_row = body_lines
        .iter()
        .rposition(|line| line.contains("fn main()"))
        .unwrap_or_abort();

    assert!(
        body_lines
            .get(code_end_row + 1)
            .is_some_and(|line| line.trim().is_empty()),
        "first row after code block should be blank (margin-bottom)\n{body_lines:#?}"
    );
    assert!(
        body_lines
            .get(code_end_row + 2)
            .is_some_and(|line| line.trim().is_empty()),
        "second row after code block should be blank (24px margin-bottom)\n{body_lines:#?}"
    );
}

#[test]
fn assistant_tool_surfaces_keep_same_trailing_gap_as_text_boxes() {
    // arrange
    // act
    // assert
    let mut activity = transcript_section_model_test_activity(
        "request-shell-alignment",
        ActivityStatus::Done,
        "I’ll run a harmless shell command.",
    );
    activity.user_message = Some(UserMessageSubmittedEvent {
        request_id: "request-shell-alignment".into(),
        text: "test out some tools".to_string(),
    });

    let mut shell_call = transcript_section_model_test_tool_call("tc-shell-alignment", "bash");
    shell_call.args_summary = r#"{"command":"printf 'bash smoke test ok\n'","description":"Run harmless shell smoke test"}"#.to_string();
    shell_call.status = ToolCallDisplayStatus::Succeeded;
    shell_call.output_summary = Some("bash smoke test ok".to_string());
    activity.tool_calls.push(shell_call);

    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(vec![activity]);
    app.transcript_view.selected_activity_index = 0;
    app.toggle_tool_output_for_test("tc-shell-alignment");

    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 80);
    let surfaces = &layout.sections[0].surfaces;
    let tool_surface = surfaces
        .iter()
        .find(|surface| {
            transcript_test_line_texts(surface.lines.clone())
                .iter()
                .any(|line| line.contains("bash smoke test ok"))
        })
        .unwrap_or_abort();
    let tool_lines = transcript_test_line_texts(tool_surface.lines.clone());
    let tool_interactions = tool_surface.interaction_rows.as_ref().unwrap_or_abort();

    assert_eq!(tool_surface.width, 78);
    let command_row = tool_lines
        .iter()
        .position(|line| line.contains("$ printf 'bash smoke test ok"))
        .unwrap_or_abort();
    let output_row = tool_lines
        .iter()
        .enumerate()
        .find_map(|(index, line)| {
            (!line.contains("$ printf") && line.contains("bash smoke test ok")).then_some(index)
        })
        .unwrap_or_abort();
    let command_column = tool_lines[command_row]
        .find("$ printf 'bash smoke test ok")
        .unwrap_or_abort();
    let output_column = tool_lines[output_row]
        .find("bash smoke test ok")
        .unwrap_or_abort();
    assert_eq!(output_column, command_column);
    assert!(
        tool_interactions[command_row].is_some(),
        "the command header should remain the disclosure hit target"
    );
    assert_eq!(tool_interactions[output_row], None);
    assert!(
        tool_lines
            .iter()
            .any(|line| line.contains('◈') || line.contains('◆')),
        "harness shell blocks should render the flat tool header (◈ completed / ◆ active)\n{tool_lines:#?}"
    );
    assert!(
        tool_surface
            .lines
            .iter()
            .all(|line| line.width() <= usize::from(tool_surface.width)),
        "tool card lines should be built for the same visual width as the rendered surface"
    );

    let area = Rect::new(0, 0, 100, 30);
    let snapshot = transcript_selection_debug_snapshot(&app, area).unwrap_or_abort();
    let command_row = snapshot
        .rows
        .iter()
        .position(|line| line.contains("$ printf 'bash smoke test ok"))
        .unwrap_or_abort();
    let output_row = snapshot
        .rows
        .iter()
        .position(|line| line.contains("bash smoke test ok"))
        .unwrap_or_abort();
    let copied = transcript_selection_text(
        &app,
        area,
        TranscriptSelection {
            anchor: TranscriptSelectionCell {
                row: command_row,
                column: 0,
            },
            focus: TranscriptSelectionCell {
                row: output_row,
                column: usize::from(snapshot.viewport.width.saturating_sub(1)),
            },
        },
    )
    .unwrap_or_abort();
    assert!(
        copied.contains("$ printf 'bash smoke test ok"),
        "copied shell card text should contain the command without rail: {copied:?}"
    );
    assert!(copied.contains("$ printf 'bash smoke test ok"));
    assert!(copied.contains("bash smoke test ok"));
    assert!(!copied.contains('┃'));
}

#[test]
fn assistant_tool_surface_spacing_matches_shell_rhythm() {
    // arrange
    // act
    // assert
    assert_eq!(
        transcript_surface_leading_gap(
            Some(TranscriptRenderSurfaceKind::AssistantBody),
            TranscriptRenderSurfaceKind::AssistantTool,
        ),
        0,
        "assistant body padding already carries the reference section break before tool rows"
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
        1,
        "reasoning-to-body surface gap should be 1 to match the 12px assistant-content gap"
    );
    assert_eq!(
        transcript_surface_leading_gap(
            Some(TranscriptRenderSurfaceKind::AssistantTool),
            TranscriptRenderSurfaceKind::AssistantReasoning,
        ),
        0,
        "tool-to-reasoning spacing is carried by the reasoning block itself so the rendered gap stays single-row"
    );
    assert_eq!(
        transcript_surface_leading_gap(
            Some(TranscriptRenderSurfaceKind::AssistantReasoning),
            TranscriptRenderSurfaceKind::AssistantTool,
        ),
        0,
        "Reference question state: Thought then Ask are adjacent with no blank between"
    );
}

#[test]
fn reasoning_to_answer_transition_uses_one_blank_row() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-reasoning-gap",
        ActivityStatus::Streaming,
        "answer",
    );
    entry.thinking_text = "reasoning".to_string();
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    let reasoning_row = lines
        .iter()
        .position(|line| line.contains("reasoning"))
        .unwrap_or_abort();
    let answer_row = lines
        .iter()
        .position(|line| line.contains("answer"))
        .unwrap_or_abort();

    assert_eq!(
        answer_row,
        reasoning_row + 2,
        "reasoning and answer should be separated by one blank row\n{lines:#?}"
    );
    assert!(lines[reasoning_row + 1].is_empty());
}

#[test]
fn mixed_prose_reasoning_and_tools_never_insert_multiple_blank_rows() {
    let tool = |tool_call_id: &str, status: ToolCallDisplayStatus| TranscriptToolCallSection {
        tool_call_id: tool_call_id.to_string(),
        child_session_id: None,
        hovered_target: None,
        header: TranscriptToolCallHeader {
            tool_id: "edit".to_string(),
            title: "Patch".to_string(),
            subtitle: None,
            path_metadata: None,
            icon: None,
            status,
            visual_style: TranscriptToolCallVisualStyle::Inline,
            struck_out: false,
            disclosure_state: Some(TranscriptToolCallDisclosureState::Collapsed),
        },
        detail_blocks: Vec::new(),
        details_collapsed_by_default: true,
        details_preview_visible: false,
        animation_phase: 0,
        expanded: false,
    };
    let failed = tool("patch-failed", ToolCallDisplayStatus::Failed);
    let succeeded = tool("patch-succeeded", ToolCallDisplayStatus::Succeeded);
    let turn = TranscriptTurnSection {
        request_id: "request-mixed-density".to_string(),
        user_message: None,
        show_footer: true,
        footer_timestamp: Some("9:42 PM".to_string()),
        animation_phase: 0,
        header: TranscriptTurnHeader {
            status: ActivityStatus::Done,
            is_selected: true,
            provider_request_open: false,
            profile_label: "default".to_string(),
            model_id: "model".to_string(),
            duration_ms: Some(15_900),
            thinking_duration_ms: Some(6_000),
            responding_duration_ms: None,
            total_tokens: None,
            retry: None,
            retry_elapsed_ms: None,
        },
        body_blocks: Vec::new(),
        tool_calls: vec![failed.clone(), succeeded.clone()],
        thinking: None,
        error: None,
        assistant_parts: vec![
            TranscriptAssistantPart::Reasoning(TranscriptLabeledTextSection {
                label: THINKING_TRACE_LABEL,
                text: "Planning sequential file operations".to_string(),
            }),
            TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(
                "I’ll create a temporary test file.".to_string(),
            )),
            TranscriptAssistantPart::ToolCall(Box::new(failed)),
            TranscriptAssistantPart::Reasoning(TranscriptLabeledTextSection {
                label: THINKING_TRACE_LABEL,
                text: "Retrying with a corrected patch".to_string(),
            }),
            TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(
                "The failed patch is clear; I’ll retry with corrected input.".to_string(),
            )),
            TranscriptAssistantPart::ToolCall(Box::new(succeeded)),
            TranscriptAssistantPart::Body(TranscriptBodyBlock::RichText(
                "Created, edited, and deleted the file successfully.".to_string(),
            )),
        ],
    };
    let surfaces = build_transcript_render_surfaces(
        &turn,
        &Theme::default(),
        120,
        ratatui::style::Color::Reset,
    );
    let lines = render_transcript_surface_lines(&surfaces)
        .into_iter()
        .map(|line| {
            line.spans
                .into_iter()
                .map(|span| span.content.into_owned())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    let maximum_blank_run = lines
        .split(|line| !line.trim().is_empty())
        .map(<[String]>::len)
        .max()
        .unwrap_or(0);

    assert!(
        maximum_blank_run <= 1,
        "mixed response/tool transcripts must stay densely packed\n{lines:#?}"
    );
}

#[test]
fn streaming_reasoning_header_renders_spinner_and_thinking_label() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-streaming-reasoning",
        ActivityStatus::Streaming,
        "",
    );
    entry.thinking_text = "analyzing the problem".to_string();
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    let rendered = lines.join("\n");
    assert!(
        rendered.contains("◆ Thinking…") && !rendered.contains('⠋'),
        "streaming reasoning should use Grok's animated-bullet Thinking header\n{rendered}"
    );
    assert!(
        rendered.contains("analyzing the problem"),
        "body text should still render\n{rendered}"
    );
}

#[test]
fn streaming_reasoning_keeps_spinner_while_body_text_arrives() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-streaming-reasoning-then-body",
        ActivityStatus::Streaming,
        "Here is my answer",
    );
    entry.thinking_text = "analyzing the problem".to_string();
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    let rendered = lines.join("\n");
    assert!(
        rendered.contains("◆ Thinking…") && !rendered.contains('⠋'),
        "reasoning animation should remain live while body text arrives mid-turn\n{rendered}"
    );
    assert!(
        rendered.contains("Thinking"),
        "reasoning header should show 'Thinking' once body text arrives\n{rendered}"
    );
    assert!(
        rendered.contains("analyzing the problem"),
        "body text should still render\n{rendered}"
    );
    assert!(
        rendered.contains("Here is my answer"),
        "body text should still render\n{rendered}"
    );
}

#[test]
fn streaming_reasoning_keeps_spinner_while_tool_call_arrives() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-streaming-reasoning-then-tool",
        ActivityStatus::Streaming,
        "",
    );
    entry.thinking_text = "planning the approach".to_string();
    entry.tool_calls.push(crate::app::ToolCallEntry {
        tool_call_id: "call-1".to_string(),
        tool_id: "shell.run".to_string(),
        canonical_tool_id: None,
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: r#"{"cmd":"ls"}"#.to_string(),
        args_digest: "digest".to_string(),
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
        first_seq: 2,
        last_seq: 2,
        first_mono_ms: 2,
        last_mono_ms: 2,
        first_timestamp: None,
        last_timestamp: None,
    });
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    let rendered = lines.join("\n");
    assert!(
        rendered.contains("◆ Thinking…") && !rendered.contains('⠋'),
        "reasoning animation should remain live while a tool call runs mid-turn\n{rendered}"
    );
    assert!(
        rendered.contains("Thinking"),
        "reasoning header should show 'Thinking' once a tool call arrives\n{rendered}"
    );
    assert!(
        rendered.contains("planning the approach"),
        "reasoning body text should still render\n{rendered}"
    );
}

#[test]
fn streaming_reasoning_title_stays_in_the_body_below_thinking_header() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-streaming-reasoning-title",
        ActivityStatus::Streaming,
        "",
    );
    entry.thinking_text = "**Planning approach**\n\nDetailed analysis".to_string();
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    let rendered = lines.join("\n");
    assert!(
        rendered.contains("◆ Thinking…") && rendered.contains("Planning approach"),
        "streaming reasoning title should remain in the body below Grok's fixed header\n{rendered}"
    );
    assert!(
        rendered.contains("Detailed analysis"),
        "reasoning detail should render below the title\n{rendered}"
    );
}

#[test]
fn completed_reasoning_header_renders_thinking_with_title() {
    // arrange
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-completed-reasoning",
        ActivityStatus::Done,
        "Final answer",
    );
    entry.thinking_text = "**Review**\n\nbody text".to_string();
    entry.last_mono_ms = 1501;
    // Reference packing: Thought for uses reasoning mono span, not full turn duration.
    entry.thinking_first_mono_ms = Some(0);
    entry.thinking_last_mono_ms = Some(1500);
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    // act
    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    let rendered = lines.join("\n");

    // assert
    assert!(
        rendered.contains("Thought for 1.5s"),
        "completed reasoning should show Thought for duration\n{rendered}"
    );
    assert!(
        !rendered.contains("Thinking · Review"),
        "completed reasoning should not keep the streaming Thinking · title form\n{rendered}"
    );
    assert!(
        !rendered.contains("body text"),
        "completed reasoning should auto-collapse to its Thought header\n{rendered}"
    );
    assert!(
        rendered.contains("Worked for 1.5s."),
        "completed turn footer should show Worked for duration\n{rendered}"
    );
}

#[test]
fn completed_reasoning_header_without_title_renders_thinking() {
    // arrange
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-completed-reasoning-no-title",
        ActivityStatus::Done,
        "Final answer",
    );
    entry.thinking_text = "simple reasoning".to_string();
    entry.last_mono_ms = 1501;
    // Reference packing: Thought for uses reasoning mono span, not full turn duration.
    entry.thinking_first_mono_ms = Some(0);
    entry.thinking_last_mono_ms = Some(1500);
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    // act
    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    let rendered = lines.join("\n");

    // assert
    assert!(
        rendered.contains("Thought for 1.5s"),
        "completed reasoning should show Thought for duration when no title\n{rendered}"
    );
    assert!(
        !rendered.contains("Thinking"),
        "completed reasoning should not keep the streaming Thinking label\n{rendered}"
    );
    assert!(
        !rendered.contains("simple reasoning"),
        "completed reasoning should auto-collapse to its Thought header\n{rendered}"
    );
    assert!(
        rendered.contains("Worked for 1.5s."),
        "completed turn footer should show Worked for duration\n{rendered}"
    );
}

#[test]
fn completed_turn_without_thinking_text_still_renders_thought_for() {
    // arrange
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-completed-empty-thinking",
        ActivityStatus::Done,
        "Final answer only",
    );
    entry.thinking_text.clear();
    entry.last_mono_ms = 2500;
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    // act
    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    let rendered = lines.join("\n");

    // assert — pinned reference freeze does NOT show Thought for completed
    // turns without reasoning deltas; only Worked for + body text.
    assert!(
        !rendered.contains("Thought for"),
        "completed turns without reasoning must not render Thought for\n{rendered}"
    );
    assert!(
        !rendered.contains("Thinking"),
        "completed empty reasoning must not use streaming Thinking label\n{rendered}"
    );
    assert!(
        rendered.contains("Worked for 2.5s."),
        "completed turn footer should still show Worked for duration\n{rendered}"
    );
    assert!(
        rendered.contains("Final answer only"),
        "body text should still render\n{rendered}"
    );
}

#[test]
fn failed_turn_without_thinking_text_omits_thought_for() {
    // arrange
    // act
    // assert
    // Given: failed turn with no reasoning (reference run1-stream-probe failure state)
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-failed-no-thinking",
        ActivityStatus::Error,
        "",
    );
    entry.thinking_text.clear();
    entry.transcript_text.clear();
    entry.error_message = Some(
        "API error (status 400 Bad Request): invalid-argument: Incorrect API key provided.".into(),
    );
    entry.last_mono_ms = 300;
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    // When
    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    let rendered = lines.join("\n");

    // Then: fail chrome is flat Retry failed / Turn failed — no empty Thought for
    assert!(
        !rendered.contains("Thought for"),
        "failed turns without reasoning must omit Thought for (reference failure state)\n{rendered}"
    );
    assert!(
        rendered.contains("Retry failed") || rendered.contains("API error"),
        "failed turn must still render error chrome\n{rendered}"
    );
}

#[test]
fn reasoning_header_suppresses_empty_redacted_reasoning() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-redacted-only",
        ActivityStatus::Done,
        "answer",
    );
    entry.thinking_text = "[REDACTED]".to_string();
    entry.last_mono_ms = 1500;
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    let rendered = lines.join("\n");
    assert!(
        !rendered.contains("Thinking"),
        "redacted-only reasoning must not use streaming Thinking label\n{rendered}"
    );
    assert!(
        rendered.contains("Thought for"),
        "completed redacted-only turns still show Thought for chrome\n{rendered}"
    );
}

#[test]
fn streaming_assistant_spinner_is_not_rendered_in_scrollback() {
    // arrange
    // act
    // assert
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
        thinking_first_mono_ms: None,
        thinking_last_mono_ms: None,
        transcript_text: String::new(),
        first_delta_mono_ms: None,
        usage: None,
        cache_usage: None,
        error_message: None,
        permissions: Vec::new(),
        tool_calls: Vec::new(),
        first_seq: 1,
        last_seq: 1,
        first_mono_ms: 1,
        last_mono_ms: 1,
        request_started_mono_ms: None,
        revision: 0,
    }]);
    app.transcript_view.selected_activity_index = 0;

    let first = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));
    for _ in 0..4 {
        app.advance_transcript_animation_phase();
    }
    let second = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    assert!(
        first
            .iter()
            .chain(second.iter())
            .all(|line| !line.contains("gpt-5.4-mini")
                && !line.contains('⠋')
                && !line.contains('⠙')),
        "the live spinner belongs in the prompt-adjacent row\nfirst={first:#?}\nsecond={second:#?}"
    );
}

#[path = "ui_transcript_lifecycle_tests.rs"]
mod lifecycle_tests;

#[test]
fn transcript_measurement_wrap_correctness_across_widths_and_styles() {
    // arrange
    let patterns = [
        "short",
        "a medium length reply that should wrap at narrow widths",
        "漢字🙂漢字🙂漢字🙂 wide glyph text that needs careful wrapping across columns",
        "word-with-dashes-and-underscores_that_must_not_break_mid_token",
        "multiple\nlines\nin\none\nreply\nthat\nshould\npreserve\nhard\nbreaks",
        "a single very long word without any spaces supercalifragilisticexpialidocious1234567890",
        "trailing   spaces   and   tabs\t\t\tthat   collapse",
        "",
    ];

    let widths = [20u16, 40, 60, 80, 100, 120];
    // act
    for pattern in &patterns {
        for &width in &widths {
            let mut app = AppState::default();
            app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
                request_id: "request-wrap-correctness".to_string(),
                profile_label: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                provider_id: "openai".to_string(),
                status: ActivityStatus::Done,
                user_message: Some(harness_core::event::UserMessageSubmittedEvent {
                    request_id: "request-wrap-correctness".into(),
                    text: "wrap correctness probe".to_string(),
                }),
                user_timestamp: None,
                request_data: None,
                thinking_text: String::new(),
                thinking_first_mono_ms: None,
                thinking_last_mono_ms: None,
                transcript_text: pattern.to_string(),
                first_delta_mono_ms: None,
                usage: None,
                cache_usage: None,
                error_message: None,
                permissions: Vec::new(),
                tool_calls: Vec::new(),
                first_seq: 1,
                last_seq: 1,
                first_mono_ms: 1,
                last_mono_ms: 1,
                request_started_mono_ms: None,
                revision: 0,
            }]);
            app.transcript_view.selected_activity_index = 0;

            let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), width);
            let measured_rows = layout.total_height;

            let mut independently_computed_rows = 0usize;
            for section in &layout.sections {
                independently_computed_rows += section.leading_gap_height;
                let mut section_rows = 0usize;
                for surface in &section.surfaces {
                    let content_width = if surface.width == 0 {
                        1
                    } else {
                        usize::from(
                            surface
                                .width
                                .saturating_sub(u16::from(surface.show_outer_rail)),
                        )
                        .max(1)
                    };
                    let visual_rows = surface
                        .lines
                        .iter()
                        .map(|line| {
                            let line_width = line.width();
                            if line_width == 0 {
                                1
                            } else {
                                line_width.div_ceil(content_width)
                            }
                        })
                        .sum::<usize>();
                    section_rows = surface.top_offset + visual_rows;
                }
                independently_computed_rows += section_rows;
            }
            // assert
            assert_eq!(
                measured_rows, independently_computed_rows,
                "measured_rows ({measured_rows}) must equal independently computed rows \
                 ({independently_computed_rows}) for pattern {pattern:?} at width {width}"
            );
        }
    }
}

#[test]
fn transcript_selection_rows_proportional_to_visual_lines_not_cell_count() {
    // arrange
    let message_count = 50usize;
    let mut activities = Vec::with_capacity(message_count);
    for idx in 0..message_count {
        activities.push(transcript_section_model_test_activity(
            &format!("request-perf-{idx}"),
            ActivityStatus::Done,
            &format!("Assistant reply number {idx} with some content to render."),
        ));
    }
    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(activities);
    app.transcript_view.selected_activity_index = message_count.saturating_sub(1);

    let area = Rect::new(0, 0, 140, 40);
    // act
    let layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 80);
    let total_height = layout.total_height;

    let row_count = transcript_selection_row_count(&app, area).unwrap_or_abort();
    // assert
    assert_eq!(
        row_count, total_height,
        "SelectionRow count must equal visual line count, not scale with width"
    );

    assert!(
        row_count < message_count * 80,
        "SelectionRow count ({row_count}) must be much less than message_count * width ({}), \
         proving selection does not allocate per-cell",
        message_count * 80
    );

    assert!(
        row_count >= message_count,
        "SelectionRow count ({row_count}) must be at least proportional to message count ({message_count})"
    );

    let wide_layout = build_measured_transcript_layout_for_width(&app, &Theme::default(), 120);
    let wide_total_height = wide_layout.total_height;
    let wide_row_count =
        transcript_selection_row_count(&app, Rect::new(0, 0, 140, 40)).unwrap_or_abort();

    assert_eq!(
        wide_row_count, wide_total_height,
        "SelectionRow count must equal visual line count at any width"
    );

    let width_ratio = 120.0 / 80.0;
    let row_ratio = f64::from(u32::try_from(wide_row_count).unwrap_or(u32::MAX))
        / f64::from(u32::try_from(row_count.max(1)).unwrap_or(u32::MAX));
    assert!(
        row_ratio < width_ratio,
        "SelectionRow count should not scale linearly with width: \
         narrow={row_count}, wide={wide_row_count}, width_ratio={width_ratio:.2}, row_ratio={row_ratio:.2}"
    );
}

#[test]
fn perf_500_event_streaming_transcript_cache_and_layout_budget() {
    use std::time::{Duration, Instant};

    // arrange
    const ACTIVITY_COUNT: usize = 500;
    const STREAMING_DELTA_COUNT: usize = 20;
    const CACHE_KEY_BUDGET: Duration = Duration::from_millis(15);
    const LAYOUT_BUDGET: Duration = Duration::from_millis(500);

    let activities: Vec<ActivityEntry> = (0..ACTIVITY_COUNT)
        .map(|index| {
            let status = if index == ACTIVITY_COUNT - 1 {
                ActivityStatus::Streaming
            } else {
                ActivityStatus::Done
            };
            let mut entry = transcript_section_model_test_activity(
                &format!("req-{index:04}"),
                status,
                &format!(
                    "Assistant reply {index}: the workspace looks consistent and the \
                     transcript cache should remain stable across streaming deltas."
                ),
            );
            entry.user_message = Some(UserMessageSubmittedEvent {
                request_id: format!("req-{index:04}").into(),
                text: format!("User turn {index}: inspect the workspace."),
            });
            if index % 10 == 0 {
                entry
                    .tool_calls
                    .push(transcript_section_model_test_tool_call(
                        &format!("tc-{index:04}"),
                        "fs.read",
                    ));
            }
            entry
        })
        .collect();

    let mut app = AppState::default();
    app.activities = std::collections::VecDeque::from(activities);
    app.transcript_view.selected_activity_index = ACTIVITY_COUNT - 1;

    let theme = Theme::default();
    let width: u16 = 120;
    // act
    let _ = build_transcript_lines_for_width(&app, &theme, width);

    let mut max_cache_key = Duration::ZERO;
    let mut max_layout = Duration::ZERO;
    let mut total_delta = Duration::ZERO;

    for delta_index in 0..STREAMING_DELTA_COUNT {
        if let Some(last) = app.activities.back_mut() {
            last.transcript_text.push_str(&format!(
                " Delta {delta_index}: appending streaming text to exercise the \
                 transcript render cache and layout pipeline."
            ));
            last.revision = last.revision.wrapping_add(1);
        }

        app.advance_transcript_animation_phase();

        let key_start = Instant::now();
        let _ = app.transcript_render_cache_key();
        let key_elapsed = key_start.elapsed();

        let layout_start = Instant::now();
        let lines = build_transcript_lines_for_width(&app, &theme, width);
        let layout_elapsed = layout_start.elapsed();

        assert!(
            !lines.is_empty(),
            "transcript lines must not be empty for delta {delta_index}"
        );

        max_cache_key = max_cache_key.max(key_elapsed);
        max_layout = max_layout.max(layout_elapsed);
        total_delta += key_elapsed + layout_elapsed;
    }

    let avg_delta = total_delta / u32::try_from(STREAMING_DELTA_COUNT).unwrap_or(u32::MAX);

    eprintln!(
        "perf_500_event: {ACTIVITY_COUNT} activities, {STREAMING_DELTA_COUNT} deltas | \
         cache_key max={max_cache_key:?} | layout max={max_layout:?} | \
         avg_delta={avg_delta:?}"
    );
    // assert
    assert!(
        max_cache_key < CACHE_KEY_BUDGET,
        "per-delta cache key time {max_cache_key:?} exceeded budget {CACHE_KEY_BUDGET:?} \
         (avg delta {avg_delta:?}, {STREAMING_DELTA_COUNT} deltas, {ACTIVITY_COUNT} activities)"
    );

    assert!(
        max_layout < LAYOUT_BUDGET,
        "per-delta layout time {max_layout:?} exceeded budget {LAYOUT_BUDGET:?} \
         (avg delta {avg_delta:?}, {STREAMING_DELTA_COUNT} deltas, {ACTIVITY_COUNT} activities)"
    );
}

#[test]
fn capture_all_spacing_evidence() {
    // arrange
    // act
    // assert
    let dir = std::path::Path::new(".omo/evidence/chat-spacing-parity");
    std::fs::create_dir_all(dir).unwrap_or_abort();

    let write_evidence = |name: &str, app: &AppState, width: u16| {
        let lines = transcript_test_line_texts(build_transcript_lines_for_width(
            app,
            &Theme::default(),
            width,
        ));
        std::fs::write(dir.join(format!("{name}.txt")), lines.join("\n")).unwrap_or_abort();
    };

    let single_turn = || {
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![ActivityEntry {
            request_id: "req-single".to_string(),
            profile_label: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            provider_id: "openai".to_string(),
            status: ActivityStatus::Done,
            user_message: Some(UserMessageSubmittedEvent {
                request_id: "req-single".into(),
                text: "Hello".to_string(),
            }),
            user_timestamp: None,
            request_data: None,
            thinking_text: String::new(),
            thinking_first_mono_ms: None,
            thinking_last_mono_ms: None,
            transcript_text: "Hi there!".to_string(),
            first_delta_mono_ms: None,
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
            request_started_mono_ms: None,
            revision: 0,
        }]);
        app.transcript_view.selected_activity_index = 0;
        app
    };

    let two_turns = || {
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![
            ActivityEntry {
                request_id: "req-two-a".to_string(),
                profile_label: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                provider_id: "openai".to_string(),
                status: ActivityStatus::Done,
                user_message: Some(UserMessageSubmittedEvent {
                    request_id: "req-two-a".into(),
                    text: "First question".to_string(),
                }),
                user_timestamp: None,
                request_data: None,
                thinking_text: String::new(),
                thinking_first_mono_ms: None,
                thinking_last_mono_ms: None,
                transcript_text: "First answer".to_string(),
                first_delta_mono_ms: None,
                usage: None,
                cache_usage: None,
                error_message: None,
                permissions: Vec::new(),
                tool_calls: Vec::new(),
                first_seq: 1,
                last_seq: 1,
                first_mono_ms: 1,
                last_mono_ms: 1,
                request_started_mono_ms: None,
                revision: 0,
            },
            ActivityEntry {
                request_id: "req-two-b".to_string(),
                profile_label: "default".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                provider_id: "openai".to_string(),
                status: ActivityStatus::Done,
                user_message: Some(UserMessageSubmittedEvent {
                    request_id: "req-two-b".into(),
                    text: "Second question".to_string(),
                }),
                user_timestamp: None,
                request_data: None,
                thinking_text: String::new(),
                thinking_first_mono_ms: None,
                thinking_last_mono_ms: None,
                transcript_text: "Second answer".to_string(),
                first_delta_mono_ms: None,
                usage: None,
                cache_usage: None,
                error_message: None,
                permissions: Vec::new(),
                tool_calls: Vec::new(),
                first_seq: 2,
                last_seq: 2,
                first_mono_ms: 2,
                last_mono_ms: 2,
                request_started_mono_ms: None,
                revision: 0,
            },
        ]);
        app.transcript_view.selected_activity_index = 1;
        app
    };

    let reasoning_to_body = || {
        let mut app = AppState::default();
        let mut entry = transcript_section_model_test_activity(
            "req-reasoning",
            ActivityStatus::Done,
            "Here is my answer",
        );
        entry.thinking_text = "Let me think about this".to_string();
        entry.user_message = Some(UserMessageSubmittedEvent {
            request_id: "req-reasoning".into(),
            text: "What is 2+2?".to_string(),
        });
        app.activities = std::collections::VecDeque::from(vec![entry]);
        app.transcript_view.selected_activity_index = 0;
        app
    };

    let tool_to_body = || {
        let mut app = AppState::default();
        let mut activity = transcript_section_model_test_activity(
            "req-tool-body",
            ActivityStatus::Done,
            "The file contains the configuration",
        );
        activity.user_message = Some(UserMessageSubmittedEvent {
            request_id: "req-tool-body".into(),
            text: "Read the file".to_string(),
        });
        let mut tool = transcript_section_model_test_tool_call("tc-read", "fs.read");
        tool.status = ToolCallDisplayStatus::Succeeded;
        tool.output_summary = Some("24 lines read".to_string());
        tool.truncated_output = Some("24 lines read".to_string());
        activity.tool_calls.push(tool);
        app.activities = std::collections::VecDeque::from(vec![activity]);
        app.transcript_view.selected_activity_index = 0;
        app
    };

    let consecutive_tools = || {
        let mut app = AppState::default();
        let mut activity = transcript_section_model_test_activity(
            "req-tools",
            ActivityStatus::Done,
            "Done checking",
        );
        activity.user_message = Some(UserMessageSubmittedEvent {
            request_id: "req-tools".into(),
            text: "Check the files".to_string(),
        });
        let mut tool1 = transcript_section_model_test_tool_call("tc-read-1", "fs.read");
        tool1.status = ToolCallDisplayStatus::Succeeded;
        tool1.output_summary = Some("10 lines".to_string());
        tool1.truncated_output = Some("10 lines".to_string());
        let mut tool2 = transcript_section_model_test_tool_call("tc-read-2", "fs.read");
        tool2.status = ToolCallDisplayStatus::Succeeded;
        tool2.output_summary = Some("20 lines".to_string());
        tool2.truncated_output = Some("20 lines".to_string());
        activity.tool_calls.push(tool1);
        activity.tool_calls.push(tool2);
        app.activities = std::collections::VecDeque::from(vec![activity]);
        app.transcript_view.selected_activity_index = 0;
        app
    };

    let bash_output = || {
        let mut app = AppState::default();
        let mut activity = transcript_section_model_test_activity(
            "req-bash",
            ActivityStatus::Done,
            "Tests passed",
        );
        activity.user_message = Some(UserMessageSubmittedEvent {
            request_id: "req-bash".into(),
            text: "Run the tests".to_string(),
        });
        let mut tool = transcript_section_model_test_tool_call("tc-bash", "shell.run");
        tool.args_summary = r#"{"command":"seq 20","description":"Generate 20 lines"}"#.to_string();
        tool.status = ToolCallDisplayStatus::Succeeded;
        let output = (1..=20)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        tool.output_summary = Some(output.clone());
        tool.truncated_output = Some(output);
        activity.tool_calls.push(tool);
        app.activities = std::collections::VecDeque::from(vec![activity]);
        app.transcript_view.selected_activity_index = 0;
        app
    };

    let markdown_headings = || {
        let mut app = AppState::default();
        app.activities =
            std::collections::VecDeque::from(vec![transcript_section_model_test_activity(
                "req-markdown",
                ActivityStatus::Done,
                "Intro paragraph\n\n# Heading\n\nBody text after heading",
            )]);
        app.transcript_view.selected_activity_index = 0;
        app
    };

    let todos = || {
        let mut app = AppState::default();
        let mut activity = transcript_section_model_test_activity(
            "req-todos",
            ActivityStatus::Done,
            "Here is the plan",
        );
        activity.user_message = Some(UserMessageSubmittedEvent {
            request_id: "req-todos".into(),
            text: "Show me the plan".to_string(),
        });
        let mut tool = transcript_section_model_test_tool_call("tc-todos", "task");
        tool.status = ToolCallDisplayStatus::Succeeded;
        tool.output_summary = Some("Plan created".to_string());
        tool.truncated_output = Some("Plan created".to_string());
        activity.tool_calls.push(tool);
        app.activities = std::collections::VecDeque::from(vec![activity]);
        app.transcript_view.selected_activity_index = 0;
        app
    };

    write_evidence("single-turn", &single_turn(), 80);
    write_evidence("two-turns", &two_turns(), 80);
    write_evidence("reasoning-to-body", &reasoning_to_body(), 80);
    write_evidence("tool-to-body", &tool_to_body(), 80);
    write_evidence("consecutive-tools", &consecutive_tools(), 80);
    write_evidence("bash-output", &bash_output(), 80);
    write_evidence("markdown-headings-paragraphs", &markdown_headings(), 80);
    write_evidence("todos", &todos(), 80);
}

#[test]
fn reasoning_body_plain_text_has_no_dim_modifier() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    let mut entry =
        transcript_section_model_test_activity("req-no-dim", ActivityStatus::Streaming, "answer");
    entry.thinking_text = "Plain reasoning text without markdown markers".to_string();
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    let lines = build_transcript_lines_for_width(&app, &Theme::default(), 80);

    let reasoning_line = lines
        .iter()
        .find(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .contains("Plain reasoning")
        })
        .unwrap_or_abort();

    for span in &reasoning_line.spans {
        assert!(
            !span.style.add_modifier.contains(Modifier::DIM),
            "span {:?} should not have DIM modifier",
            span.content
        );
    }
}

#[test]
fn reasoning_body_screenshot_text_no_false_positives() {
    // arrange
    // act
    // assert
    let screenshot_text =
        "18. sessionlist, sessionread, sessionsearch, sessioninfo - session tools\n\
                           19. backgroundoutput, backgroundcancel - background task tools\n\
                           20. mcp tools (docs-rs, gh_grep)";
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "req-screenshot-text",
        ActivityStatus::Streaming,
        "answer",
    );
    entry.thinking_text = screenshot_text.to_string();
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    let lines = build_transcript_lines_for_width(&app, &Theme::default(), 80);

    let keywords = [
        "sessionlist",
        "sessionread",
        "sessionsearch",
        "sessioninfo",
        "backgroundoutput",
        "backgroundcancel",
        "gh_grep",
        "docs-rs",
    ];

    for keyword in &keywords {
        let keyword_line = lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
                    .contains(keyword)
            })
            .unwrap_or_abort();

        for span in &keyword_line.spans {
            assert!(
                !span.style.add_modifier.contains(Modifier::ITALIC),
                "span {:?} in line with {keyword:?} should not have ITALIC — \
                 intraword delimiters must not trigger emphasis",
                span.content
            );
        }
    }
}

#[test]
fn reasoning_body_markdown_constructs_use_blended_colors() {
    // arrange
    // act
    // assert
    let mut app = AppState::default();
    let mut entry =
        transcript_section_model_test_activity("req-md-blend", ActivityStatus::Streaming, "answer");
    entry.thinking_text = "See `inline_code` and **bold** and *italic*".to_string();
    app.activities = std::collections::VecDeque::from(vec![entry]);
    app.transcript_view.selected_activity_index = 0;

    let theme = Theme::default();
    let lines = build_transcript_lines_for_width(&app, &theme, 80);

    let all_spans: Vec<_> = lines.iter().flat_map(|line| line.spans.iter()).collect();

    let code_span = all_spans
        .iter()
        .find(|span| span.content == "inline_code")
        .unwrap_or_abort();
    assert_ne!(
        code_span.style.fg,
        Some(theme.markdown.code),
        "inline code should use blended (not raw) code color"
    );
    assert_ne!(
        code_span.style.fg,
        Some(theme.text.secondary),
        "inline code should not use base color"
    );

    let bold_span = all_spans
        .iter()
        .find(|span| span.content == "bold")
        .unwrap_or_abort();
    assert_ne!(
        bold_span.style.fg,
        Some(theme.markdown.strong),
        "bold should use blended (not raw) strong color"
    );
    assert!(bold_span.style.add_modifier.contains(Modifier::BOLD));

    let italic_span = all_spans
        .iter()
        .find(|span| span.content == "italic")
        .unwrap_or_abort();
    assert_ne!(
        italic_span.style.fg,
        Some(theme.markdown.emph),
        "italic should use blended (not raw) emph color"
    );
    assert!(italic_span.style.add_modifier.contains(Modifier::ITALIC));
}
