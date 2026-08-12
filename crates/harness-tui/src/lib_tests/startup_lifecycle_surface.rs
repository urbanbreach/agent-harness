use super::*;
use crate::UnwrapOrAbort;

pub(super) fn startup_surface_renders_primary_actions() {
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
            true,
            None,
        )],
        None,
    );
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );

    let rendered = render_live_lines(&app, 100, 24);
    assert_eq!(app.focus, app::Focus::Prompt);
    assert!(
        rendered.contains('╭')
            && (rendered.contains("New worktree") || rendered.contains("New session")),
        "startup welcome panel with actions\n{rendered}"
    );
    assert!(!rendered.contains("Launch: worker · model-1"));
    assert!(!rendered.contains("Provider mock"));
    assert!(
        rendered.contains("model-1") || rendered.contains("Worker") || rendered.contains("Demo")
    );
    assert!(rendered.contains('❯'));
    assert!(!rendered.contains("Enter select"));
    assert!(!rendered.contains("● Tip"));
    assert!(!rendered.contains("Dispatch a new run, reopen live work, or inspect saved history."));

    app.handle_key(key(crossterm::event::KeyCode::Char('x')));
    let draft = render_live_lines(&app, 100, 24);
    assert!(
        draft.contains("model-1") || draft.contains("Worker") || draft.contains("Demo"),
        "draft startup restores model chrome on composer\n{draft}"
    );
}

pub(super) fn startup_surface_projects_clipboard_capability() {
    let mut app = app::AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );
    crate::runtime::apply_startup_capability_notice(
        &mut app,
        crate::runtime::TerminalCapabilityState::absent(),
    );

    for (width, height) in [(80, 24), (100, 30), (120, 40)] {
        let buffer = render_live_cells(&app, width, height);
        let rendered = buffer
            .content
            .chunks(width as usize)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>();
        let warning_row = rendered
            .iter()
            .position(|row| row.contains("Clipboard may be unreachable."))
            .unwrap_or_else(|| panic!("missing clipboard warning at {width}x{height}"));
        let hint_row = rendered
            .iter()
            .position(|row| row.contains("Run /doctor for details and fixes."))
            .unwrap_or_else(|| panic!("missing clipboard hint at {width}x{height}"));
        let panel_row = rendered
            .iter()
            .position(|row| row.contains('╭') && row.contains('─'))
            .unwrap_or_else(|| panic!("missing welcome panel at {width}x{height}"));
        assert_eq!(hint_row, warning_row + 1);
        assert!(
            panel_row > hint_row,
            "welcome panel overlaps warning at {width}x{height}"
        );
        assert!(rendered[1].contains("worktree"));
    }

    let loading = render_live_cells(&app, 100, 30);
    let loading_rows = loading.content.chunks(100).collect::<Vec<_>>();
    assert_eq!(
        loading_rows
            .iter()
            .position(|row| row.iter().any(|cell| cell.symbol() == "╭")),
        Some(6)
    );
    assert!(loading
        .content
        .iter()
        .any(|cell| cell.symbol().contains('•')));

    let mut ready_app = app;
    for _ in 0..4 {
        ready_app.advance_animation_tick_for_evidence();
    }
    let ready = render_live_cells(&ready_app, 100, 30);
    let ready_rows = ready.content.chunks(100).collect::<Vec<_>>();
    assert_eq!(
        ready_rows
            .iter()
            .position(|row| row.iter().any(|cell| cell.symbol() == "╭")),
        Some(6)
    );
    assert_eq!(loading, ready);
    let ready_warning_row = ready_rows
        .iter()
        .position(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .contains("Clipboard")
        })
        .unwrap_or_else(|| {
            let rows = ready_rows
                .iter()
                .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
                .collect::<Vec<_>>();
            panic!("ready warning missing: {rows:?}")
        });
    let ready_hint_row = ready_rows
        .iter()
        .position(|row| {
            row.iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .contains("doctor")
        })
        .unwrap_or_else(|| panic!("ready hint missing: {ready_rows:?}"));
    assert_eq!(ready_warning_row + 1, ready_hint_row);
    assert!(ready_hint_row < 6);
}

pub(super) fn startup_typing_moves_to_quick_start_prompt() {
    let mut app = app::AppState::new_startup(Vec::new(), None);

    assert_eq!(app.focus, app::Focus::Prompt);
    assert!(app.composer.prompt_buffer.is_empty());

    app.handle_key(key(crossterm::event::KeyCode::Char('x')));

    assert_eq!(app.focus, app::Focus::Prompt);
    assert_eq!(app.composer.prompt_buffer, "x");
    assert_eq!(app.composer.prompt_cursor, 1);

    let rendered = render_live_lines(&app, 100, 24);
    assert!(!rendered.contains("Composer"));
    assert!(rendered.contains('x'));
    assert!(
        rendered.contains("Enter:send") || rendered.contains('❯'),
        "draft keeps composer and switches footer grammar\n{rendered}"
    );
    assert!(!rendered.contains("New worktree"));
    assert!(!rendered.contains("New session"));
    assert!(!rendered.contains("● Tip"));
    assert!(!rendered.contains("Dispatch a new run, reopen live work, or inspect saved history."));
}

pub(super) fn startup_palette_remains_secondary_and_draft_safe() {
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
            true,
            None,
        )],
        None,
    );

    for ch in "keep this draft".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }

    let rendered = render_live_lines(&app, 100, 24);
    assert!(!rendered.contains("Dispatch a new run, reopen live work, or inspect saved history."));
    assert!(!rendered.contains("Actions:"));
    assert_eq!(app.composer.prompt_buffer, "keep this draft");
    assert_eq!(app.focus, app::Focus::Prompt);

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    assert!(app.palette_visible);
    let overlay_render = render_live_lines(&app, 120, 30);
    assert!(overlay_render.contains("Commands"));
    assert!(overlay_render.contains("New Session"));
    assert!(overlay_render.contains("Resume Session"));

    app.handle_key(key(crossterm::event::KeyCode::Esc));

    assert!(!app.palette_visible);
    assert_eq!(app.composer.prompt_buffer, "keep this draft");
    assert_eq!(
        app.composer.prompt_cursor,
        "keep this draft".chars().count()
    );
    assert_eq!(app.focus, app::Focus::Prompt);
}

pub(super) fn post_run_handoff_renders_next_actions() {
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
        None,
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    assert_eq!(app.active_tab, app::Tab::Run);
    assert_eq!(app.focus, app::Focus::Details);
    assert!(!app.post_run_handoff_visible());
    assert!(app.completed_session_shell_active());

    let rendered = render_live_lines(&app, 100, 24);
    assert!(rendered.contains("keep this draft"));
    assert!(!rendered.contains("Composer"));
    assert!(rendered.contains("keep this draft"));
    assert!(!rendered.contains("Next action"));
    assert!(!rendered.contains("Continue this session"));
}

pub(super) fn post_run_failure_handoff_renders_recovery_actions() {
    let mut app = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::RunFailed(harness_core::event::RunFailedEvent {
            error: "tool execution failed".to_string(),
        }),
    ));

    assert_eq!(app.focus, app::Focus::Details);
    assert!(!app.post_run_handoff_visible());
    assert!(app.completed_session_shell_active());

    let rendered = render_live_lines(&app, 100, 24);
    assert!(!rendered.contains("Shift+Tab:mode"));
    assert!(!rendered.contains("Ctrl+x:shortcuts"));
    assert!(!rendered.contains("Next action"));
    assert!(!rendered.contains("Continue this session"));
}

pub(super) fn post_run_handoff_disables_prompt_submission() {
    let intents = Arc::new(std::sync::Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = app::AppState::new_live(None, false, Some(intent_sink));
    app.composer.prompt_buffer = "blocked prompt".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();

    app.ingest_event(envelope(
        1,
        None,
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));

    let rendered = render_live_lines(&app, 100, 24);
    assert!(!rendered.contains("Next action"));
    assert!(!rendered.contains("Continue this session"));
    assert!(rendered.contains("blocked prompt"));
    assert!(!rendered.contains("Composer"));

    app.focus = app::Focus::Prompt;
    app.handle_key(key(crossterm::event::KeyCode::Enter));
    assert_eq!(app.composer.prompt_buffer, "blocked prompt");

    let intents = intents.lock().unwrap_or_abort();
    assert!(intents.is_empty());
    assert!(!app.should_quit);
}

pub(super) fn double_escape_does_not_interrupt_active_live_turn() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = app::AppState::new_live(None, false, Some(intent_sink));
    app.focus = app::Focus::Details;

    app.ingest_event(envelope_with_actor(
        1,
        Some("req_active"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_active".to_string().into(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("provider_model:default:model-1".to_string()),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        2,
        Some("req_sibling"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_sibling".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_sibling".to_string().into(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("provider_model:default:model-2".to_string()),
        }),
    ));

    assert!(app.interrupt_hint_visible());
    assert!(render_live_lines(&app, 100, 24).contains("ctrl+c interrupt"));

    app.handle_key(key(crossterm::event::KeyCode::Esc));
    app.handle_key(key(crossterm::event::KeyCode::Esc));

    assert!(!app.interrupt_confirmation_pending());
    assert!(
        intents.lock().unwrap_or_abort().is_empty(),
        "Esc must not cancel a running turn (simple-mode: Ctrl+C cancels)"
    );
}

pub(super) fn ctrl_c_interrupts_current_active_turn_set() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = app::AppState::new_live(None, false, Some(intent_sink));
    app.focus = app::Focus::Details;

    app.ingest_event(envelope_with_actor(
        1,
        Some("req_old"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_old".to_string().into(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("provider_model:default:model-1".to_string()),
        }),
    ));

    app.ingest_event(envelope(
        2,
        Some("req_old"),
        harness_core::event::EventV1::TaskCancelled(harness_core::event::TaskCancelledEvent {
            task_id: "task_old".to_string().into(),
            reason: "cancelled externally".to_string(),
            task_scope: Some(harness_core::event::TaskTerminalScope::AgentTurn),
        }),
    ));
    app.ingest_event(envelope_with_actor(
        3,
        Some("req_new"),
        harness_core::event::EventActor::new(
            harness_core::event::ActorKind::Worker,
            Some("agent_worker".to_string()),
        ),
        harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
            task_id: "task_new".to_string().into(),
            state: harness_core::event::TaskScheduleState::Started,
            queue_key: Some("provider_model:default:model-1".to_string()),
        }),
    ));

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('c'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    assert_eq!(
        &*intents.lock().unwrap_or_abort(),
        &[UiIntent::InterruptSession {
            task_ids: vec!["task_new".to_string()],
        }]
    );
}

pub(super) fn continued_quiescent_bootstrap_shows_handoff_before_reopening_live_conversation() {
    app::set_pending_live_launch_metadata(
        app::LaunchMetadata::from_model_ref("worker", "default:model-1")
            .with_mode_label("Continued"),
    );
    let mut app = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_resume_quiescent")),
        false,
        Some(Arc::new(|_| {})),
    );

    app.ingest_historical_event(envelope(
        1,
        Some("req_resume_terminal"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    let handoff_render = render_live_lines(&app, 100, 24);
    assert!(!handoff_render.contains("Next action"));
    assert!(!handoff_render.contains("Continue this session"));
    assert!(!handoff_render.contains("Ask Harness to inspect, edit, or explain…"));
    assert!(!handoff_render.contains("Composer"));
    assert!(!app.composer_disabled());
}

pub(super) fn lifecycle_shell_state_transitions() {
    let mut startup = app::AppState::new_startup(Vec::new(), None);
    startup.composer.prompt_buffer = "draft prompt".to_string();
    startup.composer.prompt_cursor = startup.composer.prompt_buffer.chars().count();

    assert_eq!(
        startup.lifecycle_shell_state(),
        app::LifecycleShellState::Startup
    );
    assert!(startup.startup_shell_visible());
    assert!(!startup.post_run_handoff_visible());
    assert!(!startup.composer_disabled());

    let mut post_run = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    post_run.ingest_event(envelope(
        1,
        Some("req_state_transition"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    assert_eq!(
        post_run.lifecycle_shell_state(),
        app::LifecycleShellState::None
    );
    assert!(!post_run.startup_shell_visible());
    assert!(!post_run.post_run_handoff_visible());
    assert!(post_run.completed_session_shell_active());

    let fallback_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(|_| {});
    let mut missing_session_path = app::AppState::new_live(None, false, Some(fallback_sink));
    missing_session_path.ingest_event(envelope(
        1,
        Some("req_state_transition_missing_path"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    assert_eq!(
        missing_session_path.lifecycle_shell_state(),
        app::LifecycleShellState::None
    );
    assert!(!missing_session_path.post_run_handoff_visible());
    assert!(missing_session_path.completed_session_shell_active());

    let replay = app::AppState::new_replay(
        PathBuf::from("/tmp/replay-session"),
        vec![envelope(
            1,
            Some("req_replay_state_transition"),
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
    assert!(replay.composer_disabled());
}

pub(super) fn lifecycle_shell_snapshots_preserve_startup_and_handoff_contracts() {
    let mut startup = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
            true,
            None,
        )],
        None,
    );
    startup.set_launch_metadata(
        app::LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );

    let startup_render = render_live_lines(&startup, 100, 24);
    assert!(startup_render.contains("Harness") || startup_render.contains('╭'));
    assert!(
        startup_render.contains("New worktree")
            || startup_render.contains("New session")
            || startup_render.contains('❯')
    );
    assert!(!startup_render.contains("Enter select"));
    assert!(
        !startup_render.contains("Dispatch a new run, reopen live work, or inspect saved history.")
    );

    let entries = vec![
        startup_session_entry_with_details(
            "run_resume",
            "/tmp/sessions/run_resume",
            "alpha-run",
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
        startup_session_entry_with_mode_and_details(
            "run_blocked",
            "/tmp/sessions/run_blocked",
            "blocked-interactive",
            Some(harness_core::proj::RunStatus::Running),
            Some("2026-03-06T09:15:00Z"),
            "ops",
            "openai/gpt-5.4-mini",
            harness_core::proj::SessionModeSource::InteractiveLive,
            false,
            Some("run is still active"),
        ),
    ];
    let mut picker = app::AppState::new_startup(entries, None);
    for ch in "keep this draft".chars() {
        picker.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    assert_eq!(picker.composer.prompt_buffer, "keep this draft");
    assert_eq!(picker.focus, app::Focus::Prompt);

    picker.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        picker.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    picker.handle_key(key(crossterm::event::KeyCode::Enter));

    let continue_render = render_live_lines(&picker, 120, 30);
    assert!(picker.session_history_visible);
    assert_eq!(picker.composer.prompt_buffer, "keep this draft");
    assert!(continue_render.contains("Resume session"));
    assert!(
        continue_render.contains("ago")
            || continue_render.contains("just now")
            || continue_render.contains("run is still active"),
        "session picker must show relative age or status\n{continue_render}"
    );
    assert!(continue_render.contains("run is still active"));
    assert!(!continue_render.contains("beta-prompt"));
    assert!(continue_render.contains("Harness") || continue_render.contains("Resume session"));

    let mut completed_shell = app::AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_fixture")),
        false,
        None,
    );
    completed_shell.active_review_surface = Some(app::ReviewSurface::Help);
    completed_shell.focus = app::Focus::Prompt;
    completed_shell.composer.prompt_buffer = "keep this draft".to_string();
    completed_shell.composer.prompt_cursor = completed_shell.composer.prompt_buffer.chars().count();
    completed_shell.ingest_event(envelope(
        1,
        Some("req_post_run"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    let completed_shell_render = render_live_lines(&completed_shell, 100, 24);
    assert!(completed_shell_render.contains("keep this draft"));
    assert!(!completed_shell_render.contains("Composer"));
    assert!(!completed_shell_render.contains("Next action"));
    insta::with_settings!({ prepend_module_to_snapshot => false, snapshot_path => "../snapshots" }, {
        insta::assert_snapshot!(
            "harness_tui__completed_shell_lifecycle",
            completed_shell_render
                .lines()
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n")
        );
    });

    let fallback_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(|_| {});
    let mut fallback = app::AppState::new_live(None, false, Some(fallback_sink));
    fallback.ingest_event(envelope(
        1,
        Some("req_post_run_missing_session_path"),
        harness_core::event::EventV1::RunFinished(harness_core::event::RunFinishedEvent {
            summary: "all tasks complete".to_string(),
        }),
    ));

    let fallback_render = render_live_lines(&fallback, 100, 24);
    assert!(!fallback_render.contains("Run complete"));
    assert!(!fallback_render.contains("Shift+Tab:mode"));
    assert!(!fallback_render.contains("Ctrl+x:shortcuts"));
    assert!(!fallback_render.contains("Composer"));
    assert!(!fallback_render.contains("Next action"));
    insta::with_settings!({ prepend_module_to_snapshot => false, snapshot_path => "../snapshots" }, {
        insta::assert_snapshot!(
            "harness_tui__completed_shell_fallback_lifecycle",
            fallback_render
                .lines()
                .map(str::trim_end)
                .collect::<Vec<_>>()
                .join("\n")
        );
    });
}

pub(super) fn session_history_browse_preserves_draft() {
    let mut app = app::AppState::new_startup(
        vec![
            startup_session_entry("run_a", "/tmp/sessions/run_a", true, None),
            startup_session_entry("run_b", "/tmp/sessions/run_b", true, None),
        ],
        None,
    );
    for c in "startup draft".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    let before = app.composer.prompt_buffer.clone();
    let cursor_before = app.composer.prompt_cursor;

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "resume".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(app.session_history_visible);
    assert_eq!(app.composer.prompt_buffer, before);
    assert_eq!(app.composer.prompt_cursor, cursor_before);

    app.handle_key(key(crossterm::event::KeyCode::Down));
    assert_eq!(app.session_history_selected, 1);

    app.handle_key(key(crossterm::event::KeyCode::Esc));
    assert!(!app.session_history_visible);
    assert_eq!(app.composer.prompt_buffer, before);
    assert_eq!(app.composer.prompt_cursor, cursor_before);
}

pub(super) fn new_session_resets_transcript_but_keeps_unsent_draft() {
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_a",
            "/tmp/sessions/run_a",
            true,
            None,
        )],
        None,
    );
    app.ingest_event(envelope(
        1,
        Some("req_before_reset"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_before_reset".into(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "before reset".to_string(),
                request_digest: "digest-before-reset".to_string(),
                metadata: None,
            },
        ),
    ));
    app.composer
        .prompt_history
        .push("older sent prompt".to_string());
    app.composer.prompt_buffer = "unsent startup draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "new".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    assert!(app.events.is_empty());
    assert!(app.activities.is_empty());
    assert!(app.composer.prompt_history.is_empty());
    assert_eq!(app.composer.prompt_buffer, "unsent startup draft");
    assert_eq!(
        app.composer.prompt_cursor,
        "unsent startup draft".chars().count()
    );
}

pub(super) fn startup_first_run_shows_onboarding_hint() {
    let app = app::AppState::new_startup(Vec::new(), None);

    assert!(app.is_first_run());

    let rendered = render_live_lines(&app, 100, 24);
    assert!(
        (rendered.contains("New worktree") || rendered.contains("New session"))
            && rendered.contains('❯'),
        "first-run startup should expose welcome actions and composer\n{rendered}"
    );
    assert!(
        rendered.contains("Changelog") || rendered.contains("Harness"),
        "first-run startup should show welcome identity/changelog\n{rendered}"
    );
}

pub(super) fn startup_returning_user_hides_onboarding_hint() {
    let app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_resume",
            "/tmp/sessions/run_resume",
            true,
            None,
        )],
        None,
    );

    assert!(!app.is_first_run());

    let rendered = render_live_lines(&app, 100, 24);
    assert!(
        !rendered.contains("harness doctor"),
        "returning-user startup should not show first-run onboarding hint\n{rendered}"
    );
    assert!(
        !rendered.contains("harness auth login"),
        "returning-user startup should not show first-run onboarding hint\n{rendered}"
    );
    assert!(
        rendered.contains("Resume session")
            || rendered.contains("New worktree")
            || rendered.contains("New session"),
        "returning-user startup should still show welcome actions\n{rendered}"
    );
}

pub(super) fn startup_hints_stay_compose_first() {
    let app = app::AppState::new_startup(Vec::new(), None);

    let rendered = render_live_lines(&app, 100, 24);
    assert!(
        rendered.contains('❯'),
        "startup with welcome shell should keep the composer accessible\n{rendered}"
    );
    assert!(
        rendered.contains("New worktree")
            || rendered.contains("New session")
            || rendered.contains("Logged in"),
        "startup should keep welcome actions or status footer\n{rendered}"
    );
    assert!(
        !rendered.contains("Dispatch a new run"),
        "startup should not render the subtitle\n{rendered}"
    );
}
