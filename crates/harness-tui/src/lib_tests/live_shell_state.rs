use super::*;
use crate::UnwrapOrAbort;

pub(super) fn live_shell_omits_irrelevant_idle_shortcuts() {
    let mut live = app::AppState::new_live(None, false, None);
    live.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("continued"),
    );

    let primary_render = render_live_lines(&live, 100, 30);
    assert!(!primary_render.contains("q quit"));
    assert!(
        !primary_render.contains("Shift+Tab:mode") && !primary_render.contains("Ctrl+x:shortcuts"),
        "idle live shell must leave transcript space to the conversation\n{primary_render}"
    );
    assert!(
        !primary_render.contains("live ctx"),
        "live idle disclosure must not show live-ctx cluster\n{primary_render}"
    );

    let reduced_render = render_live_lines(&live, 80, 24);
    assert!(!reduced_render.contains("q quit"));
    assert!(!reduced_render.contains("Shift+Tab:mode"));
    assert!(!reduced_render.contains("Ctrl+x:shortcuts"));

    let minimal_render = render_live_lines(&live, 60, 18);
    assert!(!minimal_render.contains("q quit"));
    assert!(!minimal_render.contains("Shift+Tab:mode"));
    assert!(!minimal_render.contains("Ctrl+x:shortcuts"));

    let replay =
        app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), session_view_events());
    let replay_render = render_live_lines(&replay, 100, 24);
    let replay_lines = replay_render.lines().collect::<Vec<_>>();
    let replay_footer_row = find_last_line_containing(&replay_lines, "q quit")
        .map(|row| replay_lines[row].trim_end().to_string())
        .unwrap_or_abort();
    assert_markers_in_order(&replay_footer_row, &["shortcuts", "tab focus", "q quit"]);
    assert!(!replay_footer_row.contains("Replay"));
    assert!(!replay_footer_row.contains("run_fixture"));
    assert!(!replay_footer_row.contains("/tmp/replay-session"));
    assert!(!replay_footer_row.contains("/status"));
}

pub(super) fn live_post_turn_disclosure_appears_only_for_a_draft() {
    // Given: live post-turn shell (not startup), empty draft — freeze run1-stream-probe footer
    let mut app = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        app.ingest_event(event);
    }
    assert!(!app.startup_shell_visible());

    // When: shell is rendered at freeze-primary geometry
    let rendered = render_live_lines(&app, 120, 40);
    assert!(!rendered.contains("Shift+Tab:mode"));
    assert!(!rendered.contains("Ctrl+x:shortcuts"));

    // Given: same shell with a non-empty draft — freeze run1-draft footer
    for ch in "Browser QA draft".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    let draft_rendered = render_live_lines(&app, 120, 40);
    let draft_lines = draft_rendered.lines().collect::<Vec<_>>();
    let draft_disclosure = draft_lines
        .iter()
        .rposition(|line| line.contains("Shift+Tab") || line.contains("Enter:send"))
        .map(|idx| draft_lines[idx].trim_end().to_string())
        .unwrap_or_else(|| {
            panic!("draft freeze shortcut disclosure row missing\n{draft_rendered}")
        });
    assert!(
        draft_disclosure.contains("Enter:send"),
        "draft disclosure must lead with Enter:send\n{draft_disclosure}\n{draft_rendered}"
    );
    assert!(
        draft_disclosure.contains("Shift+Tab:mode") && draft_disclosure.contains("Ctrl+x:shortcuts"),
        "draft disclosure must keep freeze mode/shortcuts chrome\n{draft_disclosure}\n{draft_rendered}"
    );
    assert!(
        !draft_disclosure.contains("live ctx") && !draft_disclosure.contains("? commands"),
        "draft disclosure must not show live-ctx / ? commands cluster\n{draft_disclosure}\n{draft_rendered}"
    );
}

pub(super) fn primary_and_wide_live_shells_hide_metadata_header() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
    );
    for event in session_view_events() {
        app.ingest_event(event);
    }

    for (width, height) in [(100, 30), (160, 48)] {
        let plan = FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, width, height));
        assert_eq!(
            plan.header.height, 0,
            "live shell header should stay hidden at {width}x{height}"
        );
        assert!(plan.live_anchor.is_none());

        let rendered = render_live_lines(&app, width, height);
        assert!(
            !rendered.contains("Composer ·"),
            "wide live shells should not reintroduce composer label chrome\n{rendered}"
        );
        assert!(
            !rendered
                .lines()
                .next()
                .unwrap_or_default()
                .contains("run run_fixture"),
            "wide live shells should not surface the old top identity bar\n{rendered}"
        );
    }
}

pub(super) fn completed_shell_bottom_rows_do_not_duplicate_command_help_footers() {
    let mut app = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    app.active_review_surface = Some(app::ReviewSurface::Help);
    app.focus = app::Focus::Prompt;
    app.composer.prompt_buffer = "keep this draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.ingest_event(envelope(
        1,
        Some("req_completed_decrowded_footer"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    let rendered = render_live_lines(&app, 100, 24);
    let lines = rendered.lines().collect::<Vec<_>>();
    let footer_row = find_last_line_containing(&lines, "Ctrl+x:shortcuts").unwrap_or_abort();

    assert_eq!(
        lines
            .iter()
            .filter(|line| line.contains("Ctrl+x:shortcuts"))
            .count(),
        1,
        "completed shell should keep a single footer hint row\n{rendered}"
    );
    assert!(lines[footer_row].contains("Shift+Tab:mode"));
    assert!(!lines[footer_row].contains("? commands"));
}

pub(super) fn live_state_matrix_preserves_shell_structure() {
    let mut ready = app::AppState::new_live(None, false, None);
    assert_live_shell_document_composer_contract(&ready, 100, 24, None, None, "Shift+Tab:mode");

    for ch in "draft next turn".chars() {
        ready.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_live_shell_document_composer_contract(
        &ready,
        100,
        24,
        Some("draft next turn"),
        None,
        "Shift+Tab:mode",
    );

    let mut multiline = app::AppState::new_live(None, false, None);
    for ch in "draft".chars() {
        multiline.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    multiline.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::SHIFT,
    ));
    for ch in "second line".chars() {
        multiline.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_live_shell_document_composer_contract(
        &multiline,
        100,
        24,
        Some("draft"),
        None,
        "Shift+Tab:mode",
    );

    let mut streaming = app::AppState::new_live(None, false, None);
    streaming.ingest_event(envelope(
        1,
        Some("req_streaming_matrix"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_streaming_matrix".into(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "streaming".to_string(),
                request_digest: "digest-streaming".to_string(),
                metadata: None,
            },
        ),
    ));
    streaming.ingest_event(envelope(
        2,
        Some("req_streaming_matrix"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_streaming_matrix".into(),
                delta: "partial output".to_string(),
            },
        ),
    ));
    assert_live_shell_document_composer_contract(&streaming, 100, 24, None, None, "Shift+Tab:mode");

    let mut degraded = app::AppState::new_live(None, false, None);
    degraded.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    assert_live_shell_document_composer_contract(&degraded, 100, 24, None, None, "Degraded");

    let mut disconnected = app::AppState::new_live(None, false, None);
    disconnected.set_status_banner(Some("live event stream disconnected".to_string()));
    assert_live_shell_document_composer_contract(
        &disconnected,
        100,
        24,
        None,
        None,
        "Disconnected",
    );

    let mut failure = app::AppState::new_live(None, false, None);
    failure.set_status_banner(Some("runtime error while updating session".to_string()));
    assert_live_shell_document_composer_contract(&failure, 100, 24, None, None, "Failure");

    let mut completed = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    completed.ingest_event(envelope(
        1,
        Some("req_completed_matrix"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));
    assert_live_shell_document_composer_contract(&completed, 100, 24, None, None, "Tab focus");
}
