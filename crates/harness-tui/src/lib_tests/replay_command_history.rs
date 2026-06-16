use super::*;

pub(super) fn replay_read_only_copy_matches_operator_shell_contract() {
    let app =
        app::AppState::new_replay(PathBuf::from("/tmp/replay-session"), session_view_events());

    let rendered = render_live_lines(&app, 100, 24);

    assert!(rendered.contains("Replay · read-only"));
    assert!(rendered.contains("Replay is read-only"));
    assert!(rendered.contains("▼ MCP"));
    assert!(rendered.contains("▶ Modified Files"));
    assert!(rendered.contains("r reload"));
    assert!(rendered.contains("q quit"));
    assert!(!rendered.contains("Tab nav"));
    assert!(
        !rendered.contains("Inspect the transcript, event log, or diff, then press r to reload.")
    );
}

pub(super) fn replay_shell_is_read_only_without_tab_bar() {
    replay_shell_uses_read_only_operator_layout();
}

pub(super) fn command_palette_groups_commands_for_shell() {
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
            true,
            None,
        )],
        None,
    );
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    let rendered = render_live_lines(&app, 120, 30);
    assert!(rendered.contains("Commands"));
    assert!(rendered.contains("Continue session"));

    let mut live_app = app::AppState::new_live(None, false, None);
    live_app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "toggle".chars() {
        live_app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    let filtered = render_live_lines(&live_app, 120, 30);
    assert!(filtered.contains("Commands"));
    assert!(filtered.contains("Toggle follow"));

    let mut system_app = app::AppState::new_startup(Vec::new(), None);
    system_app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "quit".chars() {
        system_app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    let system_render = render_live_lines(&system_app, 120, 30);
    assert!(system_render.contains("Commands"));
    assert!(system_render.contains("Quit"));

    let mut help_app = app::AppState::new_startup(Vec::new(), None);
    help_app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "help".chars() {
        help_app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    let help_render = render_live_lines(&help_app, 120, 30);
    assert!(help_render.contains("Commands"));
    assert!(help_render.contains("Help"));
}

pub(super) fn session_switcher_groups_entries_by_recency() {
    let entries = vec![
        startup_session_entry_with_details(
            "run_older",
            "/tmp/sessions/run_older",
            "older-run",
            Some(harness_core::proj::RunStatus::Finished),
            Some("2026-02-14T08:30:00Z"),
            "deep",
            "openai/gpt-5.4-mini",
            true,
            None,
        ),
        startup_session_entry_with_details(
            "run_yesterday",
            "/tmp/sessions/run_yesterday",
            "yesterday-run",
            Some(harness_core::proj::RunStatus::Finished),
            Some(&test_timestamp_days_ago(1, "21:15")),
            "ops",
            "anthropic/claude-3.7",
            true,
            None,
        ),
        startup_session_entry_with_details(
            "run_today",
            "/tmp/sessions/run_today",
            "today-run",
            Some(harness_core::proj::RunStatus::Running),
            Some(&test_timestamp_days_ago(0, "09:45")),
            "worker",
            "mock/model-1",
            true,
            None,
        ),
    ];
    let mut app = app::AppState::new_startup(entries, None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert_eq!(
        app.session_history_filtered
            .iter()
            .map(|index| app.session_history_entries[*index].catalog.run_id.as_str())
            .collect::<Vec<_>>(),
        vec!["run_today", "run_yesterday", "run_older"]
    );

    let rendered = render_live_lines(&app, 120, 30);
    assert!(rendered.contains("Continue session"));
    assert!(rendered.contains("today-run"));
    assert!(rendered.contains("yesterday-run"));
    assert!(rendered.contains("older-run"));
}

pub(super) fn session_history_overlay_sorts_results_deterministically() {
    session_switcher_groups_entries_by_recency();
}

pub(super) fn footer_shortcuts_collapse_without_overlap() {
    lifecycle_shell_narrow_layout_renders_primary_cta();
}

pub(super) fn slash_commands_only_track_leading_slash_input() {
    let mut plain = app::AppState::new_live(None, false, None);
    plain.handle_key(key(crossterm::event::KeyCode::Char('h')));
    assert!(!plain.slash_visible);

    let mut app = app::AppState::new_live(None, false, None);
    app.handle_key(key(crossterm::event::KeyCode::Char('/')));
    assert!(app.slash_visible);
    assert_eq!(
        app.overlay_stack().top(),
        Some(overlay::OverlayKind::SlashCommands)
    );

    app.handle_key(key(crossterm::event::KeyCode::Char('h')));
    assert!(app.slash_visible);

    let mut non_leading = app::AppState::new_live(None, false, None);
    for ch in "hi/there".chars() {
        non_leading.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert!(!non_leading.slash_visible);
}
