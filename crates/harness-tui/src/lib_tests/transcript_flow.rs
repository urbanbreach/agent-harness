use super::*;

pub(super) fn run_finished_keeps_transcript_and_ready_composer() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_done"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_done".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "finished".to_string(),
                request_digest: "digest-done".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_done"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_done".to_string(),
                delta: "transcript remains visible".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_done"),
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_done".to_string(),
                finish_reason: "stop".to_string(),
                output_digest: Some("digest-done-out".to_string()),
                usage: None,
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        4,
        None,
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));

    insta::with_settings!({ prepend_module_to_snapshot => false, snapshot_path => "../snapshots" }, {
        insta::assert_snapshot!("harness_tui__live_shell_finished_state", render_live_lines(&app, 80, 24));
    });
}

pub(super) fn streaming_transcript_auto_scrolls_to_latest_wrapped_content() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_scroll"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_scroll".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "scroll test".to_string(),
                request_digest: "digest-scroll".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_scroll"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_scroll".to_string(),
                delta: [
                    "HEADTOKEN",
                    "alpha",
                    "beta",
                    "gamma",
                    "delta",
                    "epsilon",
                    "zeta",
                    "eta",
                    "theta",
                    "iota",
                    "kappa",
                    "lambda",
                    "mu",
                    "nu",
                    "xi",
                    "omicron",
                    "harness",
                    "rho",
                    "sigma",
                    "tau",
                    "upsilon",
                    "phi",
                    "chi",
                    "psi",
                    "TAILTOKEN",
                ]
                .join(" "),
            },
        ),
    ));

    let debug = render_live_buffer(&app, 38, 11);
    assert!(
        debug.contains("TAILTOKEN"),
        "auto-follow should keep the latest wrapped transcript content visible: {debug}"
    );
}

pub(super) fn transcript_scrollbar_matches_session_shape() {
    let mut app = app::AppState::new_live(None, false, None);
    app.activities = std::collections::VecDeque::from(
        (0..14)
            .map(|index| {
                transcript_turn_group_test_activity(
                    &format!("request-scrollbar-{index}"),
                    app::ActivityStatus::Done,
                    Some(&format!("question {index}")),
                    &format!(
                        "reply {index} keeps wrapping through the transcript viewport so the scrollbar thumb has real room to move"
                    ),
                )
            })
            .collect::<Vec<_>>(),
    );
    app.selected_activity_index = 13;
    app.follow_mode = false;
    app.transcript_scroll = 18;

    insta::with_settings!({ prepend_module_to_snapshot => false, snapshot_path => "../snapshots" }, {
        insta::assert_snapshot!("harness_tui__live_transcript_scrollbar", render_live_lines(&app, 80, 24));
    });
}

pub(super) fn transcript_page_down_reaches_response_tail_after_scrolling_up() {
    let mut app = app::AppState::new_live(None, false, None);
    app.activities = std::collections::VecDeque::from(vec![transcript_turn_group_test_activity(
        "request-scroll-recovery",
        app::ActivityStatus::Done,
        None,
        &[
            "HEADTOKEN",
            "alpha",
            "beta",
            "gamma",
            "delta",
            "epsilon",
            "zeta",
            "eta",
            "theta",
            "iota",
            "kappa",
            "lambda",
            "mu",
            "nu",
            "xi",
            "omicron",
            "harness",
            "rho",
            "sigma",
            "tau",
            "upsilon",
            "phi",
            "chi",
            "psi",
            "omega",
            "TAILTOKEN",
        ]
        .join(" "),
    )]);
    app.selected_activity_index = 0;
    app.focus = app::Focus::Details;

    let _ = render_live_buffer(&app, 38, 11);
    app.handle_key(key(KeyCode::Home));

    let top = render_live_buffer(&app, 38, 11);
    assert!(
        top.contains("HEADTOKEN"),
        "scroll-to-top should reveal the response head: {top}"
    );
    assert!(
        !top.contains("TAILTOKEN"),
        "top view should not already show the tail: {top}"
    );

    for _ in 0..20 {
        app.handle_key(key(KeyCode::PageDown));
        if app.follow_mode {
            break;
        }
    }

    let bottom = render_live_buffer(&app, 38, 11);
    assert!(
        bottom.contains("TAILTOKEN"),
        "paging back down should make the tail reachable again: {bottom}"
    );
}

pub(super) fn transcript_without_overflow_hides_scrollbar() {
    let mut app = app::AppState::new_live(None, false, None);
    app.activities = std::collections::VecDeque::from(vec![transcript_turn_group_test_activity(
        "request-no-scrollbar",
        app::ActivityStatus::Done,
        Some("short question"),
        "short reply",
    )]);
    app.selected_activity_index = 0;
    app.follow_mode = true;

    let rendered = render_live_lines(&app, 80, 24);
    assert!(
        !rendered.contains('│'),
        "non-overflow transcripts should not reserve the shell scrollbar track\n{rendered}"
    );
}

pub(super) fn disconnected_stream_disables_composer_with_reopen_guidance() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_disconnect"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_disconnect".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "disconnect".to_string(),
                request_digest: "digest-disconnect".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_disconnect"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_disconnect".to_string(),
                delta: "transcript stays visible".to_string(),
            },
        ),
    ));
    app.set_status_banner(Some("live event stream disconnected".to_string()));
    app.handle_key(key(crossterm::event::KeyCode::Char('x')));

    let debug = render_live_buffer(&app, 80, 24);
    assert!(app.prompt_buffer.is_empty());
    assert!(debug.contains("transcript stays visible"));
    assert!(debug.contains("Disconnected"));
    assert!(!debug.contains("Composer ·"));
    assert!(!debug.contains("Draft preserved locally"));
    assert!(debug.contains("Reopen the TUI, then continue from the transcript."));
}

pub(super) fn transcript_renders_inline_tool_states_and_prompt_echo() {
    let mut app = app::AppState::new_live(None, false, None);

    for c in "Inspect src/ui.rs".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    app.ingest_event(envelope(
        1,
        Some("req_inline"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_inline".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Inspect src/ui.rs".to_string(),
                request_digest: "digest-inline".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_inline"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_inline".to_string(),
                delta: "Drafting a plan".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_inline"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_inline".to_string(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"false"}"#.to_string(),
                args_digest: "digest-inline-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        4,
        Some("req_inline"),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tc_inline".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some("req_inline"),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_inline".to_string(),
                status: harness_core::event::ToolCallStatus::Failed,
                output_summary: Some("exit code: 1".to_string()),
                output_digest: None,
                output_json: None,
                metadata: None,
            },
        ),
    ));

    let debug = render_live_buffer(&app, 80, 24);
    assert!(debug.contains("Inspect src/ui.rs"));
    assert!(debug.contains("exit code: 1") || debug.contains("Drafting a plan"));
    assert!(!debug.contains("args {"));
    assert!(!debug.contains(r#"{"cmd":"false"}"#));
}

pub(super) fn transcript_tool_rows_keep_status_but_not_raw_json_dump() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_tool_compact"),
        harness_core::event::EventV1::UserMessageSubmitted(
            harness_core::event::UserMessageSubmittedEvent {
                request_id: "req_tool_compact".to_string(),
                text: "Read the file".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_tool_compact"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_tool_compact".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Read the file".to_string(),
                request_digest: "digest-tool-compact".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_tool_compact"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_compact".to_string(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"path":"src/lib.rs","start_line":42,"limit":20}"#.to_string(),
                args_digest: "digest-tool-compact-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        4,
        Some("req_tool_compact"),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tc_compact".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some("req_tool_compact"),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_compact".to_string(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("12 lines read".to_string()),
                output_digest: Some("digest-tool-compact-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));

    let transcript = render_live_lines(&app, 120, 36);
    assert!(transcript.contains("Read src/lib.rs [offset=42, limit=20]"));
    assert!(!transcript.contains(r#"{"path":"src/lib.rs","start_line":42,"limit":20}"#));
    assert!(!transcript.contains("args {"));
}

pub(super) fn transcript_shell_renders_bubbleless_document_flow() {
    transcript_shell_remains_scannable_without_bubble_cards();
}

pub(super) fn transcript_shell_remains_scannable_without_bubble_cards() {
    let app = rich_transcript_fixture_app();

    let rendered = render_live_lines(&app, 120, 30);
    let lines = rendered.lines().collect::<Vec<_>>();
    let prompt_row =
        find_line_containing(&lines, "Restyle the transcript shell").expect("user prompt row");
    let thinking_row =
        find_line_containing_all_from(&lines, prompt_row + 1, &["Drafting a document-like plan"])
            .expect("reasoning row");
    let tool_row = find_line_containing_all_from(
        &lines,
        thinking_row + 1,
        &["Read src/ui.rs", "[offset=1, limit=24]"],
    )
    .expect("tool row");
    let body_row = find_line_containing_from(
        &lines,
        tool_row + 1,
        "Found the transcript renderer and the composer chrome.",
    )
    .expect("assistant body row");

    assert!(prompt_row < body_row);
    assert!(prompt_row < thinking_row);
    assert!(thinking_row < tool_row);
    assert!(tool_row < body_row);
    assert!(
        first_alphanumeric_column(lines[thinking_row]) == first_alphanumeric_column(lines[body_row]),
        "reasoning should align with the assistant body text while keeping its own muted rail\n{rendered}"
    );
    assert!(
        first_alphanumeric_column(lines[tool_row]) > first_alphanumeric_column(lines[body_row]),
        "tool details should remain nested deeper than the assistant body rail\n{rendered}"
    );
    assert!(!rendered.contains("Composer ·"));
    assert!(!rendered.contains("Ask Harness to inspect, edit, or explain…"));
    assert!(!rendered.contains("Current runtime: default · model-1"));
    assert!(!rendered.contains("provider mock"));
    assert!(!rendered.contains("┌"));
    assert!(!rendered.contains("└"));
    assert!(!rendered.contains("(tool fs.read · succeeded)"));
}

pub(super) fn transcript_status_metadata_stays_tool_inline_without_assistant_footer() {
    let app = rich_transcript_fixture_app();

    let rendered = render_live_lines(&app, 120, 30);

    assert!(!rendered.contains("req_rich_shell"));
    assert!(!rendered.contains("Assistant ·"));
    assert!(rendered.contains("Read src/ui.rs [offset=1, limit=24]"));
    assert!(!rendered.contains("user ("));
    assert!(!rendered.contains("assistant ("));
    assert!(!rendered.contains("(tool fs.read · succeeded)"));
}

pub(super) fn transcript_turn_spacing_collapses_without_losing_actor_boundaries() {
    let app = multi_turn_transcript_fixture_app();
    let rendered = render_live_lines(&app, 120, 30);
    let lines = rendered.lines().collect::<Vec<_>>();

    let first_reply_row = find_line_containing(&lines, "The shell is transcript-first and calm.")
        .expect("first assistant body row");
    let second_prompt_row = find_line_containing_from(
        &lines,
        first_reply_row + 1,
        "Tighten the transcript spacing",
    )
    .expect("second prompt row");
    let second_assistant_row = find_line_containing_from(
        &lines,
        second_prompt_row + 1,
        "Spacing is collapsed without losing turn boundaries.",
    )
    .expect("second assistant row");

    assert!(
        second_prompt_row > first_reply_row,
        "second turn should follow the first reply\n{rendered}"
    );
    assert!(
        second_assistant_row > second_prompt_row,
        "assistant reply should stay below the second prompt\n{rendered}"
    );
}

pub(super) fn nested_transcript_rows_preserve_prefix_on_wrapped_continuations() {
    let mut app = rich_transcript_fixture_app();
    app.activities[0].thinking_text = "Drafting a document-like plan with enough extra detail to force a wrapped continuation so the nested rail stays visible on every continued row.".to_string();

    let rendered = render_live_lines(&app, 80, 24);
    let lines = rendered.lines().collect::<Vec<_>>();
    let thinking_row = find_line_containing(&lines, "Drafting a document-like plan")
        .expect("wrapped reasoning row");
    let body_row = find_line_containing(
        &lines,
        "Found the transcript renderer and the composer chrome.",
    )
    .expect("assistant body row");
    let continuation_row = (thinking_row + 1..body_row)
        .find(|row| !lines[*row].trim().is_empty())
        .expect("wrapped continuation row");
    let answer_gap_row = (continuation_row + 1..body_row)
        .find(|row| lines[*row].trim().is_empty())
        .expect("blank gap row before assistant body");

    assert!(
        first_alphanumeric_column(lines[thinking_row])
            == first_alphanumeric_column(lines[body_row]),
        "reasoning should keep the same text column while wrapping under its own rail\n{rendered}"
    );
    assert_eq!(
        first_alphanumeric_column(lines[thinking_row]),
        first_alphanumeric_column(lines[continuation_row]),
        "wrapped nested continuation should repeat the nested prefix and rail\n{rendered}"
    );
    assert!(answer_gap_row < body_row);
}

pub(super) fn thinking_visibility_toggle_hides_and_restores_inline_thinking_rows() {
    let mut app = rich_transcript_fixture_app();

    let initial = render_live_lines(&app, 120, 30);
    assert!(initial.contains("Drafting a document-like plan"));

    run_palette_command(&mut app, "hide thinking");
    let hidden = render_live_lines(&app, 120, 30);
    assert!(!hidden.contains("Drafting a document-like plan"));
    assert!(hidden.contains("Found the transcript renderer and the composer chrome."));

    run_palette_command(&mut app, "show thinking");
    let restored = render_live_lines(&app, 120, 30);
    assert!(restored.contains("Drafting a document-like plan"));
}

pub(super) fn tool_details_toggle_collapses_successful_tool_payloads() {
    let mut app = rich_transcript_fixture_app();

    let shown = render_live_lines(&app, 120, 30);
    assert!(shown.contains("Read src/ui.rs [offset=1, limit=24]"));

    run_palette_command(&mut app, "hide tool details");
    let hidden = render_live_lines(&app, 120, 30);
    assert!(!hidden.contains("Read src/ui.rs [offset=1, limit=24]"));

    run_palette_command(&mut app, "show tool details");
    let restored = render_live_lines(&app, 120, 30);
    assert!(restored.contains("Read src/ui.rs [offset=1, limit=24]"));
}

pub(super) fn failed_tool_rows_still_surface_error_summary() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_tool_error"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_tool_error".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "Run the command".to_string(),
                request_digest: "digest-tool-error".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_tool_error"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_error".to_string(),
                tool_id: "shell.run".to_string(),
                args_summary: r#"{"cmd":"false","cwd":"/tmp/demo"}"#.to_string(),
                args_digest: "digest-tool-error-args".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_tool_error"),
        harness_core::event::EventV1::ToolCallStarted(harness_core::event::ToolCallStartedEvent {
            tool_call_id: "tc_error".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some("req_tool_error"),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_error".to_string(),
                status: harness_core::event::ToolCallStatus::Failed,
                output_summary: Some("exit code: 1\nstderr: permission denied".to_string()),
                output_digest: None,
                output_json: None,
                metadata: None,
            },
        ),
    ));

    let transcript = render_live_lines(&app, 120, 36);
    assert!(transcript.contains("false"));
    assert!(transcript.contains("exit code: 1 stderr: permission denied"));
    assert!(!transcript.contains(r#"{"cmd":"false","cwd":"/tmp/demo"}"#));
    assert!(!transcript.contains("args {"));
}

pub(super) fn permission_overlay_preserves_draft_and_transcript_context() {
    let mut app = app::AppState::new_live(None, false, None);

    for c in "keep this draft".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.ingest_event(permission_requested_event(
        1,
        "perm_overlay",
        "tool_call_overlay",
    ));

    let debug = render_live_buffer(&app, 80, 24);
    assert!(!debug.contains("Composer · disabled · Permission blocked"));
    assert!(debug.contains("Permission required"));
    assert!(debug.contains("Draft preserved · keep this draft"));
    assert!(!debug.contains("Select an activity to view transcript"));
    assert!(
        debug.matches("Apply hashline edit to demo.txt").count() >= 1,
        "permission summary should remain visible in the modal"
    );
}

pub(super) fn permission_overlay_ignores_plain_draft_input_once_prompt_is_active() {
    let mut app = app::AppState::new_live(None, false, None);

    for c in "keep this dr".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    app.ingest_event(permission_requested_event(
        1,
        "perm_overlay_buffered_input",
        "tool_call_overlay_buffered_input",
    ));

    for c in "aft".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Char('/')));

    assert_eq!(app.prompt_buffer, "keep this dr");
    assert!(app.active_permission().is_some());

    let debug = render_live_buffer(&app, 80, 24);
    assert!(debug.contains("Draft preserved · keep this dr"));
    assert!(!debug.contains("Slash commands"));
}

pub(super) fn permission_overlay_preserves_existing_draft_without_buffering_new_letters() {
    let mut app = app::AppState::new_live(None, false, None);

    for c in "keep t".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    app.ingest_event(permission_requested_event(
        1,
        "perm_overlay_home_row_input",
        "tool_call_overlay_home_row_input",
    ));

    for c in "zz".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    assert_eq!(app.prompt_buffer, "keep t");
    assert_eq!(
        app.permission_modal_selection("perm_overlay_home_row_input"),
        app::permissions::PermissionModalSelection::AllowOnce
    );

    let debug = render_live_buffer(&app, 80, 24);
    assert!(debug.contains("Draft preserved · keep t"));
}
