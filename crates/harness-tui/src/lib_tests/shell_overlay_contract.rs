use super::*;

pub(super) fn onboarding_inventory_screens_render_in_startup_surface() {
    for step in app::OnboardingStep::INVENTORY {
        let mut app = app::AppState::new_startup(Vec::new(), None);
        app.set_onboarding_step_for_test(step);
        let screen = app.onboarding_screen().expect("onboarding screen");
        let rendered = render_live_lines(&app, 120, 36);
        assert!(
            rendered.contains("Harness setup"),
            "screen {} should render in the Harness setup frame\n{rendered}",
            step.snapshot_name()
        );
        assert!(
            rendered.contains(screen.title),
            "screen {} should render title {}\n{rendered}",
            step.snapshot_name(),
            screen.title
        );
        assert!(
            !rendered.to_lowercase().contains("reference implementation"),
            "screen {} should use Harness branding only\n{rendered}",
            step.snapshot_name()
        );
    }
}

pub(super) fn replay_mode_never_reports_lifecycle_shell_actions() {
    let replay = app::AppState::new_replay(
        PathBuf::from("/tmp/replay-session"),
        vec![envelope(
            1,
            Some("req_replay_terminal"),
            harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
                summary: "done".to_string(),
            }),
        )],
    );

    assert_eq!(
        replay.lifecycle_shell_state(),
        app::LifecycleShellState::None
    );
    assert!(!replay.startup_shell_visible());
    assert!(!replay.post_run_handoff_visible());
    assert!(!replay.lifecycle_shell_actions_visible());
}

pub(super) fn permission_modal_preempts_palette_and_slash() {
    let mut palette_app = app::AppState::new_live(None, false, None);
    palette_app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    palette_app.handle_key(key(crossterm::event::KeyCode::Char('d')));
    assert!(palette_app.palette_visible);

    palette_app.ingest_event(permission_requested_event(
        1,
        "perm_preempt_palette_and_slash",
        "tool_call_preempt_palette_and_slash",
    ));
    palette_app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    let palette_render = render_live_lines(&palette_app, 100, 24);
    assert!(palette_render.contains("Permission required"));
    assert!(!palette_render.contains("Commands"));
    assert!(!palette_app.palette_visible);
    assert_eq!(
        palette_app.overlay_stack().ordered(),
        &[overlay::OverlayKind::PermissionModal]
    );

    let mut slash_app = app::AppState::new_live(None, false, None);
    slash_app.handle_key(key(crossterm::event::KeyCode::Char('/')));
    assert!(slash_app.slash_visible);

    slash_app.ingest_event(permission_requested_event(
        1,
        "perm_preempt_slash",
        "tool_call_preempt_slash",
    ));
    slash_app.handle_key(key(crossterm::event::KeyCode::Char('/')));

    let slash_render = render_live_lines(&slash_app, 100, 24);
    assert!(slash_render.contains("Permission required"));
    assert!(!slash_render.contains("Slash commands"));
    assert_eq!(slash_app.composer.prompt_buffer, "/");
    assert!(!slash_app.slash_visible);
    assert_eq!(
        slash_app.overlay_stack().ordered(),
        &[overlay::OverlayKind::PermissionModal]
    );
}

pub(super) fn completed_sessions_show_inline_completion_state_instead_of_handoff_card() {
    let mut app = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    app.ingest_event(envelope(
        1,
        Some("req_completed_inline"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    let rendered = render_live_lines(&app, 160, 48);

    assert!(app.completed_session_shell_active());
    assert!(!app.post_run_handoff_visible());
    assert!(rendered.contains("Tab focus"));
    assert!(rendered.contains("Ctrl+p commands"));
    assert!(rendered.contains("q quit"));
    assert!(!rendered.contains("Next action"));
    assert!(!rendered.contains("Continue this session"));
}

pub(super) fn live_shell_uses_single_chrome_path() {
    let ready = app::AppState::new_live(None, false, None);
    assert_live_shell_document_composer_contract(&ready, 100, 24, None, None, "Ctrl+p commands");

    let mut completed = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    completed.ingest_event(envelope(
        1,
        Some("req_completed_single_path"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));
    assert_live_shell_document_composer_contract(&completed, 100, 24, None, None, "Tab focus");

    let mut degraded = app::AppState::new_live(None, false, None);
    degraded.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    assert_live_shell_document_composer_contract(&degraded, 100, 24, None, None, "Degraded");
}

pub(super) fn live_shell_status_strip_has_single_priority_order() {
    let mut orchestration = orchestration_status_strip_fixture();
    orchestration.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
    );
    let mut theme = Theme::default();
    theme.live_shell.primary.details_sidebar_width = 12;
    theme.live_shell.primary.content_margin_x = 2;
    orchestration.set_theme_for_test(theme);

    let orchestration_render = render_live_lines(&orchestration, 140, 40);

    assert!(
        orchestration_render.contains("Current runtime:")
            || orchestration_render.contains("Launch:")
    );

    let mut app = app::AppState::new_live(None, false, None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("deep", "proxy:gpt-5.4").with_mode_label("Demo"),
    );
    app.set_theme_for_test(theme);
    for event in session_view_events() {
        app.ingest_event(event);
    }

    let rendered = render_live_lines(&app, 140, 40);

    assert!(rendered.contains("Ctrl+p commands"));
    assert!(!rendered.contains("Enter send"));
    assert!(!rendered.contains("tool finished"));
    assert!(!rendered.contains("turn 1"));
    assert!(!rendered.contains("ready for next turn"));
}

pub(super) fn legacy_live_redesign_gate_is_removed() {
    let app_src = include_str!("../app.rs");
    let chrome_src = include_str!("../ui_chrome.rs");
    let transcript_src = include_str!("../ui_transcript.rs");

    assert!(!app_src.contains("transcript_first_shell_redesign_active"));
    assert!(!chrome_src.contains("transcript_first_shell_redesign_active"));
    assert!(!transcript_src.contains("transcript_first_shell_redesign_active"));
    assert!(!chrome_src.contains("append_orchestration_status_legacy"));
}

pub(super) fn slash_overlay_uses_reference_navigation_keys() {
    let mut app = app::AppState::new_live(None, false, None);
    app.handle_key(key(crossterm::event::KeyCode::Char('/')));
    assert_eq!(
        app.overlay_stack().top(),
        Some(overlay::OverlayKind::SlashCommands)
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert_eq!(
        app.slash_selected,
        app.slash_filtered.len().saturating_sub(1)
    );

    app.handle_key(key(crossterm::event::KeyCode::Down));
    assert_eq!(app.slash_selected, 0);

    app.handle_key(key(crossterm::event::KeyCode::Up));
    assert_eq!(
        app.slash_selected,
        app.slash_filtered.len().saturating_sub(1)
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('n'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert_eq!(app.slash_selected, 0);

    app.handle_key(key(crossterm::event::KeyCode::Esc));
    assert_eq!(app.composer.prompt_buffer, "");
    assert!(!app.slash_visible);
}

pub(super) fn slash_overlay_uses_input_width_aligned_rows_and_accent_selection() {
    let mut app = app::AppState::new_live(None, false, None);
    app.handle_key(exact_test_key(crossterm::event::KeyCode::Char('/')));

    let rendered = render_live_lines(&app, 100, 24);
    let lines = rendered.lines().collect::<Vec<_>>();
    let row = find_line_containing_all(&lines, &["/events", "Open the review event log surface"])
        .unwrap_or_else(|| panic!("slash /events row\n{rendered}"));
    let events_description = lines[row]
        .find("Open the review event log surface")
        .expect("events description column");
    let new_row = find_line_containing_all(&lines, &["/new", "Return to the home shell"])
        .unwrap_or_else(|| panic!("slash /new row\n{rendered}"));
    let new_description = lines[new_row]
        .find("Return to the home shell")
        .expect("new description column");

    assert_eq!(events_description, new_description);
    assert!(!lines[row].contains('┃'));
    assert!(!rendered.contains('╭') && !rendered.contains('╰') && !rendered.contains('│'));

    let buffer = render_live_cells(&app, 100, 24);
    let selected_command = format!(
        "/{}",
        app.slash_filtered.first().expect("selected slash command")
    );
    let (selected_row, selected_fgs, selected_bgs) =
        row_text_and_palette(&buffer, 100, &selected_command).expect("selected slash row palette");
    let command_start = selected_row
        .find(&selected_command)
        .expect("selected command start");
    let description_start = selected_row
        .find(crate::keybindings::slash_command_description(
            selected_command.trim_start_matches('/'),
        ))
        .expect("selected description start");
    let theme = Theme::default();

    assert_eq!(selected_bgs[command_start], theme.text.accent);
    assert_eq!(selected_bgs[description_start], theme.text.accent);
    assert_eq!(selected_fgs[command_start], theme.text.inverse);
    assert_eq!(selected_fgs[description_start], theme.text.inverse);
}

pub(super) fn new_session_preserves_unsent_draft_across_home_navigation() {
    app::set_pending_live_prompt_draft(Some("draft from home".to_string()));

    let mut startup = app::AppState::new_startup(Vec::new(), None);
    assert_eq!(startup.composer.prompt_buffer, "draft from home");

    startup.handle_key(exact_test_key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "new".chars() {
        startup.handle_key(exact_test_key(crossterm::event::KeyCode::Char(ch)));
    }
    startup.handle_key(exact_test_key(crossterm::event::KeyCode::Enter));
    assert!(startup.should_quit);

    let live = app::AppState::new_live(None, false, None);
    assert_eq!(live.composer.prompt_buffer, "draft from home");
    assert_eq!(
        live.composer.prompt_cursor,
        "draft from home".chars().count()
    );
}

pub(super) fn command_driven_session_switch_emits_correct_ui_intent() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = app::AppState::new_startup(
        vec![exact_test_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
        )],
        Some(sink),
    );

    app.handle_key(exact_test_key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(exact_test_key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(exact_test_key(crossterm::event::KeyCode::Enter));
    app.handle_key(exact_test_key(crossterm::event::KeyCode::Enter));

    assert!(matches!(
        intents.lock().expect("lock intents").last(),
        Some(UiIntent::ContinueSession { run_id, run_dir })
            if run_id == "run_resume" && run_dir.as_path() == Path::new("/tmp/sessions/run_resume")
    ));
}

pub(super) fn overlays_share_elevated_card_language() {
    let width = 120;
    let height = 30;
    let mut palette = app::AppState::new_startup(
        vec![exact_test_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
        )],
        None,
    );
    palette.handle_key(exact_test_key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    let palette_render = render_live_lines(&palette, width, height);
    assert!(palette_render.contains("Commands"));
    assert_selected_overlay_row_uses_highlight(
        &palette,
        width,
        height,
        "New session",
        ratatui::style::Color::Rgb(0xF5, 0xA7, 0x42),
    );

    let mut sessions = app::AppState::new_startup(
        vec![exact_test_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
        )],
        None,
    );
    sessions.handle_key(exact_test_key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        sessions.handle_key(exact_test_key(crossterm::event::KeyCode::Char(ch)));
    }
    sessions.handle_key(exact_test_key(crossterm::event::KeyCode::Enter));
    let sessions_render = render_live_lines(&sessions, width, height);
    assert!(sessions_render.contains("Continue session"));
    assert_selected_overlay_row_uses_highlight(
        &sessions,
        width,
        height,
        "Resume target",
        ratatui::style::Color::Rgb(0xF5, 0xA7, 0x42),
    );
}

pub(super) fn quiet_overlay_helper_rows_use_semantic_chrome_palette() {
    let width = 120;
    let height = 30;

    let mut palette = app::AppState::new_startup(Vec::new(), None);
    palette.handle_key(exact_test_key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    let palette_buffer = render_live_cells(&palette, width, height);
    assert_row_segment_palette(
        &palette_buffer,
        width,
        "Commands",
        ratatui::style::Color::Rgb(0xEE, 0xEE, 0xEE),
        ratatui::style::Color::Rgb(0x14, 0x14, 0x14),
    );

    let mut sessions = app::AppState::new_startup(
        vec![exact_test_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
        )],
        None,
    );
    sessions.handle_key(exact_test_key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        sessions.handle_key(exact_test_key(crossterm::event::KeyCode::Char(ch)));
    }
    sessions.handle_key(exact_test_key(crossterm::event::KeyCode::Enter));
    let sessions_buffer = render_live_cells(&sessions, width, height);
    assert_row_segment_palette(
        &sessions_buffer,
        width,
        "Continue session",
        ratatui::style::Color::Rgb(0xEE, 0xEE, 0xEE),
        ratatui::style::Color::Rgb(0x14, 0x14, 0x14),
    );
}

pub(super) fn live_shell_redesign_preserves_replay_overlay_and_permission_parity() {
    startup_and_live_empty_share_spacing_contract();
    compact_geometry_uses_overlay_sidebar_and_minimal_footer();
    hovered_wheel_target_uses_sidebar_overlay_hit_areas();
    permission_modal_remains_visually_dominant_and_fail_closed();

    let theme = Theme::default();

    let mut replay =
        app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), session_view_events());
    replay.transcript_view.transcript_scroll = usize::MAX;
    let replay_plan = FrameLayoutPlan::for_app(&replay, ratatui::layout::Rect::new(0, 0, 100, 30));
    let replay_render = render_live_lines(&replay, 100, 30);
    let replay_buffer = render_live_cells(&replay, 100, 30);
    let replay_lines = replay_render.lines().collect::<Vec<_>>();
    assert!(replay_plan.live_anchor.is_none());
    assert!(replay_plan.operator_sidebar.is_some());
    let replay_header_row = find_line_containing_all(&replay_lines, &["Replay", "read-only"])
        .unwrap_or_else(|| {
            panic!("replay header should preserve replay identity\n{replay_render}")
        });
    let replay_disabled_row = find_line_containing_all_from(
        &replay_lines,
        replay_header_row + 1,
        &["▎", "Replay is read-only."],
    )
    .filter(|row| !replay_lines[*row].contains("run "))
    .unwrap_or_else(|| {
        panic!("replay shell should preserve a disabled composer row\n{replay_render}")
    });
    let replay_shortcuts_row =
        find_line_containing_from(&replay_lines, replay_disabled_row + 1, "shortcuts")
            .unwrap_or_else(|| {
                panic!("replay shell should preserve shortcut guidance\n{replay_render}")
            });
    let user_row = find_line_containing(&replay_lines, "Explain the refactor")
        .unwrap_or_else(|| panic!("replay shell should preserve the user turn\n{replay_render}"));
    let thinking_row =
        find_line_containing_all_from(&replay_lines, user_row + 1, &["Working through the steps."])
            .unwrap_or_else(|| {
                panic!("replay shell should preserve visible thinking text\n{replay_render}")
            });

    assert!(replay_header_row < replay_disabled_row && replay_disabled_row < replay_shortcuts_row);
    assert!(
        user_row < thinking_row,
        "replay transcript should preserve turn order\n{replay_render}"
    );
    assert_alphanumeric_row_palette(
        &replay_buffer,
        100,
        replay_disabled_row,
        theme.status.disabled,
        theme.surface.shell,
        "replay disabled composer",
    );
    assert_row_segment_palette(
        &replay_buffer,
        100,
        "? shortcuts",
        theme.text.secondary,
        theme.surface.shell,
    );

    let mut degraded = app::AppState::new_live(None, false, None);
    degraded.set_status_banner(Some(
        "live stream lagged by 2; replaying from seq 1".to_string(),
    ));
    let degraded_buffer = render_live_cells(&degraded, 80, 24);
    assert_row_segment_background(
        &degraded_buffer,
        80,
        "Recovery in progress",
        theme.surface.overlay,
    );

    let mut disconnected = app::AppState::new_live(None, false, None);
    disconnected.set_status_banner(Some("live event stream disconnected".to_string()));
    let disconnected_buffer = render_live_cells(&disconnected, 80, 24);
    assert_row_segment_background(
        &disconnected_buffer,
        80,
        "Connection lost",
        theme.surface.overlay,
    );

    let mut failure = app::AppState::new_live(None, false, None);
    failure.set_status_banner(Some(
        "runtime error: exit code 1\nstderr permission denied".to_string(),
    ));
    let failure_buffer = render_live_cells(&failure, 80, 24);
    assert_row_segment_background(
        &failure_buffer,
        80,
        "Review required",
        theme.surface.overlay,
    );
}

pub(super) fn permission_modal_remains_visually_dominant_and_fail_closed() {
    let mut app = app::AppState::new_live(None, false, None);
    app.handle_key(exact_test_key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.ingest_event(permission_requested_event(
        1,
        "perm_dominant_fail_closed",
        "tool_call_dominant_fail_closed",
    ));

    let rendered = render_live_lines(&app, 100, 24);
    let buffer = render_live_cells(&app, 100, 24);
    let theme = Theme::default();
    let (row, _, bgs) = row_text_and_palette(&buffer, 100, "Allow once").expect("allow chip row");
    let start_byte = row.find("Allow once").expect("chip substring");
    let start = row[..start_byte].chars().count();
    let end = start + "Allow once".chars().count();

    assert_eq!(
        app.overlay_stack().ordered(),
        &[overlay::OverlayKind::PermissionModal]
    );
    assert!(!app.palette_visible);
    assert!(rendered.contains("Permission required"));
    assert!(rendered.contains("Allow once"));
    assert!(rendered.contains("Allow always"));
    assert!(rendered.contains("enter"));
    assert!(rendered.contains("⇆"));
    assert!(!rendered.contains("Commands"));
    assert!(
        bgs[start..end]
            .iter()
            .all(|color| *color == theme.status.warning),
        "selected allow chip should stay stronger than quiet command overlays\n{row}"
    );
}
