use super::*;
use crate::overlay::OverlayKind;
use crate::ui::WheelTarget;
use crossterm::event::MouseEvent;
use harness_core::event::{
    ActorKind, EventActor, EventEnvelopeV1, EventV1, PermissionRequestedEvent,
    ProviderRequestStartedEvent, RunFailedEvent, RunFinishedEvent, TaskCompletedEvent,
    UserMessageSubmittedEvent, SCHEMA_VERSION,
};

fn envelope(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt_app_{seq:04}"),
        seq,
        run_id: "run_app_tests".to_string(),
        mono_ms: seq,
        ts: Some("2026-02-03T12:00:00Z".to_string()),
        actor: EventActor::new(ActorKind::System, Some("app-tests".to_string())),
        correlation_id: Some(request_id.to_string()),
        causation_id: None,
        stream_key: None,
        payload,
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key_with_modifiers(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn overlay_stack_orders_details_palette_permission() {
    let mut app = AppState::new_live(None, false, None);
    app.live_details_drawer_open = true;

    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(
        app.overlay_stack().ordered(),
        &[OverlayKind::DetailsDrawer, OverlayKind::CommandPalette]
    );

    app.ingest_event(envelope(
        1,
        "req_overlay_stack",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_overlay_stack".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_overlay_stack".to_string()),
            summary: "permission summary".to_string(),
            request_digest: "digest-overlay-stack".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    assert_eq!(
        app.overlay_stack().ordered(),
        &[OverlayKind::DetailsDrawer, OverlayKind::PermissionModal]
    );
}

#[test]
fn overlay_stack_orders_permission_above_commands_and_slash() {
    AppState::exact_test_overlay_stack_orders_permission_above_commands_and_slash();
}

#[test]
fn permission_modal_preempts_palette() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    app.handle_key(key(KeyCode::Char('d')));

    app.ingest_event(envelope(
        1,
        "req_overlay_preempt",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_overlay_preempt".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_overlay_preempt".to_string()),
            summary: "permission summary".to_string(),
            request_digest: "digest-overlay-preempt".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    app.handle_key(key_with_modifiers(
        KeyCode::Char('y'),
        KeyModifiers::CONTROL,
    ));

    assert!(!app.palette_visible);
    assert!(app.palette_input.is_empty());
    assert_eq!(
        app.overlay_stack().top(),
        Some(OverlayKind::PermissionModal)
    );
    let intents = intents.lock().expect("lock intents");
    assert_eq!(
        intents.as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_overlay_preempt".to_string(),
            decision: PermissionDecision::Allow,
        }]
    );
}

#[test]
fn permission_modal_routes_q_to_quit_without_buffering() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(intent_sink));
    app.prompt_buffer = "keep this draft".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();
    app.ingest_event(envelope(
        1,
        "req_modal_quit",
        EventV1::PermissionRequested(PermissionRequestedEvent {
            permission_id: "perm_modal_quit".to_string(),
            kind: "edit_fs".to_string(),
            tool_call_id: Some("tc_modal_quit".to_string()),
            summary: "permission summary".to_string(),
            request_digest: "digest-modal-quit".to_string(),
            timeout_ms: 30_000,
            default_decision: harness_core::event::PermissionDecision::Deny,
        }),
    ));

    app.handle_key(key(KeyCode::Char('q')));

    assert!(app.should_quit);
    assert_eq!(app.prompt_buffer, "keep this draft");
    let intents = intents.lock().expect("lock intents");
    assert_eq!(intents.as_slice(), &[UiIntent::QuitRequested]);
}

#[test]
fn focus_returns_after_palette_close() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Details;

    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    assert!(app.palette_visible);
    assert_eq!(app.focus, Focus::Details);

    app.handle_key(key(KeyCode::Esc));
    assert!(!app.palette_visible);
    assert_eq!(app.focus, Focus::Details);
}

#[test]
fn details_drawer_toggles_without_stealing_transcript_state() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_a",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_a".to_string(),
            text: "First".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_a",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_a".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "First".to_string(),
            request_digest: "digest-a".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_b",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_b".to_string(),
            text: "Second".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_b",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_b".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Second".to_string(),
            request_digest: "digest-b".to_string(),
        }),
    ));

    app.follow_mode = false;
    app.focus = Focus::Details;
    app.selected_activity_index = 0;
    app.details_scroll = 7;

    app.handle_key(key(KeyCode::Char('i')));
    assert!(app.details_drawer_open());
    assert_eq!(app.active_tab, Tab::Run);
    assert_eq!(app.focus, Focus::Details);
    assert!(!app.follow_mode);
    assert_eq!(app.selected_activity_index, 0);
    assert_eq!(app.details_scroll, 7);

    app.handle_key(key(KeyCode::Char('i')));
    assert!(!app.details_drawer_open());
    assert_eq!(app.active_tab, Tab::Run);
    assert_eq!(app.focus, Focus::Details);
    assert!(!app.follow_mode);
    assert_eq!(app.selected_activity_index, 0);
    assert_eq!(app.details_scroll, 7);
}

#[test]
fn config_backed_live_launch_starts_in_session_shell_without_details_drawer() {
    set_pending_live_launch_metadata(
        LaunchMetadata::new("deep", "default", Some("gpt-5.3-codex".to_string()))
            .with_mode_label("Live"),
    );

    let app = AppState::new_live(None, false, None);

    assert!(!app.details_drawer_open());
    assert_eq!(app.focus, Focus::Prompt);
}

#[test]
fn mouse_wheel_scrolls_transcript_without_stealing_focus() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        Some(WheelTarget::Transcript),
    );
    assert!(!app.follow_mode);
    assert_eq!(app.transcript_scroll, 3);
    assert_eq!(app.focus, Focus::Prompt);

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        Some(WheelTarget::Transcript),
    );
    assert_eq!(app.transcript_scroll, 0);
    assert!(app.follow_mode);
    assert_eq!(app.focus, Focus::Prompt);
}

#[test]
fn mouse_wheel_scrolls_inspector_when_hovered() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::List;
    app.details_scroll = 2;
    app.transcript_scroll = 4;

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 90,
            row: 12,
            modifiers: KeyModifiers::NONE,
        },
        Some(WheelTarget::Inspector),
    );
    assert_eq!(app.details_scroll, 5);
    assert_eq!(app.transcript_scroll, 4);
    assert_eq!(app.focus, Focus::List);

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 90,
            row: 12,
            modifiers: KeyModifiers::NONE,
        },
        Some(WheelTarget::Inspector),
    );
    assert_eq!(app.details_scroll, 2);
    assert_eq!(app.transcript_scroll, 4);
    assert_eq!(app.focus, Focus::List);
}

#[test]
fn mouse_wheel_ignores_non_scrollable_areas() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.details_scroll = 6;
    app.transcript_scroll = 2;
    app.follow_mode = false;

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 2,
            row: 2,
            modifiers: KeyModifiers::NONE,
        },
        None,
    );

    assert_eq!(app.details_scroll, 6);
    assert_eq!(app.transcript_scroll, 2);
    assert!(!app.follow_mode);
    assert_eq!(app.focus, Focus::Prompt);
}

#[test]
fn historical_task_completed_marks_turn_done_and_unblocks_first_resumed_submit() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = intents.clone();
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(sink));
    app.ingest_event(envelope(
        1,
        "req_resume_1",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_resume_1".to_string(),
            text: "previous question".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_resume_1",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_resume_1".to_string(),
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "previous question".to_string(),
            request_digest: "digest-resume-1".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_resume_1",
        EventV1::ProviderStreamDelta(harness_core::event::ProviderStreamDeltaEvent {
            request_id: "req_resume_1".to_string(),
            delta: "previous answer".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_resume_1",
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_000123".to_string(),
            result_summary: "previous answer".to_string(),
            result_digest: "digest-task-123".to_string(),
        }),
    ));

    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Success);

    for c in "next".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));

    let intents = intents.lock().expect("lock intents");
    assert!(
        intents
            .iter()
            .any(|intent| matches!(intent, UiIntent::SubmitPrompt { text } if text == "next")),
        "historical streaming residue should not block first resumed submit"
    );
}

#[test]
fn historical_terminal_events_stay_in_session_shell_after_live_finish() {
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

#[test]
fn continued_quiescent_bootstrap_stays_in_session_shell_without_handoff() {
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

#[test]
fn startup_prompt_enter_emits_submit_intent_and_quits_launcher() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_startup(Vec::new(), Some(sink));

    for c in "ship it".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.should_quit, "startup submit should leave the launcher");
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::SubmitPrompt {
            text: "ship it".to_string(),
        }]
    );
}

#[test]
fn ctrl_j_inserts_newline_without_submitting() {
    let mut app = AppState::new_live(None, false, None);

    for c in "hello".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key_with_modifiers(
        KeyCode::Char('j'),
        KeyModifiers::CONTROL,
    ));
    for c in "world".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }

    assert_eq!(app.prompt_buffer, "hello\nworld");
    assert_eq!(app.prompt_history.len(), 0);
}

#[test]
fn multiline_history_keys_move_cursor_before_recalling_history() {
    let mut app = AppState::new_live(None, false, None);
    app.prompt_history = vec!["older prompt".to_string()];
    app.prompt_buffer = "alpha\nbeta".to_string();
    app.prompt_cursor = app.prompt_buffer.chars().count();

    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.prompt_buffer, "alpha\nbeta");
    assert_eq!(app.prompt_cursor, 4);
    assert_eq!(app.prompt_history_index, None);

    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.prompt_cursor, 4);
    assert_eq!(app.prompt_history_index, None);

    app.prompt_cursor = 0;
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.prompt_buffer, "older prompt");
    assert_eq!(app.prompt_history_index, Some(0));

    app.handle_key(key(KeyCode::Down));
    assert!(app.prompt_buffer.is_empty());
    assert_eq!(app.prompt_history_index, None);
}

#[test]
fn live_bootstrap_auto_submit_echoes_and_emits_first_prompt() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new();
    app.focus = Focus::Prompt;
    app.on_ui_intent = Some(sink);

    app.apply_pending_live_prompt(PendingLivePrompt {
        text: "boot prompt".to_string(),
        auto_submit: true,
    });

    assert!(app.prompt_buffer.is_empty());
    assert_eq!(app.prompt_history, vec!["boot prompt".to_string()]);
    assert_eq!(
        app.activities
            .back()
            .and_then(|activity| activity.user_message.as_ref())
            .map(|message| message.text.as_str()),
        Some("boot prompt")
    );
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::SubmitPrompt {
            text: "boot prompt".to_string(),
        }]
    );
}

#[test]
fn replay_mode_focus_cycle_skips_prompt_and_blocks_draft_edits() {
    let mut app = AppState::new_replay(PathBuf::from("/tmp/replay-session"), Vec::new());

    assert_eq!(app.focus, Focus::Details);

    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.focus, Focus::Details);

    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.focus, Focus::Details);

    app.focus = Focus::Prompt;
    app.handle_key(key(KeyCode::Char('x')));
    assert!(app.prompt_buffer.is_empty());
}

#[test]
fn slash_menu_closes_after_whitespace() {
    let mut app = AppState::new_startup(Vec::new(), None);

    app.handle_key(key(KeyCode::Char('/')));
    app.handle_key(key(KeyCode::Char('n')));
    assert!(app.slash_visible);

    app.handle_key(key(KeyCode::Char(' ')));

    assert!(!app.slash_visible);
    assert_eq!(app.prompt_buffer, "/n ");
}

#[test]
fn slash_exit_matches_quit_requested_behavior() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };

    let mut app = AppState::new_startup(Vec::new(), Some(sink));
    for ch in "/exit".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert!(app.should_quit);
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::QuitRequested]
    );
}

#[test]
fn startup_mode_uses_pending_launch_metadata() {
    set_pending_live_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );

    let app = AppState::new_startup(Vec::new(), None);

    assert_eq!(app.active_profile(), "worker");
    assert_eq!(app.active_provider(), "mock");
    assert_eq!(app.current_model_label(), "model-1");
    assert_eq!(app.launch_mode_label(), Some("Demo"));
}

#[test]
fn lifecycle_shell_state_transitions() {
    let mut startup = AppState::new_startup(Vec::new(), None);
    startup.prompt_buffer = "draft prompt".to_string();

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

#[test]
fn default_shell_registry_exposes_home_and_session_shell_only() {
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

#[test]
fn post_run_handoff_ignores_completed_turns_without_terminal_event() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_completed_turn",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_completed_turn".to_string(),
            text: "status?".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_completed_turn",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_completed_turn".to_string(),
            provider_id: "mock".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "status?".to_string(),
            request_digest: "digest-completed-turn".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_completed_turn",
        EventV1::TaskCompleted(TaskCompletedEvent {
            task_id: "task_completed_turn".to_string(),
            result_summary: "all done".to_string(),
            result_digest: "digest-task-completed-turn".to_string(),
        }),
    ));

    assert_eq!(app.runtime_state().kind, RuntimeStateKind::Success);
    assert_eq!(app.lifecycle_shell_state(), LifecycleShellState::None);
    assert!(!app.startup_shell_visible());
    assert!(!app.post_run_handoff_visible());
    assert!(!app.lifecycle_shell_actions_visible());
}

#[test]
fn replay_mode_never_reports_lifecycle_shell_actions() {
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
