use super::*;

#[test]
fn open_tool_viewer_refreshes_when_the_command_finishes() {
    let mut app = AppState::new_live(None, false, None);
    let events = shell_test_events(
        ToolCallStatus::Succeeded,
        serde_json::json!({
            "command":"produce output", "status":0, "success":true, "stdout":"OUTPUT_READY", "stderr":""
        }),
    );
    let finished = events
        .iter()
        .find(|event| matches!(event.payload, EventV1::ToolCallFinished(_)))
        .unwrap_or_abort()
        .clone();
    for event in events.into_iter().filter(|event| event.seq < finished.seq) {
        app.ingest_event(event);
    }
    let id = app.activities[0].tool_calls[0].tool_call_id.clone();
    assert!(app.select_transcript_tool(&id));
    assert!(app.open_selected_transcript_viewer());
    assert!(!app
        .transcript_viewer()
        .unwrap_or_abort()
        .content()
        .content()
        .contains("OUTPUT_READY"));
    app.ingest_event(finished);
    assert!(app
        .transcript_viewer()
        .unwrap_or_abort()
        .content()
        .content()
        .contains("OUTPUT_READY"));
}

#[test]
fn deep_tool_search_keeps_the_match_above_the_search_footer() {
    let mut app = AppState::new_live(None, false, None);
    for event in shell_test_events(
        ToolCallStatus::Succeeded,
        serde_json::json!({
            "command": "produce output", "status": 0, "success": true,
            "stdout": (1..=60).map(|i| format!("DEEP_{i:03}")).collect::<Vec<_>>().join("\n"),
            "stderr": ""
        }),
    ) {
        app.ingest_event(event);
    }
    app.ingest_event(envelope(
        100,
        "next",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "next".into(),
            text: "Following turn".into(),
        }),
    ));
    app.set_frame_area(Rect::new(0, 0, 80, 24));
    let _ = render_text(&app, 80, 24);
    app.focus = Focus::Details;
    app.transcript_view.search_query = "DEEP_030".into();
    app.find_transcript_match(None);
    let screen = render_text(&app, 80, 24);
    assert_eq!(screen.matches("DEEP_030").count(), 2, "{screen}");
    assert!(screen
        .lines()
        .find(|line| line.contains("n/N next/previous"))
        .unwrap_or_abort()
        .trim_end()
        .ends_with("n/N next/previous"));
}

#[test]
fn filtered_viewer_quotes_the_visible_line_without_selection() {
    let (mut app, ids) = command_group_app(1);
    app.set_frame_area(Rect::new(0, 0, 80, 24));
    assert!(app.select_transcript_tool(&ids[0]));
    assert!(app.open_selected_transcript_viewer());
    let viewer = app
        .transcript_integration
        .as_mut()
        .and_then(TranscriptComposite::viewer_mut)
        .unwrap_or_abort();
    viewer
        .update_content(
            crate::transcript_block_viewer::ViewerBlockContent::new(
                &(1..=80)
                    .map(|i| {
                        if i == 3 {
                            "RESULT_MARKER".to_owned()
                        } else {
                            format!("Line {i:03}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                None,
            )
            .with_preamble(crate::transcript_block_viewer::ViewerPreamble::Read {
                path: "sample.txt".into(),
                details: String::new(),
                start_line: Some(1),
            }),
        )
        .unwrap_or_abort();
    app.handle_key(key(KeyCode::End));
    assert!(render_text(&app, 80, 24).contains("Line 080"));
    app.handle_key(key(KeyCode::Char('v')));
    let screen = render_text(&app, 80, 24);
    assert!(screen.contains("Line 080"), "{screen}");
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Char('f')));
    for character in "RESULT_MARKER".chars() {
        app.handle_key(key(KeyCode::Char(character)));
        app.set_frame_area(Rect::new(0, 0, 120, 40));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(render_text(&app, 80, 24).contains("RESULT_MARKER"));
    app.handle_key(key(KeyCode::Enter));
    assert!(
        app.composer.prompt_buffer.contains("RESULT_MARKER"),
        "{:?}",
        app.composer.prompt_buffer
    );
    assert!(render_text(&app, 80, 24).contains("RESULT_MARKER"));
}

#[test]
fn transcript_search_opens_the_matching_member_of_a_collapsed_group() {
    let (mut app, ids) = command_group_app(14);
    let _ = render_text(&app, 80, 24);
    app.transcript_view.search_query = "command-00".into();
    app.find_transcript_match(None);
    assert!(app.tool_output_expanded(app.tool_call_entry(&ids[0]).unwrap_or_abort()));
    assert!(!app.tool_output_expanded(app.tool_call_entry(&ids[1]).unwrap_or_abort()));
    assert!(
        matches!(app.selected_transcript_entry().and_then(|entry| entry.target), Some(TranscriptMouseTarget::Tool { tool_call_id }) if tool_call_id == ids[0])
    );
    assert!(render_text(&app, 80, 24).contains("command-00"));
    app.focus = Focus::Details;
    app.transcript_view.search_query = "command-".into();
    app.find_transcript_match(None);
    app.handle_key(key(KeyCode::Char('n')));
    assert_eq!(app.transcript_view.search_match, 1);
    app.handle_key(key_with_modifiers(KeyCode::Char('N'), KeyModifiers::SHIFT));
    assert_eq!(app.transcript_view.search_match, 0);
    assert!(app.composer.prompt_buffer.is_empty());
}

fn command_group_app(command_count: usize) -> (AppState, Vec<String>) {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(provider_started(
        1,
        "req_tool_group_parity",
        "default",
        "gpt-5.4-mini",
    ));
    let mut tool_call_ids = Vec::with_capacity(command_count);
    for index in 0..command_count {
        let tool_call_id = format!("tc_group_parity_{index:02}");
        let seq = u64::try_from(index).unwrap_or_abort() * 2 + 2;
        app.ingest_event(envelope(
            seq,
            "req_tool_group_parity",
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: tool_call_id.clone().into(),
                tool_id: "bash".to_string(),
                args_summary: format!(r#"{{"command":"printf command-{index:02}"}}"#),
                args_digest: format!("digest-group-parity-{index:02}-args"),
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            seq + 1,
            "req_tool_group_parity",
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: tool_call_id.clone().into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some(format!("command-{index:02}")),
                output_digest: None,
                output_json: None,
                metadata: None,
            }),
        ));
        tool_call_ids.push(tool_call_id);
    }
    (app, tool_call_ids)
}

pub(super) fn zero_tool_groups_render_without_fold_affordance() {
    // arrange
    let (app, _) = command_group_app(0);

    // act
    let screen = render_text(&app, 140, 40);

    // assert
    assert!(!screen.contains("Ran "), "{screen}");
    assert!(!screen.contains(" more"), "{screen}");
}

pub(super) fn one_tool_group_member_renders_once_without_more_affordance() {
    // arrange
    let (app, _) = command_group_app(1);

    // act
    let screen = render_text(&app, 140, 40);

    // assert
    assert_eq!(screen.matches("printf command-00").count(), 1, "{screen}");
    assert!(!screen.contains("1 more"), "{screen}");
}

pub(super) fn many_tool_group_members_render_one_exact_hidden_count_affordance() {
    // arrange
    let (app, _) = command_group_app(14);

    // act
    let screen = render_text(&app, 140, 40);

    // assert
    assert_eq!(screen.matches("Ran 4 commands").count(), 1, "{screen}");
    assert_eq!(
        screen.matches("Run printf command-").count(),
        10,
        "{screen}"
    );
    assert!(
        !screen.contains("command-00") && screen.contains("command-13"),
        "{screen}"
    );
}

pub(super) fn tool_group_fold_round_trip_survives_compaction_and_narrow_reflow() {
    // arrange
    let (mut app, tool_call_ids) = command_group_app(14);
    let target = TranscriptMouseTarget::ToolGroup {
        tool_call_ids: tool_call_ids.clone(),
    };
    assert_eq!(
        render_text(&app, 140, 40).matches("Ran 4 commands").count(),
        1
    );

    // act
    app.activate_transcript_mouse_target(target.clone());
    assert!(app.tool_group_expanded(&tool_call_ids[0]));
    assert!(app.transcript_view.expanded_tool_outputs.is_empty());
    assert_eq!(
        render_text(&app, 140, 40)
            .matches("Run printf command-")
            .count(),
        13
    );
    app.activate_transcript_mouse_target(target.clone());
    app.ingest_event(envelope(
        31,
        "compaction:agent_tool_group_parity",
        EventV1::CompactionWritten(CompactionWrittenEvent {
            checkpoint_id: "checkpoint_tool_group_parity".to_string(),
            agent_id: "agent_tool_group_parity".to_string(),
            artifact_path: "artifacts/compactions/agent_tool_group_parity/checkpoint.json"
                .to_string(),
            artifact_digest: Some("digest-tool-group-compaction".to_string()),
            artifact_bytes: 64,
            trigger_reason: "manual".to_string(),
            through_seq: 30,
            through_request_id: Some("req_tool_group_parity".to_string()),
            provider_id: Some("default".to_string()),
            model_id: Some("gpt-5.4-mini".to_string()),
            tokens_before: Some(1_000),
            tokens_before_estimate: Some(1_000),
            tokens_after_estimate: Some(400),
            summary_tokens_estimate: Some(80),
            compacted_turns: Some(1),
            reduction_tokens_estimate: Some(600),
            reduction_percent_estimate: Some(60),
            estimate_source: Some("fixture".to_string()),
            summary_source: None,
            preserved_turns: 1,
        }),
    ));
    let narrow_screen = render_text(&app, 80, 50);

    // assert
    assert!(!app.tool_group_expanded(&tool_call_ids[0]));
    assert!(app.transcript_view.expanded_tool_outputs.is_empty());
    assert!(app.transcript_view.collapsed_tool_outputs.is_empty());
    assert_eq!(
        narrow_screen.matches("Ran 4 commands").count(),
        1,
        "{narrow_screen}"
    );

    // act
    app.activate_transcript_mouse_target(target);

    // assert
    assert!(app.tool_group_expanded(&tool_call_ids[0]));
    assert!(app.transcript_view.expanded_tool_outputs.is_empty());
}
