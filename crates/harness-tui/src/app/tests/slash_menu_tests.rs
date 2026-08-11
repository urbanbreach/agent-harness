use super::*;
use crate::UnwrapOrAbort;

pub(super) fn slash_menu_closes_after_whitespace() {
    let mut app = AppState::new_startup(Vec::new(), None);

    app.handle_key(key(KeyCode::Char('/')));
    app.handle_key(key(KeyCode::Char('n')));
    assert!(app.slash_visible);

    app.handle_key(key(KeyCode::Char(' ')));

    assert!(!app.slash_visible);
    assert_eq!(app.composer.prompt_buffer, "/n ");
}

pub(super) fn slash_menu_resets_selection_when_filter_changes() {
    let mut app = AppState::new_startup(Vec::new(), None);

    app.handle_key(key(KeyCode::Char('/')));
    app.slash_selected = 2;
    assert_eq!(app.slash_selected, 2);

    app.handle_key(key(KeyCode::Char('r')));
    app.handle_key(key(KeyCode::Char('e')));
    app.handle_key(key(KeyCode::Char('s')));

    assert_eq!(app.slash_filtered, vec!["sessions".to_string()]);
    assert_eq!(app.slash_selected, 0);
}

pub(super) fn slash_menu_matches_descriptions_and_boosts_prefixes() {
    let mut app = AppState::new_startup(Vec::new(), None);

    for ch in "/ses".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert_eq!(
        app.slash_filtered.first().map(String::as_str),
        Some("sessions")
    );

    app.clear_prompt_input();
    for ch in "/re".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert_eq!(
        app.slash_filtered.first().map(String::as_str),
        Some("sessions")
    );
    assert!(app.slash_filtered.iter().any(|command| command == "new"));

    app.clear_prompt_input();
    for ch in "/nw".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert_eq!(app.slash_filtered.first().map(String::as_str), Some("new"));

    app.clear_prompt_input();
    for ch in "/continue".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert_eq!(
        app.slash_filtered.first().map(String::as_str),
        Some("sessions")
    );
}

pub(super) fn slash_alias_executes_matching_command_without_menu() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    for ch in "/quit".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.should_quit);
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::QuitRequested]
    );
}

pub(super) fn slash_help_opens_help_surface_and_preserves_draft() {
    // arrange
    let mut app = AppState::new_startup(Vec::new(), None);

    // act
    app.execute_slash_command("help", Some("preserved draft".to_string()));

    // assert
    assert_eq!(app.review_surface(), Some(ReviewSurface::Help));
    assert_eq!(app.composer.prompt_buffer, "preserved draft");
    assert_eq!(
        app.composer.prompt_cursor,
        "preserved draft".chars().count()
    );
    assert!(!app.should_quit);
}

pub(super) fn slash_escape_clears_token_or_restores_prior_draft() {
    let mut fresh = AppState::new_startup(Vec::new(), None);
    for ch in "/re".chars() {
        fresh.handle_key(key(KeyCode::Char(ch)));
    }

    fresh.handle_key(key(KeyCode::Esc));

    assert_eq!(fresh.composer.prompt_buffer, "");
    assert_eq!(fresh.composer.prompt_cursor, 0);
    assert!(!fresh.slash_visible);

    let mut with_draft = AppState::new_startup(Vec::new(), None);
    with_draft.composer.prompt_buffer = "draft".to_string();
    with_draft.composer.prompt_cursor = 0;
    with_draft.handle_key(key(KeyCode::Char('/')));

    with_draft.handle_key(key(KeyCode::Esc));

    assert_eq!(with_draft.composer.prompt_buffer, "draft");
    assert_eq!(with_draft.composer.prompt_cursor, "draft".chars().count());
    assert!(!with_draft.slash_visible);
}

pub(super) fn slash_exit_matches_quit_requested_behavior() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    for ch in "/exit".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.should_quit);
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::QuitRequested]
    );
}

pub(super) fn resume_history_surface_uses_meaningful_session_title() {
    // arrange
    let entry = SessionHistoryEntry {
        run_dir: PathBuf::from("/tmp/run-title"),
        catalog: harness_core::proj::SessionCatalogEntry {
            run_id: "run-title".into(),
            run_name: Some("map chat renderers".to_string()),
            status: Some(harness_core::proj::RunStatus::Finished),
            last_updated_at: Some("2026-02-03T12:00:00Z".to_string()),
            workspace_root: Some("/tmp/workspace".to_string()),
            profile_preset: Some("default".to_string()),
            provider_model: Some("mock/model".to_string()),
            mode_source: harness_core::proj::SessionModeSource::InteractiveLive,
            is_resumable: true,
            resume_disabled_reason: None,
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: None,
        },
    };
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_startup(vec![entry.clone()], Some(sink));
    for ch in "/resume".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    // act
    app.handle_key(key(KeyCode::Enter));

    // assert
    assert!(app.session_history_visible);
    assert_eq!(
        app.startup_launcher_action,
        StartupLauncherAction::ContinueSession
    );
    let selected = app.selected_session_history_entry().unwrap_or_abort();
    assert_eq!(
        session_navigation::session_history_display_title(selected),
        "map chat renderers"
    );
    let rendered = render_debug(&app, 100, 30);
    assert!(rendered.contains("map chat renderers"));
    assert!(
        !rendered.contains("<unavailable>"),
        "resume history should not degrade a titled session: {rendered}"
    );

    app.handle_key(key(KeyCode::Enter));
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ContinueSession {
            run_id: "run-title".into(),
            run_dir: PathBuf::from("/tmp/run-title"),
        }]
    );
}

pub(super) fn live_session_picker_continue_quits_tui_and_emits_intent() {
    let entry = SessionHistoryEntry {
        run_dir: PathBuf::from("/tmp/run-live-continue"),
        catalog: harness_core::proj::SessionCatalogEntry {
            run_id: "run-live-continue".into(),
            run_name: Some("live continue target".to_string()),
            status: Some(harness_core::proj::RunStatus::Finished),
            last_updated_at: Some("2026-02-03T12:00:00Z".to_string()),
            workspace_root: Some("/tmp/workspace".to_string()),
            profile_preset: Some("default".to_string()),
            provider_model: Some("mock/model".to_string()),
            mode_source: harness_core::proj::SessionModeSource::InteractiveLive,
            is_resumable: true,
            resume_disabled_reason: None,
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: None,
        },
    };
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/run-live-current")),
        false,
        Some(sink),
    );
    app.set_session_history_entries(vec![entry.clone()]);
    app.execute_slash_command("sessions", None);

    assert!(app.session_history_visible);
    assert_eq!(
        app.startup_launcher_action,
        StartupLauncherAction::ContinueSession
    );

    app.handle_key(key(KeyCode::Enter));

    assert!(app.should_quit, "selecting a continue target from the live session picker should exit the TUI so the outer workflow can switch sessions");
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ContinueSession {
            run_id: "run-live-continue".into(),
            run_dir: PathBuf::from("/tmp/run-live-continue"),
        }]
    );
}

pub(super) fn live_session_picker_replay_quits_tui_and_emits_intent() {
    let entry = SessionHistoryEntry {
        run_dir: PathBuf::from("/tmp/run-live-replay"),
        catalog: harness_core::proj::SessionCatalogEntry {
            run_id: "run-live-replay".into(),
            run_name: Some("live replay target".to_string()),
            status: Some(harness_core::proj::RunStatus::Finished),
            last_updated_at: Some("2026-02-03T12:00:00Z".to_string()),
            workspace_root: Some("/tmp/workspace".to_string()),
            profile_preset: Some("default".to_string()),
            provider_model: Some("mock/model".to_string()),
            mode_source: harness_core::proj::SessionModeSource::InteractiveLive,
            is_resumable: true,
            resume_disabled_reason: None,
            artifact_count: 0,
            child_session_count: 0,
            parent_session_id: None,
        },
    };
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/run-live-current")),
        false,
        Some(sink),
    );
    app.set_session_history_entries(vec![entry.clone()]);
    app.execute_slash_command("replay", None);

    assert!(app.session_history_visible);
    assert_eq!(
        app.startup_launcher_action,
        StartupLauncherAction::ReplaySession
    );

    app.handle_key(key(KeyCode::Enter));

    assert!(app.should_quit, "selecting a replay target from the live session picker should exit the TUI so the outer workflow can switch sessions");
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::ReplaySession {
            run_id: "run-live-replay".into(),
            run_dir: PathBuf::from("/tmp/run-live-replay"),
        }]
    );
}

pub(super) fn slash_menu_supports_mouse_selection() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.handle_key(key(KeyCode::Char('/')));

    let frame = Rect::new(0, 0, 100, 24);
    let overlay = crate::layout::FrameLayoutPlan::for_app(&app, frame)
        .slash_overlay
        .unwrap_or_abort();
    let list_area = crate::layout::slash_command_overlay_content_area(overlay);
    let target_index = app
        .slash_filtered
        .iter()
        .position(|command| command == "new")
        .unwrap_or_abort();
    let target_row = list_area
        .y
        .saturating_add(u16::try_from(target_index).unwrap_or_abort());

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: list_area.x.saturating_add(1),
            row: target_row,
            modifiers: KeyModifiers::NONE,
        },
        frame,
        None,
        None,
        None,
    );
    assert_eq!(app.slash_selected, target_index);

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: list_area.x.saturating_add(1),
            row: target_row,
            modifiers: KeyModifiers::NONE,
        },
        frame,
        None,
        None,
        None,
    );

    assert!(app.startup_shell_visible());
    assert_eq!(
        app.startup_launcher_action,
        StartupLauncherAction::NewSession
    );
}

pub(super) fn slash_menu_exposes_model_switcher_when_models_are_configured() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("default", "default:gpt-5.4-mini").with_available_models(
            vec![ModelOption::from_model_ref(
                "default",
                "default:gpt-5.4-mini",
            )],
        ),
    );

    app.handle_key(key(KeyCode::Char('/')));

    assert!(app.slash_filtered.iter().any(|command| command == "models"));
}

pub(super) fn rename_slash_command_availability_matches_mode() {
    let mut live = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    live.replace_prompt_input("/rename".to_string());
    live.sync_slash_overlay();
    assert!(live
        .slash_filtered
        .iter()
        .any(|command| command == "rename"));

    let mut replay = AppState::new_replay(PathBuf::from("/tmp/replay"), Vec::new());
    replay.handle_key(key(KeyCode::Char('/')));
    assert!(!replay
        .slash_filtered
        .iter()
        .any(|command| command == "rename"));

    let mut startup = AppState::new_startup(Vec::new(), None);
    startup.handle_key(key(KeyCode::Char('/')));
    assert!(!startup
        .slash_filtered
        .iter()
        .any(|command| command == "rename"));
}

pub(super) fn rename_slash_command_emits_update_session_title_intent() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, Some(sink));
    app.composer.prompt_buffer = "/rename New Title".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.slash_draft_snapshot = Some("draft preserved".to_string());
    app.sync_slash_overlay();

    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.composer.prompt_buffer, "draft preserved");
    assert!(!app.slash_visible);
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::UpdateSessionTitle {
            title: "New Title".to_string(),
        }]
    );
}

pub(super) fn rename_slash_empty_title_emits_error_toast() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, Some(sink));
    app.composer.prompt_buffer = "/rename  ".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.sync_slash_overlay();

    app.handle_key(key(KeyCode::Enter));

    assert!(!intents
        .lock()
        .unwrap_or_abort()
        .iter()
        .any(|intent| matches!(intent, UiIntent::UpdateSessionTitle { .. })));
    assert_eq!(
        app.status_banner.as_deref(),
        Some("session title cannot be empty")
    );
}
