use super::*;
use crate::UnwrapOrAbort;

pub(super) fn config_backed_live_launch_starts_in_session_shell_without_details_drawer() {
    set_pending_live_launch_metadata(LaunchMetadata::new(
        "deep",
        "default",
        Some("gpt-5.4-mini".to_string()),
    ));

    let app = AppState::new_live(None, false, None);

    assert!(!app.details_drawer_open());
    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(app.launch_mode_label(), None);
    assert_eq!(app.current_model_reasoning_label(), None);
}

pub(super) fn historical_task_completed_marks_turn_done_and_unblocks_first_resumed_submit() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(sink));
    app.ingest_event(envelope(
        1,
        "req_resume_1",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_resume_1".into(),
            text: "previous question".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_resume_1",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_resume_1".into(),
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "previous question".to_string(),
            request_digest: "digest-resume-1".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_resume_1",
        EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
            request_id: "req_resume_1".into(),
            delta: "previous answer".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_resume_1",
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000123".to_string().into(),
            result_summary: "previous answer".to_string(),
            result_digest: "digest-task-123".to_string(),
            metadata: None,
        }),
    ));

    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Success);

    for c in "next".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));

    let intents = intents.lock().unwrap_or_abort();
    assert!(
        intents
            .iter()
            .any(|intent| matches!(intent, UiIntent::SubmitPrompt { text, .. } if text == "next")),
        "historical streaming residue should not block first resumed submit"
    );
}

pub(super) fn historical_terminal_events_stay_in_session_shell_after_live_finish() {
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_resume")),
        true,
        Some(Arc::new(|_| {})),
    );

    app.ingest_historical_event(envelope(
        1,
        "req_resume_terminal",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "previous run complete".to_string(),
        }),
    ));

    assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!app.post_run_handoff_visible());
    assert!(!app.completed_session_shell_active());
    assert!(!app.should_quit);
    assert_eq!(app.events.len(), 1);

    app.ingest_event(envelope(
        2,
        "req_live_terminal",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "live run complete".to_string(),
        }),
    ));

    assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!app.post_run_handoff_visible());
    assert!(app.completed_session_shell_active());
    assert_eq!(app.active_tab, Tab::Run);
    assert_eq!(app.focus, Focus::Details);
    assert!(app.should_quit);
}

pub(super) fn continued_quiescent_bootstrap_stays_in_session_shell_without_handoff() {
    set_pending_live_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Continued"),
    );
    let mut app = AppState::new_live(
        Some(PathBuf::from("/tmp/sessions/run_resume_quiescent")),
        false,
        Some(Arc::new(|_| {})),
    );

    app.ingest_historical_event(envelope(
        1,
        "req_resume_terminal",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "previous run complete".to_string(),
        }),
    ));

    assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!app.post_run_handoff_visible());
    assert_eq!(app.active_tab, Tab::Run);
    assert_eq!(app.focus, Focus::Prompt);
    assert!(!app.composer_disabled());
}

pub(super) fn startup_ctrl_w_empty_composer_requests_new_worktree_session() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    assert!(app.startup_mode);
    assert!(app.composer.prompt_buffer.is_empty());

    app.handle_key(key_with_modifiers(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    ));

    assert!(
        app.should_quit,
        "worktree handoff should leave startup shell"
    );
    assert!(
        matches!(
            intents.lock().unwrap_or_abort().as_slice(),
            [UiIntent::NewWorktreeSession]
        ),
        "empty startup Ctrl+W must request NewWorktreeSession, not word-delete"
    );
}

pub(super) fn startup_ctrl_w_with_draft_still_deletes_word() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    for c in "hello world".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    assert_eq!(app.composer.prompt_buffer, "hello world");

    app.handle_key(key_with_modifiers(
        KeyCode::Char('w'),
        KeyModifiers::CONTROL,
    ));

    assert_eq!(app.composer.prompt_buffer, "hello ");
    assert!(
        intents.lock().unwrap_or_abort().is_empty(),
        "draft Ctrl+W must keep word-delete, not create a worktree"
    );
}

pub(super) fn palette_new_worktree_requests_new_worktree_session() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    app.request_new_worktree_session();

    assert!(app.should_quit);
    assert!(
        matches!(
            intents.lock().unwrap_or_abort().as_slice(),
            [UiIntent::NewWorktreeSession]
        ),
        "palette/worktree path must emit NewWorktreeSession"
    );
}

pub(super) fn startup_prompt_enter_echoes_prompt_and_selects_new_session() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_startup(Vec::new(), Some(sink));

    for c in "ship it".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.should_quit, "startup submit should leave the launcher");
    assert!(!app.startup_shell_visible());
    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(app.composer.prompt_buffer, "");
    assert_eq!(app.composer.prompt_history, vec!["ship it".to_string()]);
    assert_eq!(
        app.activities
            .back()
            .and_then(|activity| activity.user_message.as_ref())
            .map(|message| message.text.as_str()),
        Some("ship it")
    );
    let next_live = AppState::new_live(None, false, None);
    assert!(
        matches!(
            intents.lock().unwrap_or_abort().as_slice(),
            [UiIntent::NewSession]
        ),
        "startup submit should select a fresh session after the local prompt echo"
    );
    assert_eq!(
        next_live.composer.prompt_history,
        vec!["ship it".to_string()]
    );
    assert_eq!(
        next_live
            .activities
            .back()
            .and_then(|activity| activity.user_message.as_ref())
            .map(|message| message.text.as_str()),
        Some("ship it")
    );
}

pub(super) fn slash_new_then_submit_bootstraps_fresh_session_instead_of_live_turn_submit() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, Some(sink));
    for ch in "/new".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(app.startup_shell_visible());

    app.clear_prompt_input();
    for ch in "fresh run".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.should_quit);
    assert!(!app.startup_shell_visible());
    assert!(
        matches!(
            intents.lock().unwrap_or_abort().as_slice(),
            [UiIntent::NewSession]
        ),
        "/new startup handoff must select a fresh session, not submit to the old live run"
    );

    let relaunched = AppState::new_live(None, false, None);
    assert_eq!(relaunched.composer.prompt_buffer, "");
    assert_eq!(
        relaunched.composer.prompt_history,
        vec!["fresh run".to_string()]
    );
    assert_eq!(
        relaunched
            .activities
            .back()
            .and_then(|activity| activity.user_message.as_ref())
            .map(|message| message.text.as_str()),
        Some("fresh run")
    );
}

pub(super) fn startup_mode_uses_pending_launch_metadata() {
    set_pending_live_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );

    let app = AppState::new_startup(Vec::new(), None);

    assert_eq!(app.active_profile(), "worker");
    assert_eq!(app.active_provider(), "mock");
    assert_eq!(app.current_model_label(), "model-1");
    assert_eq!(app.launch_mode_label(), Some("Demo"));
}

pub(super) fn lifecycle_shell_state_transitions() {
    let mut startup = AppState::new_startup(Vec::new(), None);
    startup.composer.prompt_buffer = "draft prompt".to_string();

    assert_eq!(
        startup.lifecycle_shell_state(),
        LifecycleShellState::Startup
    );
    assert!(startup.startup_shell_visible());
    assert!(!startup.post_run_handoff_visible());
    assert!(startup.lifecycle_shell_actions_visible());
    assert_eq!(startup.runtime_state().summary, "startup ready");

    let live = AppState::new_live(None, false, None);

    assert_eq!(live.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!live.startup_shell_visible());
    assert!(!live.post_run_handoff_visible());
    assert!(!live.lifecycle_shell_actions_visible());

    let mut finished = AppState::new_live(Some(PathBuf::from("/tmp/live-finished")), false, None);
    finished.ingest_event(envelope(
        1,
        "req_lifecycle_finished",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "done".to_string(),
        }),
    ));

    assert_eq!(finished.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!finished.startup_shell_visible());
    assert!(!finished.post_run_handoff_visible());
    assert!(!finished.lifecycle_shell_actions_visible());
    assert!(finished.completed_session_shell_active());
    assert!(!finished.composer_disabled());

    let mut failed = AppState::new_live(Some(PathBuf::from("/tmp/live-failed")), false, None);
    failed.ingest_event(envelope(
        1,
        "req_lifecycle_failed",
        EventV1::RunFailed(RunFailedEvent {
            error: "boom".to_string(),
        }),
    ));

    assert_eq!(failed.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!failed.post_run_handoff_visible());
    assert!(!failed.lifecycle_shell_actions_visible());
    assert!(failed.completed_session_shell_active());

    let fallback_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = Arc::new(|_| {});
    let mut missing_session_path = AppState::new_live(None, false, Some(fallback_sink));
    missing_session_path.ingest_event(envelope(
        1,
        "req_lifecycle_missing_path",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "done without persisted path".to_string(),
        }),
    ));

    assert_eq!(
        missing_session_path.lifecycle_shell_state(),
        LifecycleShellState::None
    );
    assert!(!missing_session_path.post_run_handoff_visible());
    assert!(missing_session_path.completed_session_shell_active());
    assert!(!missing_session_path.composer_disabled());

    let mut terminal_without_routing = AppState::new_live(None, false, None);
    terminal_without_routing.ingest_event(envelope(
        1,
        "req_lifecycle_without_routing",
        EventV1::RunFinished(RunFinishedEvent {
            summary: "done without lifecycle routing".to_string(),
        }),
    ));

    assert_eq!(
        terminal_without_routing.lifecycle_shell_state(),
        LifecycleShellState::None
    );
    assert!(!terminal_without_routing.post_run_handoff_visible());
    assert!(terminal_without_routing.completed_session_shell_active());
    assert!(!terminal_without_routing.composer_disabled());
}

pub(super) fn default_shell_registry_exposes_home_and_session_shell_only() {
    let live_registry = default_shell_registry(false);
    assert_eq!(
        live_registry,
        &[
            ShellDescriptor {
                kind: ShellKind::Home,
                label: "Home",
                read_only: false,
            },
            ShellDescriptor {
                kind: ShellKind::Session,
                label: "Session",
                read_only: false,
            },
        ]
    );

    let replay_registry = default_shell_registry(true);
    assert_eq!(
        replay_registry,
        &[
            ShellDescriptor {
                kind: ShellKind::Home,
                label: "Home",
                read_only: false,
            },
            ShellDescriptor {
                kind: ShellKind::Session,
                label: "Replay",
                read_only: true,
            },
        ]
    );
}

pub(super) fn post_run_handoff_ignores_completed_turns_without_terminal_event() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_completed_turn",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_completed_turn".into(),
            text: "status?".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_completed_turn",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_completed_turn".into(),
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "status?".to_string(),
            request_digest: "digest-completed-turn".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_completed_turn",
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_completed_turn".to_string().into(),
            result_summary: "all done".to_string(),
            result_digest: "digest-task-completed-turn".to_string(),
            metadata: None,
        }),
    ));

    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Success);
    assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!app.startup_shell_visible());
    assert!(!app.post_run_handoff_visible());
    assert!(!app.lifecycle_shell_actions_visible());
}

pub(super) fn replay_mode_never_reports_lifecycle_shell_actions() {
    let replay = AppState::new_replay(
        PathBuf::from("/tmp/replay-session"),
        vec![envelope(
            1,
            "req_replay_terminal",
            EventV1::RunFinished(RunFinishedEvent {
                summary: "done".to_string(),
            }),
        )],
    );

    assert_eq!(replay.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!replay.startup_shell_visible());
    assert!(!replay.post_run_handoff_visible());
    assert!(!replay.lifecycle_shell_actions_visible());
}
