use super::*;
use crate::UnwrapOrAbort;

pub(super) fn startup_surface_renders_without_onboarding_overlay() {
    let app = app::AppState::new_startup(Vec::new(), None);
    let rendered = render_live_lines(&app, 120, 36);
    assert!(
        !rendered.contains("Harness setup"),
        "startup surface should not render onboarding setup frame after migration\n{rendered}"
    );
    assert!(
        !rendered.to_lowercase().contains("onboarding"),
        "startup surface should not reference onboarding\n{rendered}"
    );
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
    assert!(palette_render.contains("Allow Edit"));
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
    assert!(slash_render.contains("Allow Edit"));
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
    assert!(
        rendered.contains("commands") && rendered.contains("quit"),
        "completed shell uses focus/commands/quit disclosure, not live Shift+Tab:mode chrome\n{rendered}"
    );
    assert!(!rendered.contains("Next action"));
    assert!(!rendered.contains("Continue this session"));
}

pub(super) fn live_shell_uses_single_chrome_path() {
    let ready = app::AppState::new_live(None, false, None);
    assert_live_shell_document_composer_contract(&ready, 100, 24, None, None, "Shift+Tab:mode");

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
            || orchestration_render.contains("gpt-5")
            || orchestration_render.contains("deep")
            || orchestration_render.contains('❯'),
        "status strip or composer chrome should surface runtime identity\n{orchestration_render}"
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

    assert!(rendered.contains("Shift+Tab:mode") || rendered.contains("Ctrl+x:shortcuts"));
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
    assert!(!rendered.contains("/events"));
    let row = find_line_containing_all(&lines, &["/feedback", "Show shortcuts and TUI controls"])
        .unwrap_or_else(|| panic!("slash /feedback row\n{rendered}"));
    let feedback_description = lines[row]
        .find("Show shortcuts and TUI controls")
        .unwrap_or_abort();
    let exit_row = find_line_containing_all(&lines, &["/exit", "Quit the application"])
        .unwrap_or_else(|| panic!("slash /exit row\n{rendered}"));
    let exit_description = lines[exit_row]
        .find("Quit the application")
        .unwrap_or_abort();

    assert_eq!(feedback_description, exit_description);
    assert!(!lines[row].contains('┃'));
    assert!(
        !lines[row].contains('╭') && !lines[row].contains('╰'),
        "slash rows stay unboxed even when the live composer is bordered\n{rendered}"
    );

    let buffer = render_live_cells(&app, 100, 24);
    let selected_command = format!("/{}", app.slash_filtered.first().unwrap_or_abort());
    let (selected_row, selected_fgs, selected_bgs) =
        row_text_and_palette(&buffer, 100, &selected_command).unwrap_or_abort();
    let command_start = selected_row.find(&selected_command).unwrap_or_abort();
    let description_start = selected_row
        .find(crate::keybindings::slash_command_description(
            selected_command.trim_start_matches('/'),
        ))
        .unwrap_or_abort();
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
            intents.lock().unwrap_or_abort().push(intent);
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
        intents.lock().unwrap_or_abort().last(),
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
    let palette_buffer = render_live_cells(&palette, width, height);
    let (row, fgs, bgs) = row_text_and_palette(&palette_buffer, width, "Resume Session")
        .unwrap_or_else(|| panic!("missing selected overlay row Resume Session"));
    let start_byte = row.find("Resume Session").unwrap_or_abort();
    let start = row[..start_byte].chars().count();
    let end = start + "Resume Session".chars().count();
    assert!(
        bgs[start..end]
            .iter()
            .all(|color| *color == ratatui::style::Color::Indexed(0)
                || *color == ratatui::style::Color::Indexed(7)),
        "selected palette row uses freeze Indexed(0) or Indexed(7) surface, not inverse card fill\n{row}"
    );
    assert!(
        fgs[start..end]
            .iter()
            .all(|color| *color == ratatui::style::Color::Indexed(0)
                || *color == ratatui::style::Color::Indexed(15)),
        "selected palette row keeps primary text on Indexed(0) surface\n{row}"
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
    assert!(
        sessions_render.contains("Continue session")
            || sessions_render.contains("Resume session")
            || sessions_render.contains("Resume target")
    );
    let sessions_buffer = render_live_cells(&sessions, width, height);
    let (sessions_row, sessions_fgs, sessions_bgs) =
        row_text_and_palette(&sessions_buffer, width, "Resume target")
            .unwrap_or_else(|| panic!("missing selected session history row Resume target"));
    let sessions_start_byte = sessions_row.find("Resume target").unwrap_or_abort();
    let sessions_start = sessions_row[..sessions_start_byte].chars().count();
    let sessions_end = sessions_start + "Resume target".chars().count();
    assert!(
        sessions_bgs[sessions_start..sessions_end]
            .iter()
            .all(|color| *color == ratatui::style::Color::Reset
                || *color == ratatui::style::Color::Indexed(0)
                || *color == ratatui::style::Color::Indexed(7)
                || *color == ratatui::style::Color::Rgb(0xD9, 0x84, 0xD9)
                || *color == ratatui::style::Color::Rgb(0x0B, 0x0E, 0x14)
                || *color == ratatui::style::Color::Rgb(0x12, 0x16, 0x1E)),
        "session history selected row uses freeze surface chrome\n{sessions_row}"
    );
    assert!(
        sessions_fgs[sessions_start..sessions_end]
            .iter()
            .all(|color| *color == ratatui::style::Color::Reset
                || *color == ratatui::style::Color::Indexed(15)
                || *color == ratatui::style::Color::Indexed(7)
                || *color == ratatui::style::Color::Rgb(0xD7, 0xDA, 0xE0)
                || *color == ratatui::style::Color::Rgb(0x0B, 0x0E, 0x14)),
        "session history selected row keeps readable text chrome\n{sessions_row}"
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
    let (commands_row, commands_fgs, commands_bgs) =
        row_text_and_palette(&palette_buffer, width, "Commands")
            .unwrap_or_else(|| panic!("missing Commands title row"));
    let commands_start = commands_row[..commands_row.find("Commands").unwrap_or_abort()]
        .chars()
        .count();
    let commands_end = commands_start + "Commands".chars().count();
    assert!(
        commands_bgs[commands_start..commands_end]
            .iter()
            .all(|color| *color == ratatui::style::Color::Indexed(7)),
        "Commands title uses freeze Indexed(7) surface\n{commands_row}"
    );
    assert!(
        commands_fgs[commands_start..commands_end]
            .iter()
            .all(|color| *color == ratatui::style::Color::Indexed(0)),
        "Commands title uses Indexed(0) text on Indexed(7) surface\n{commands_row}"
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
    let sessions_render = render_live_lines(&sessions, width, height);
    let title_needle = if sessions_render.contains("Continue session") {
        "Continue session"
    } else if sessions_render.contains("Resume session") {
        "Resume session"
    } else {
        "Resume Session"
    };
    let (title_row, title_fgs, title_bgs) =
        row_text_and_palette(&sessions_buffer, width, title_needle)
            .unwrap_or_else(|| panic!("missing session history title row {title_needle}"));
    let title_start = title_row[..title_row.find(title_needle).unwrap_or_abort()]
        .chars()
        .count();
    let title_end = title_start + title_needle.chars().count();
    assert!(
        title_bgs[title_start..title_end]
            .iter()
            .all(|color| *color == ratatui::style::Color::Indexed(7)),
        "session history title uses freeze Indexed(7) surface\n{title_row}"
    );
    assert!(
        title_fgs[title_start..title_end]
            .iter()
            .all(|color| *color == ratatui::style::Color::Indexed(0)),
        "session history title uses Indexed(0) text on Indexed(7) surface\n{title_row}"
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
        ratatui::style::Color::Reset,
        "replay disabled composer",
    );
    assert_row_segment_palette(
        &replay_buffer,
        100,
        "h shortcuts",
        theme.text.secondary,
        ratatui::style::Color::Reset,
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
    let failure_render = render_live_lines(&failure, 80, 24);
    // Freeze-aligned: Failure no longer paints elevated overlay chrome.
    assert!(!failure_render.contains("Review required"));
    assert_eq!(failure.runtime_state().kind, app::RuntimeStateKind::Failure);
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
    let selected_marker = "1 (●) Yes, and don't ask again for anything (always-approve mode)";
    let (row, fgs, bgs) = row_text_and_palette(&buffer, 100, selected_marker).unwrap_or_abort();
    let start_byte = row.find(selected_marker).unwrap_or_abort();
    let start = row[..start_byte].chars().count();
    let end = start + selected_marker.chars().count();

    assert_eq!(
        app.overlay_stack().ordered(),
        &[overlay::OverlayKind::PermissionModal]
    );
    assert!(!app.palette_visible);
    assert!(rendered.contains("Allow Edit"));
    assert!(rendered.contains("always-approve"));
    assert!(rendered.contains("No, reject"));
    assert!(
        rendered.contains("Ctrl+o:always-approve")
            || rendered.contains("Ctrl+c:cancel")
            || rendered.contains("enter:confirm")
            || rendered.contains("Ctrl+n:deny")
            || rendered.contains("esc:cancel")
    );
    assert!(!rendered.contains("Commands"));
    // Grok freeze: selected option is distinguished by the filled marker only, with the same
    // card-surface background and primary text as unselected options.
    assert!(
        bgs[start..end]
            .iter()
            .all(|color| *color == theme.surface.card),
        "selected allow option should use the card surface background\n{row}"
    );
    assert!(
        fgs[start..end]
            .iter()
            .all(|color| *color == theme.text.primary),
        "selected allow option should use primary text color\n{row}"
    );
}
