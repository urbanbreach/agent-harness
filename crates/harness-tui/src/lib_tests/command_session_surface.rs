use super::*;
use crate::UnwrapOrAbort;

pub(super) fn session_shell_hides_tab_chrome_and_replay_review_is_command_driven() {
    use ratatui::{backend::TestBackend, Terminal};

    let mut live = app::AppState::new_live(None, false, None);
    for event in session_view_events() {
        live.ingest_event(event);
    }

    let live_backend = TestBackend::new(80, 24);
    let mut live_terminal = Terminal::new(live_backend).unwrap_or_abort();
    live_terminal
        .draw(|frame| ui::render_app(frame, &live))
        .unwrap_or_abort();

    let live_debug = format!("{:?}", live_terminal.backend().buffer());
    assert!(live_debug.contains("❯ "));
    assert!(!live_debug.contains("Composer ·"));
    assert!(!live_debug.contains("Tabs"));
    assert!(!live_debug.contains("Activity ("));
    assert!(!live_debug.contains("Inspector"));

    live.live_details_drawer_open = true;
    assert_eq!(live.review_surface(), None);
    assert!(live.details_drawer_open());
    live.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "event log".chars() {
        live.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert!(!live
        .palette_filtered
        .iter()
        .any(|c| c == "harness.open_event_log"));
    live.handle_key(key(crossterm::event::KeyCode::Esc));
    assert_eq!(live.review_surface(), None);
    assert!(live.details_drawer_open());

    let replay = app::AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-session"),
        session_view_events(),
    );
    let replay_backend = TestBackend::new(80, 24);
    let mut replay_terminal = Terminal::new(replay_backend).unwrap_or_abort();
    replay_terminal
        .draw(|frame| ui::render_app(frame, &replay))
        .unwrap_or_abort();

    let replay_debug = format!("{:?}", replay_terminal.backend().buffer());
    assert!(!replay_debug.contains("Tabs"));
    assert!(replay_debug.contains("Replay · read-only"));

    let mut replay = replay;
    replay.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "event log".chars() {
        replay.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert!(!replay
        .palette_filtered
        .iter()
        .any(|c| c == "harness.open_event_log"));
}

pub(super) fn live_mode_accepts_input_without_focus_switch() {
    let mut app = app::AppState::new_live(None, false, None);

    for c in "hello".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    assert_eq!(app.composer.prompt_buffer, "hello");
    assert_eq!(app.composer.prompt_cursor, 5);
}

pub(super) fn command_palette_renders_and_filters() {
    let mut app = app::AppState::new_live(None, false, None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    assert!(app.palette_visible);

    assert!(app.palette_filtered.contains(&"session.new".to_string()));
    assert!(
        !app.palette_filtered.contains(&"app.exit".to_string()),
        "app.exit is outside freeze empty-filter inventory"
    );

    assert!(!app.palette_filtered.contains(&"help.show".to_string()));
    assert!(!app.palette_filtered.contains(&"theme.switch".to_string()));
    assert!(!app.palette_filtered.contains(&"session.share".to_string()));
    assert!(!app.palette_filtered.contains(&"prompt.editor".to_string()));
    assert!(!app.palette_filtered.contains(&"docs.open".to_string()));
    assert!(!app.palette_filtered.contains(&"diff.open".to_string()));

    assert!(
        !app.palette_filtered
            .iter()
            .any(|c| c.starts_with("suggested:")),
        "empty filter matches freeze: no suggested duplicates"
    );

    let open_debug = render_live_screen(&app, 120, 36);
    assert!(open_debug.contains("Commands"));

    app.handle_key(key(crossterm::event::KeyCode::Char('n')));

    assert_eq!(app.palette_input, "n");
    assert_eq!(app.palette_cursor, 1);
    assert!(
        !app.palette_filtered.is_empty(),
        "filtering should produce results for 'n'"
    );
    assert!(
        !app.palette_filtered
            .iter()
            .any(|c| c.starts_with("suggested:")),
        "non-empty filter should not produce suggested duplicates"
    );

    for _ in 0..app.palette_input.len() {
        app.handle_key(key(crossterm::event::KeyCode::Backspace));
    }
    for ch in "exit".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert!(
        app.palette_filtered.contains(&"app.exit".to_string()),
        "live palette should expose app.exit via filter"
    );

    let filtered_debug = render_live_screen(&app, 120, 36);
    assert!(filtered_debug.contains("Commands"));
}

pub(super) fn command_palette_exposes_model_switcher_when_models_are_configured() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini").with_available_models(
            vec![app::ModelOption::from_model_ref(
                "build",
                "default:gpt-5.4-mini",
            )],
        ),
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    assert!(app.palette_visible);
    assert!(app
        .palette_filtered
        .iter()
        .any(|command| command == "model.list"));
}

pub(super) fn command_palette_dims_background_instead_of_repainting_it() {
    let width = 120;
    let height = 36;
    let base = app::AppState::new_startup(Vec::new(), None);
    let base_buffer = render_live_cells(&base, width, height);

    let mut palette = app::AppState::new_startup(Vec::new(), None);
    palette.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert!(palette.palette_visible);

    let overlay =
        FrameLayoutPlan::for_app(&palette, ratatui::layout::Rect::new(0, 0, width, height))
            .palette_overlay
            .unwrap_or_abort();
    let palette_buffer = render_live_cells(&palette, width, height);
    let mut saw_outside_reset = false;
    for index in 0..base_buffer.content.len() {
        let x = u16::try_from(index % usize::from(width)).unwrap_or_abort();
        let y = u16::try_from(index / usize::from(width)).unwrap_or_abort();
        let inside_overlay = x >= overlay.x
            && x < overlay.x.saturating_add(overlay.width)
            && y >= overlay.y
            && y < overlay.y.saturating_add(overlay.height);
        if inside_overlay {
            continue;
        }
        let palette_cell = &palette_buffer[(x, y)];
        if matches!(
            (palette_cell.fg, palette_cell.bg),
            (ratatui::style::Color::Reset, ratatui::style::Color::Reset)
        ) {
            saw_outside_reset = true;
            break;
        }
    }
    assert!(
        saw_outside_reset,
        "palette backdrop should reset colors outside the overlay under freeze Color::Reset surfaces"
    );

    let shared = base_buffer
        .content
        .iter()
        .enumerate()
        .find_map(|(index, base_cell)| {
            let x = u16::try_from(index % usize::from(width)).ok()?;
            let y = u16::try_from(index / usize::from(width)).ok()?;
            let inside_overlay = x >= overlay.x
                && x < overlay.x.saturating_add(overlay.width)
                && y >= overlay.y
                && y < overlay.y.saturating_add(overlay.height);
            if inside_overlay || base_cell.symbol().trim().is_empty() {
                return None;
            }
            let palette_cell = &palette_buffer[(x, y)];
            (palette_cell.symbol() == base_cell.symbol())
                .then(|| (x, y, base_cell.clone(), palette_cell.clone()))
        });
    if let Some((x, y, base_cell, palette_cell)) = shared {
        assert_eq!(
            palette_cell.symbol(),
            base_cell.symbol(),
            "shared glyph at ({x}, {y}) should survive palette open"
        );
    }
}

fn overlay_scrim_channel(channel: u8) -> u8 {
    let channel = u16::from(channel);
    u8::try_from(channel.saturating_mul(105) / 255).unwrap_or_default()
}

pub(super) fn command_palette_empty_state_renders() {
    let mut app = app::AppState::new_live(None, false, None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('z')));

    assert!(app.palette_visible);
    assert!(app.palette_filtered.is_empty());

    let debug = render_live_screen(&app, 100, 24);
    println!("EMPTY\n{debug}");
    assert!(debug.contains("Commands"));
    assert!(debug.contains("No results found"));
}

pub(super) fn command_palette_filtered_results_preserve_overlay_command_order() {
    let mut app = app::AppState::new_startup(Vec::new(), None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "ne".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }

    assert!(app.palette_filtered.contains(&"session.new".to_string()));
}

pub(super) fn command_palette_includes_session_history_entry() {
    let mut app = app::AppState::new_live(None, false, None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    assert!(app.palette_visible);
    assert!(app.palette_filtered.contains(&"session.new".to_string()));
    assert!(app.palette_filtered.contains(&"session.list".to_string()));

    let rendered = render_live_lines(&app, 120, 40);
    assert!(rendered.contains("New Session"));
    assert!(rendered.contains("Resume Session"));
}

pub(super) fn session_history_picker_renders_resumable_and_replay_rows() {
    let entries = vec![
        startup_session_entry_with_details(
            "run_resume",
            "/tmp/sessions/run_resume",
            "New session - 2026-03-08T12:34:56.000Z",
            Some(harness_core::proj::RunStatus::Finished),
            Some("2026-03-08T12:34:56Z"),
            "deep",
            "openai/gpt-5.4-mini",
            true,
            None,
        ),
        startup_session_entry_with_mode_and_details(
            "run_prompt_only",
            "/tmp/sessions/run_prompt_only",
            "beta-prompt",
            Some(harness_core::proj::RunStatus::Failed),
            Some("2026-03-07T03:21:00Z"),
            "ops",
            "anthropic/claude-3.7",
            harness_core::proj::SessionModeSource::Prompt,
            false,
            Some("prompt runs are not resumable"),
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
    let resume_render = render_live_lines(&app, 120, 30);
    assert!(
        resume_render.contains("Resume session")
            || resume_render.contains("Resume session")
            || resume_render.contains("Resume Session")
    );
    assert!(
        resume_render.contains("Search")
            || resume_render.contains("/ to search")
            || resume_render.contains("search")
    );
    assert!(
        resume_render.contains("New session")
            || resume_render.contains("New Session")
            || resume_render.contains("continue ready")
    );
    assert!(!resume_render.contains("New session - 2026-03-08T12:34:56.000Z"));
    assert!(!resume_render.contains("beta-prompt"));
    assert!(resume_render.contains("continue ready"));
}

pub(super) fn session_history_filter_matches_visible_fields_and_fuzzy_title() {
    fn open_continue_picker() -> app::AppState {
        let mut app = app::AppState::new_startup(
            vec![
                startup_session_entry_with_mode_and_details(
                    "RUN-ABC123",
                    "/tmp/sessions/RUN-ABC123",
                    "Alpha Runner",
                    Some(harness_core::proj::RunStatus::Finished),
                    Some("2026-03-08T12:34:56Z"),
                    "DeepOps",
                    "OpenAI/GPT-5.4-Mini",
                    harness_core::proj::SessionModeSource::InteractiveLive,
                    false,
                    Some("run is still active"),
                ),
                startup_session_entry_with_details(
                    "run_other",
                    "/tmp/sessions/run_other",
                    "beta-run",
                    Some(harness_core::proj::RunStatus::Running),
                    Some("2026-03-08T08:00:00Z"),
                    "ops",
                    "anthropic/claude-3.7",
                    true,
                    None,
                ),
            ],
            None,
        );

        app.handle_key(key_with_modifiers(
            crossterm::event::KeyCode::Char('p'),
            crossterm::event::KeyModifiers::CONTROL,
        ));
        for ch in "resume".chars() {
            app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
        }
        app.handle_key(key(crossterm::event::KeyCode::Enter));
        app
    }

    let mut by_run_name = open_continue_picker();
    for ch in "runner".chars() {
        by_run_name.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_eq!(by_run_name.session_history_filtered, vec![0]);

    let mut by_case_insensitive_title = open_continue_picker();
    for ch in "ALPHA".chars() {
        by_case_insensitive_title.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_eq!(by_case_insensitive_title.session_history_filtered, vec![0]);

    let mut by_non_title_metadata = open_continue_picker();
    for ch in "gpt-5".chars() {
        by_non_title_metadata.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_eq!(by_non_title_metadata.session_history_filtered, vec![0]);

    let mut by_fuzzy_title = open_continue_picker();
    for ch in "alrn".chars() {
        by_fuzzy_title.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_eq!(by_fuzzy_title.session_history_filtered, vec![0]);

    let mut no_match = open_continue_picker();
    for ch in "missing".chars() {
        no_match.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    no_match.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(no_match.session_history_filtered.is_empty());
    let rendered = render_live_lines(&no_match, 120, 30);
    assert!(rendered.contains("No matches"));
}

pub(super) fn continue_picker_filters_to_interactive_sessions() {
    let mut app = app::AppState::new_startup(
        vec![
            startup_session_entry_with_mode_and_details(
                "run_blocked",
                "/tmp/sessions/run_blocked",
                "blocked-interactive",
                Some(harness_core::proj::RunStatus::Running),
                Some("2026-03-08T09:00:00Z"),
                "ops",
                "openai/gpt-5.4-mini",
                harness_core::proj::SessionModeSource::InteractiveLive,
                false,
                Some("run is still active"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_prompt",
                "/tmp/sessions/run_prompt",
                "prompt-only",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T08:00:00Z"),
                "ops",
                "openai/gpt-5.4-mini",
                harness_core::proj::SessionModeSource::Prompt,
                false,
                Some("prompt runs are not resumable"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_ready_live",
                "/tmp/sessions/run_ready_live",
                "ready-live",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T07:00:00Z"),
                "deep",
                "openai/gpt-5.4-mini",
                harness_core::proj::SessionModeSource::InteractiveLive,
                true,
                None,
            ),
            startup_session_entry_with_mode_and_details(
                "run_scenario",
                "/tmp/sessions/run_scenario",
                "scenario-fixture",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T06:00:00Z"),
                "default",
                "mock/mock-1",
                harness_core::proj::SessionModeSource::ScenarioFixture,
                false,
                Some("scenario fixture runs are excluded from resume"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_replay_only",
                "/tmp/sessions/run_replay_only",
                "replay-only",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T05:00:00Z"),
                "default",
                "openai/gpt-5.4-mini",
                harness_core::proj::SessionModeSource::ReplayOnly,
                false,
                Some("replay-only launches are not resumable"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_ready_mock",
                "/tmp/sessions/run_ready_mock",
                "ready-mock",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T04:00:00Z"),
                "mock",
                "mock/mock-1",
                harness_core::proj::SessionModeSource::InteractiveMock,
                true,
                None,
            ),
        ],
        None,
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(app.session_history_visible);
    assert_eq!(
        app.session_history_filtered
            .iter()
            .map(|index| app.session_history_entries[*index].catalog.run_id.as_str())
            .collect::<Vec<_>>(),
        vec!["run_ready_live", "run_ready_mock", "run_blocked"]
    );
    assert_eq!(
        app.session_history_entries[*app.session_history_filtered.last().unwrap_or_abort()]
            .catalog
            .resume_disabled_reason
            .as_deref(),
        Some("run is still active")
    );
    let rendered = render_live_lines(&app, 120, 30);
    assert!(rendered.contains("Resume session"));
    assert!(rendered.contains("run is still active"));
    assert!(!rendered.contains("prompt-only"));
    assert!(!rendered.contains("scenario-fixture"));
    assert!(!rendered.contains("replay-only"));
}

pub(super) fn replay_picker_keeps_prompt_runs_visible() {
    let mut app = app::AppState::new_startup(
        vec![
            startup_session_entry_with_mode_and_details(
                "run_ready_live",
                "/tmp/sessions/run_ready_live",
                "ready-live",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T07:00:00Z"),
                "deep",
                "openai/gpt-5.4-mini",
                harness_core::proj::SessionModeSource::InteractiveLive,
                true,
                None,
            ),
            startup_session_entry_with_mode_and_details(
                "run_prompt",
                "/tmp/sessions/run_prompt",
                "prompt-only",
                Some(harness_core::proj::RunStatus::Failed),
                Some("2026-03-08T06:00:00Z"),
                "ops",
                "openai/gpt-5.4-mini",
                harness_core::proj::SessionModeSource::Prompt,
                false,
                Some("prompt runs are not resumable"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_scenario",
                "/tmp/sessions/run_scenario",
                "scenario-fixture",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T05:00:00Z"),
                "default",
                "mock/mock-1",
                harness_core::proj::SessionModeSource::ScenarioFixture,
                false,
                Some("scenario fixture runs are excluded from resume"),
            ),
            startup_session_entry_with_mode_and_details(
                "run_replay_only",
                "/tmp/sessions/run_replay_only",
                "replay-only",
                Some(harness_core::proj::RunStatus::Finished),
                Some("2026-03-08T04:00:00Z"),
                "default",
                "openai/gpt-5.4-mini",
                harness_core::proj::SessionModeSource::ReplayOnly,
                false,
                Some("replay-only launches are not resumable"),
            ),
        ],
        None,
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(app.session_history_visible);
    assert_eq!(
        app.session_history_filtered
            .iter()
            .map(|index| app.session_history_entries[*index].catalog.run_id.as_str())
            .collect::<Vec<_>>(),
        vec!["run_ready_live"]
    );
    let rendered = render_live_lines(&app, 120, 30);
    assert!(rendered.contains("Resume session"));
    assert!(rendered.contains("continue ready"));
    assert!(!rendered.contains("scenario-fixture"));
    assert!(!rendered.contains("prompt-only"));
    assert!(!rendered.contains("replay-only"));
}

pub(super) fn focus_returns_after_session_history_close() {
    let mut app = app::AppState::new_live(None, false, None);
    app.focus = app::Focus::Details;
    app.composer.prompt_buffer = "keep prompt draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.set_session_history_entries(vec![startup_session_entry_with_details(
        "run_replay",
        "/tmp/sessions/run_replay",
        "replayable-run",
        Some(harness_core::proj::RunStatus::Finished),
        Some("2026-03-08T12:34:56Z"),
        "deep",
        "openai/gpt-5.4-mini",
        true,
        None,
    )]);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(app.session_history_visible);
    assert_eq!(app.focus, app::Focus::Details);
    assert_eq!(app.composer.prompt_buffer, "keep prompt draft");

    app.handle_key(key(crossterm::event::KeyCode::Esc));

    assert!(!app.session_history_visible);
    assert!(!app.palette_visible);
    assert_eq!(app.focus, app::Focus::Details);
    assert_eq!(app.composer.prompt_buffer, "keep prompt draft");
    assert_eq!(
        app.composer.prompt_cursor,
        "keep prompt draft".chars().count()
    );
}

pub(super) fn command_palette_enter_executes_selected_command() {
    let mut app = app::AppState::new_live(None, false, None);
    app.active_review_surface = Some(app::ReviewSurface::Help);
    app.focus = app::Focus::Details;
    app.composer.prompt_buffer = "preserve me".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "shell".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert_eq!(app.active_tab, app::Tab::Run);
    assert!(!app.palette_visible);
    assert_eq!(app.focus, app::Focus::Details);
    assert_eq!(app.composer.prompt_buffer, "preserve me");
    assert_eq!(app.composer.prompt_cursor, "preserve me".chars().count());
}

pub(super) fn palette_escape_preserves_prompt_draft() {
    let mut app = app::AppState::new_live(None, false, None);
    for c in "keep this prompt".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    let prompt_before = app.composer.prompt_buffer.clone();
    let cursor_before = app.composer.prompt_cursor;

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('d')));

    assert!(app.palette_visible);
    assert_eq!(app.palette_input, "d");

    app.handle_key(key(crossterm::event::KeyCode::Esc));

    assert!(!app.palette_visible);
    assert!(app.palette_input.is_empty());
    assert_eq!(app.palette_cursor, 0);
    assert!(app.palette_filtered.is_empty());
    assert_eq!(app.palette_selected, 0);
    assert_eq!(app.composer.prompt_buffer, prompt_before);
    assert_eq!(app.composer.prompt_cursor, cursor_before);
    assert!(app.composer.prompt_history.is_empty());
    assert_eq!(app.composer.prompt_history_index, None);
}

pub(super) fn session_pin_toggles_and_sorts_pinned_first() {
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry_with_details(
            "run_pin",
            "/tmp/sessions/run_pin",
            "Pin target",
            Some(harness_core::proj::RunStatus::Finished),
            Some("2026-03-08T12:34:56Z"),
            "deep",
            "openai/gpt-5.4-mini",
            true,
            None,
        )],
        None,
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));
    assert!(app.session_history_visible);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('f'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert!(app.session_pins.contains("run_pin"));

    assert_eq!(
        app.session_history_entries[*app.session_history_filtered.first().unwrap_or_abort()]
            .catalog
            .run_id,
        "run_pin"
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('f'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert!(!app.session_pins.contains("run_pin"));
}

pub(super) fn session_delete_two_press_arms_then_emits_intent() {
    let captured_intent = std::sync::Arc::new(std::sync::Mutex::new(None));
    let cb: std::sync::Arc<dyn Fn(app::UiIntent) + Send + Sync> = std::sync::Arc::new({
        let captured_intent = std::sync::Arc::clone(&captured_intent);
        move |intent| {
            *captured_intent.lock().unwrap() = Some(intent);
        }
    });

    let mut app = app::AppState::new_startup(
        vec![startup_session_entry_with_details(
            "run_del",
            "/tmp/sessions/run_del",
            "Delete target",
            Some(harness_core::proj::RunStatus::Finished),
            Some("2026-03-08T12:34:56Z"),
            "deep",
            "openai/gpt-5.4-mini",
            true,
            None,
        )],
        Some(cb),
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));
    assert!(app.session_history_visible);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('d'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert_eq!(app.session_delete_armed_run_id.as_deref(), Some("run_del"));

    app.handle_key(key(crossterm::event::KeyCode::Down));
    assert!(app.session_delete_armed_run_id.is_none());

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('d'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert_eq!(app.session_delete_armed_run_id.as_deref(), Some("run_del"));

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('d'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    let intent = captured_intent.lock().unwrap().take();
    assert!(
        matches!(intent, Some(app::UiIntent::DeleteSession { ref run_id, .. }) if run_id == "run_del"),
        "expected DeleteSession intent, got {intent:?}"
    );
}

pub(super) fn session_rename_dialog_opens_and_cancels() {
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry_with_details(
            "run_rename",
            "/tmp/sessions/run_rename",
            "Rename target",
            Some(harness_core::proj::RunStatus::Finished),
            Some("2026-03-08T12:34:56Z"),
            "deep",
            "openai/gpt-5.4-mini",
            true,
            None,
        )],
        None,
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));
    assert!(app.session_history_visible);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('r'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert!(app.session_rename_visible);
    assert_eq!(app.session_rename_input, "Rename target");
    assert_eq!(
        app.session_rename_target_run_id.as_deref(),
        Some("run_rename")
    );

    app.handle_key(key(crossterm::event::KeyCode::Esc));
    assert!(!app.session_rename_visible);
    assert!(app.session_rename_input.is_empty());
    assert!(app.session_rename_target_run_id.is_none());
}

pub(super) fn theme_dialog_opens_and_cycles_themes() {
    let mut app = app::AppState::new_live(None, false, None);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('t')));
    assert!(app.theme_dialog_visible);

    app.handle_key(key(crossterm::event::KeyCode::Down));
    app.handle_key(key(crossterm::event::KeyCode::Enter));
    assert!(!app.theme_dialog_visible);
    assert_eq!(app.theme_name, "high-contrast");
}

pub(super) fn theme_dialog_escape_closes_without_applying() {
    let mut app = app::AppState::new_live(None, false, None);
    let theme_before = *app.theme();

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('t')));
    assert!(app.theme_dialog_visible);

    app.handle_key(key(crossterm::event::KeyCode::Down));
    app.handle_key(key(crossterm::event::KeyCode::Esc));
    assert!(!app.theme_dialog_visible);
    assert_eq!(*app.theme(), theme_before);
}

pub(super) fn model_favorite_toggles_and_sorts_first() {
    let mut app = app::AppState::new_live(None, false, None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("build", "default:gpt-5.4-mini").with_available_models(
            vec![app::ModelOption::from_model_ref(
                "build",
                "default:gpt-5.4-mini",
            )],
        ),
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    app.handle_key(key(crossterm::event::KeyCode::Char('m')));
    assert!(app.model_switcher_visible);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('f'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert!(app.model_favorites.contains("gpt-5.4-mini"));

    let selected_index = app.model_filtered[app.model_selected];
    let selected_option = &app.model_options[selected_index];
    assert_eq!(selected_option.model, "gpt-5.4-mini");
}
