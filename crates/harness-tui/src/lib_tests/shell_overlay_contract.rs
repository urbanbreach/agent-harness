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
        "startup surface should not render onboarding\n{rendered}"
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
    assert!(rendered.contains("Shift+Tab:mode"));
    assert!(rendered.contains("Ctrl+x:shortcuts"));
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

    assert!(rendered.contains("Shift+Tab:mode"));
    assert!(rendered.contains("Ctrl+x:shortcuts"));
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

pub(super) fn slash_overlay_uses_input_width_aligned_rows_and_accent_selection() {
    let mut app = app::AppState::new_live(None, false, None);
    app.handle_key(exact_test_key(crossterm::event::KeyCode::Char('/')));

    let rendered = render_live_lines(&app, 100, 24);
    let lines = rendered.lines().collect::<Vec<_>>();
    assert!(!rendered.contains("/events"));
    let row = find_line_containing_all(
        &lines,
        &["/agents", "Browse available provider/model options"],
    )
    .unwrap_or_else(|| panic!("slash /agents row\n{rendered}"));
    let agents_description = lines[row]
        .find("Browse available provider/model options")
        .unwrap_or_abort();
    let exit_row = find_line_containing_all(&lines, &["/exit", "Quit the application"])
        .unwrap_or_else(|| panic!("slash /exit row\n{rendered}"));
    let exit_description = lines[exit_row]
        .find("Quit the application")
        .unwrap_or_abort();

    assert_eq!(agents_description, exit_description);
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
    for ch in "resume".chars() {
        palette.handle_key(exact_test_key(crossterm::event::KeyCode::Char(ch)));
    }
    let palette_render = render_live_lines(&palette, width, height);
    assert!(palette_render.contains("Commands"));
    let palette_buffer = render_live_cells(&palette, width, height);
    let (row, fgs, bgs) = row_text_and_palette(&palette_buffer, width, "Resume Session")
        .unwrap_or_else(|| panic!("missing selected overlay row Resume Session"));
    let start_byte = row.find("Resume Session").unwrap_or_abort();
    let start = row[..start_byte].chars().count();
    let end = start + "Resume Session".chars().count();
    let theme = Theme::default();
    assert!(
        bgs[start..end]
            .iter()
            .all(|color| *color == theme.question_prompt.selected),
        "selected palette row uses semantic selection surface\n{row}"
    );
    assert!(
        fgs[start..end]
            .iter()
            .all(|color| *color == theme.text.primary),
        "selected palette row uses semantic primary text\n{row}"
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
            .all(|color| *color == theme.question_prompt.selected),
        "session history selected row uses semantic selection surface\n{sessions_row}"
    );
    assert!(
        sessions_fgs[sessions_start..sessions_end]
            .iter()
            .all(|color| *color == theme.text.primary),
        "session history selected row uses semantic primary text\n{sessions_row}"
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
    let theme = Theme::default();
    assert!(
        commands_bgs[commands_start..commands_end]
            .iter()
            .all(|color| *color == theme.surface.canvas),
        "Commands title uses semantic palette surface\n{commands_row}"
    );
    assert!(
        commands_fgs[commands_start..commands_end]
            .iter()
            .all(|color| *color == theme.text.primary),
        "Commands title uses semantic palette title text\n{commands_row}"
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
            .all(|color| *color == theme.surface.canvas),
        "session history title uses semantic palette surface\n{title_row}"
    );
    assert!(
        title_fgs[title_start..title_end]
            .iter()
            .all(|color| *color == theme.text.primary),
        "session history title uses semantic palette title text\n{title_row}"
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
    assert!(
        bgs[start..end]
            .iter()
            .all(|color| *color == theme.question_prompt.selected),
        "selected allow option should use the question selection background\n{row}"
    );
    assert!(
        fgs[start..end].iter().all(|color| {
            *color == theme.question_prompt.primary || *color == theme.question_prompt.accent
        }),
        "selected allow option should use semantic question text colors\n{row}"
    );
}
