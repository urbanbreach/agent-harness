use super::*;
use crate::UnwrapOrAbort;
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
        revision: 0,
    }]);
    app.transcript_view.selected_activity_index = 0;
    // act
    AppState::reset_transcript_render_key_metrics_for_test();
    let _ = app.transcript_measure_cache_key();
    // assert
    let builds_after_first = AppState::transcript_render_key_build_count_for_test();
    assert_eq!(builds_after_first, 1, "first call should build once");

    app.advance_transcript_animation_phase();
    let _ = app.transcript_measure_cache_key();
    let builds_after_animation = AppState::transcript_render_key_build_count_for_test();
    assert_eq!(
        builds_after_animation, 1,
        "animation phase change must not rebuild the measure cache key"
    );
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
            request_id: "request-theme-cache".into(),
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
        revision: 0,
    }]);
    app.transcript_view.selected_activity_index = 0;

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
        revision: 0,
    }]);
    app.transcript_view.selected_activity_index = 0;

    let lines = transcript_test_line_texts(build_transcript_lines_for_width(
        &app,
        &Theme::default(),
        80,
    ));

    let footer_row = lines
        .iter()
        .position(|line| line.contains("Assistant · gpt-5.4-mini · active"))
        .unwrap_or_abort();
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
                request_id: "request-old-footer".into(),
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
    app.transcript_view.selected_activity_index = 0;

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
            request_id: "request-user-padding".into(),
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
fn transcript_turn_sections_keep_two_blank_rows_between_sections() {
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
        layout.sections[1].leading_gap_height, 2,
        "turn-to-turn gap should be 2 blank rows to match Opencode 24px session-turn-list gap"
    );
}

#[test]
fn markdown_headings_get_blank_row_before_when_preceded_by_text() {
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
    assert_eq!(tool_interactions[command_row], None);
    assert_eq!(tool_interactions[output_row], None);
    assert!(
        tool_lines.iter().any(|line| line.contains('┃')),
        "harness shell blocks should render the split rail\n{tool_lines:#?}"
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
        copied.starts_with("$ printf 'bash smoke test ok"),
        "copied shell card text should skip visual rail/padding: {copied:?}"
    );
    assert!(copied.contains("$ printf 'bash smoke test ok"));
    assert!(copied.contains("bash smoke test ok"));
    assert!(!copied.contains('┃'));
}

#[test]
fn assistant_tool_surface_spacing_matches_shell_rhythm() {
    assert_eq!(
        transcript_surface_leading_gap(
            Some(TranscriptRenderSurfaceKind::AssistantBody),
            TranscriptRenderSurfaceKind::AssistantTool,
        ),
        1,
        "assistant text should leave the reference section break before tool rows"
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
        "reasoning-to-body surface gap should be 1 to match Opencode 12px assistant-content gap"
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
fn reasoning_to_answer_transition_uses_two_blank_rows() {
    let mut app = AppState::default();
    let mut entry = transcript_section_model_test_activity(
        "request-reasoning-gap",
        ActivityStatus::Done,
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
        reasoning_row + 3,
        "reasoning and answer should be separated by 2 blank rows (surface gap + prepend gap)\n{lines:#?}"
    );
    assert!(lines[reasoning_row + 1].is_empty());
    assert!(lines[reasoning_row + 2].is_empty());
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
        revision: 0,
    }]);
    app.transcript_view.selected_activity_index = 0;

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
                transcript_text: pattern.to_string(),
                usage: None,
                cache_usage: None,
                error_message: None,
                permissions: Vec::new(),
                tool_calls: Vec::new(),
                first_seq: 1,
                last_seq: 1,
                first_mono_ms: 1,
                last_mono_ms: 1,
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
            transcript_text: "Hi there!".to_string(),
            usage: None,
            cache_usage: None,
            error_message: None,
            permissions: Vec::new(),
            tool_calls: Vec::new(),
            first_seq: 1,
            last_seq: 1,
            first_mono_ms: 1,
            last_mono_ms: 1,
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
                transcript_text: "First answer".to_string(),
                usage: None,
                cache_usage: None,
                error_message: None,
                permissions: Vec::new(),
                tool_calls: Vec::new(),
                first_seq: 1,
                last_seq: 1,
                first_mono_ms: 1,
                last_mono_ms: 1,
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
                transcript_text: "Second answer".to_string(),
                usage: None,
                cache_usage: None,
                error_message: None,
                permissions: Vec::new(),
                tool_calls: Vec::new(),
                first_seq: 2,
                last_seq: 2,
                first_mono_ms: 2,
                last_mono_ms: 2,
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
