use super::*;
use crate::UnwrapOrAbort;

pub(super) fn run_finished_keeps_transcript_and_ready_composer() {
    let mut app = app::AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        Some("req_done"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_done".into(),
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
                request_id: "req_done".into(),
                delta: "transcript remains visible".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_done"),
        harness_core::event::EventV1::ProviderRequestFinished(
            harness_core::event::ProviderRequestFinishedEvent {
                request_id: "req_done".into(),
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
                request_id: "req_scroll".into(),
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
                request_id: "req_scroll".into(),
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

    let debug = render_live_buffer(&app, 60, 24);
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
    app.transcript_view.selected_activity_index = 13;
    app.transcript_view.follow_mode = false;
    app.transcript_view.transcript_scroll = 18;

    let rendered = render_live_lines(&app, 80, 24)
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered.contains('▼'),
        "mid-list scroll must paint more-below affordance\n{rendered}"
    );

    insta::with_settings!({ prepend_module_to_snapshot => false, snapshot_path => "../snapshots" }, {
        insta::assert_snapshot!(
            "harness_tui__live_transcript_scrollbar",
            rendered
        );
    });
}

pub(super) fn transcript_page_down_reaches_response_tail_after_scrolling_up() {
    let mut app = app::AppState::new_live(None, false, None);
    // Force many visual rows so HEADTOKEN and TAILTOKEN cannot share one viewport
    // after breadcrumb + dock chrome. Newlines beat wrapping for deterministic height.
    let long_body = format!(
        "HEADTOKEN\n{}\nTAILTOKEN",
        (0..60)
            .map(|index| format!("overflow-line-{index:02}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    app.activities = std::collections::VecDeque::from(vec![transcript_turn_group_test_activity(
        "request-scroll-recovery",
        app::ActivityStatus::Done,
        None,
        &long_body,
    )]);
    app.transcript_view.selected_activity_index = 0;
    app.focus = app::Focus::Details;

    let _ = render_live_buffer(&app, 60, 24);
    app.handle_key(key(KeyCode::Home));

    let top = render_live_buffer(&app, 60, 24);
    assert!(
        top.contains("HEADTOKEN"),
        "scroll-to-top should reveal the response head: {top}"
    );
    assert!(
        !top.contains("TAILTOKEN"),
        "top view should not already show the tail: {top}"
    );

    for _ in 0..40 {
        app.handle_key(key(KeyCode::PageDown));
        if app.transcript_view.follow_mode {
            break;
        }
    }

    let bottom = render_live_buffer(&app, 60, 24);
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
    app.transcript_view.selected_activity_index = 0;
    app.transcript_view.follow_mode = true;

    let rendered = render_live_lines(&app, 80, 24);
    let lines = rendered.lines().collect::<Vec<_>>();
    let transcript_end = find_line_containing(&lines, "❯")
        .or_else(|| find_line_containing(&lines, "╭"))
        .unwrap_or(lines.len());
    let transcript_body = lines[..transcript_end].join("\n");
    assert!(
        !transcript_body.contains('│') && !transcript_body.contains('█'),
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
                request_id: "req_disconnect".into(),
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
                request_id: "req_disconnect".into(),
                delta: "transcript stays visible".to_string(),
            },
        ),
    ));
    app.set_status_banner(Some("live event stream disconnected".to_string()));
    app.handle_key(key(crossterm::event::KeyCode::Char('x')));

    let debug = render_live_buffer(&app, 80, 24);
    assert!(app.composer.prompt_buffer.is_empty());
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
                request_id: "req_inline".into(),
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
                request_id: "req_inline".into(),
                delta: "Drafting a plan".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        3,
        Some("req_inline"),
        harness_core::event::EventV1::ToolCallRequested(
            harness_core::event::ToolCallRequestedEvent {
                tool_call_id: "tc_inline".into(),
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
            tool_call_id: "tc_inline".into(),
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some("req_inline"),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_inline".into(),
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
                request_id: "req_tool_compact".into(),
                text: "Read the file".to_string(),
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_tool_compact"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_tool_compact".into(),
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
                tool_call_id: "tc_compact".into(),
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
            tool_call_id: "tc_compact".into(),
        }),
    ));
    app.ingest_event(envelope(
        5,
        Some("req_tool_compact"),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_compact".into(),
                status: harness_core::event::ToolCallStatus::Succeeded,
                output_summary: Some("12 lines read".to_string()),
                output_digest: Some("digest-tool-compact-output".to_string()),
                output_json: None,
                metadata: None,
            },
        ),
    ));

    let transcript = render_live_lines(&app, 120, 36);
    assert!(transcript.contains("Read 1 file"));
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
    let prompt_row = find_line_containing(&lines, "Restyle the transcript shell").unwrap_or_abort();
    let thinking_row =
        find_line_containing_all_from(&lines, prompt_row + 1, &["Thought"]).unwrap_or_abort();
    let tool_row =
        find_line_containing_all_from(&lines, thinking_row + 1, &["Read 1 file"]).unwrap_or_abort();
    let body_row = find_line_containing_from(
        &lines,
        tool_row + 1,
        "Found the transcript renderer and the composer chrome.",
    )
    .unwrap_or_abort();

    assert!(prompt_row < body_row);
    assert!(prompt_row < thinking_row);
    assert!(thinking_row < tool_row);
    assert!(tool_row < body_row);
    assert!(
        lines[tool_row].contains('┃') && !lines[body_row].contains('┃'),
        "tool details should keep their nested rail while the assistant body remains rail-free\n{rendered}"
    );
    assert!(!rendered.contains("Composer ·"));
    assert!(!rendered.contains("Ask Harness to inspect, edit, or explain…"));
    assert!(!rendered.contains("Current runtime: default · model-1"));
    assert!(!rendered.contains("provider mock"));
    assert!(!rendered.contains("┌"));
    assert!(!rendered.contains("└"));
    assert!(!rendered.contains("(tool fs.read · succeeded)"));
}

pub(super) fn transcript_status_metadata_is_inline_not_chrome() {
    let app = rich_transcript_fixture_app();

    let rendered = render_live_lines(&app, 120, 30);

    assert!(!rendered.contains("req_rich_shell"));
    assert!(rendered.contains("model-1"));
    assert!(rendered.contains("Read 1 file"));
    assert!(!rendered.contains("user ("));
    assert!(!rendered.contains("assistant ("));
    assert!(!rendered.contains("(tool fs.read · succeeded)"));
}

pub(super) fn transcript_turn_spacing_collapses_without_losing_actor_boundaries() {
    let app = multi_turn_transcript_fixture_app();
    let rendered = render_live_lines(&app, 120, 30);
    let lines = rendered.lines().collect::<Vec<_>>();

    let first_reply_row =
        find_line_containing(&lines, "The shell is transcript-first and calm.").unwrap_or_abort();
    let second_prompt_row = find_line_containing_from(
        &lines,
        first_reply_row + 1,
        "Tighten the transcript spacing",
    )
    .unwrap_or_abort();
    let second_assistant_row = find_line_containing_from(
        &lines,
        second_prompt_row + 1,
        "Spacing is collapsed without losing turn boundaries.",
    )
    .unwrap_or_abort();

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
    app.transcript_view.selected_activity_index = 0;
    assert!(app.toggle_selected_transcript_fold());

    // Scroll to top so wrapped thinking first-line + body both stay visible under breadcrumb chrome.
    app.transcript_view.follow_mode = false;
    app.transcript_view.transcript_scroll = usize::MAX;
    let rendered = render_live_lines(&app, 80, 36);
    let lines = rendered.lines().collect::<Vec<_>>();
    let thinking_row =
        find_line_containing(&lines, "Drafting a document-like plan").unwrap_or_abort();
    let body_row = find_line_containing(
        &lines,
        "Found the transcript renderer and the composer chrome.",
    )
    .unwrap_or_abort();
    let continuation_row = (thinking_row + 1..body_row)
        .find(|row| !lines[*row].trim().is_empty())
        .unwrap_or_abort();

    assert_eq!(
        first_alphanumeric_column(lines[thinking_row]),
        first_alphanumeric_column(lines[continuation_row]),
        "wrapped nested continuation should repeat the nested prefix and rail\n{rendered}"
    );
    assert!(
        continuation_row < body_row,
        "wrapped thinking continuation should stay above the assistant body\n{rendered}"
    );
}

pub(super) fn thinking_visibility_toggle_hides_and_restores_inline_thinking_rows() {
    let mut app = rich_transcript_fixture_app();

    let initial = render_live_lines(&app, 120, 30);
    assert!(initial.contains("Thought"));
    assert!(!initial.contains("Drafting a document-like plan"));

    run_palette_command(&mut app, "collapse thinking");
    let hidden = render_live_lines(&app, 120, 30);
    assert!(!hidden.contains("Thought"));
    assert!(!hidden.contains("Drafting a document-like plan"));
    assert!(hidden.contains("Found the transcript renderer and the composer chrome."));

    run_palette_command(&mut app, "expand thinking");
    let restored = render_live_lines(&app, 120, 30);
    assert!(restored.contains("Thought"));
    assert!(!restored.contains("Drafting a document-like plan"));

    app.transcript_view.selected_activity_index = 0;
    assert!(app.toggle_selected_transcript_fold());
    let expanded = render_live_lines(&app, 120, 30);
    assert!(expanded.contains("Drafting a document-like plan"));
}

pub(super) fn tool_details_toggle_collapses_successful_tool_payloads() {
    let mut app = rich_transcript_fixture_app();

    let shown = render_live_lines(&app, 120, 30);
    assert!(shown.contains("Read 1 file"));

    run_palette_command(&mut app, "hide tool details");
    let hidden = render_live_lines(&app, 120, 30);
    assert!(!hidden.contains("Read 1 file"));

    run_palette_command(&mut app, "show tool details");
    let restored = render_live_lines(&app, 120, 30);
    assert!(restored.contains("Read 1 file"));
}

pub(super) fn failed_tool_rows_still_surface_error_summary() {
    let mut app = app::AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        Some("req_tool_error"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_tool_error".into(),
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
                tool_call_id: "tc_error".into(),
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
            tool_call_id: "tc_error".into(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        Some("req_tool_error"),
        harness_core::event::EventV1::ToolCallFinished(
            harness_core::event::ToolCallFinishedEvent {
                tool_call_id: "tc_error".into(),
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
    assert!(
        transcript.contains("exit code: 1") && transcript.contains("stderr: permission denied"),
        "failed tool rows must still surface the error summary\n{transcript}"
    );
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

    assert_eq!(app.composer.prompt_buffer, "keep this draft");

    let debug = render_live_buffer(&app, 80, 24);
    assert!(!debug.contains("Composer · disabled · Permission blocked"));
    assert!(debug.contains("Allow Edit to demo.txt?"));
    assert!(debug.contains("Draft preserved"));
    assert!(!debug.contains("Select an activity to view transcript"));
    assert!(
        debug.contains("always-approve") && debug.contains("No, reject"),
        "permission options should remain visible in the modal\n{debug}"
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

    assert_eq!(app.composer.prompt_buffer, "keep this dr");
    assert!(app.active_permission().is_some());

    let debug = render_live_buffer(&app, 80, 24);
    assert!(debug.contains("Draft preserved"));
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

    assert_eq!(app.composer.prompt_buffer, "keep t");
    assert_eq!(
        app.permission_modal_selection("perm_overlay_home_row_input"),
        app::permissions::PermissionModalSelection::AllowAlways
    );

    let debug = render_live_buffer(&app, 80, 24);
    assert!(debug.contains("Draft preserved"));
}
