use super::*;
use crate::UnwrapOrAbort;

pub(super) fn permission_modal_preempts_prompt_submission() {
    let intents = Arc::new(std::sync::Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = app::AppState::new_live(None, false, Some(intent_sink));
    for c in "blocked by permission".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.ingest_event(permission_requested_event(
        1,
        "perm_block_submit",
        "tool_call_block_submit",
    ));

    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let intents = intents.lock().unwrap_or_abort();
    assert_eq!(
        intents.as_slice(),
        &[UiIntent::ResolvePermission {
            permission_id: "perm_block_submit".to_string(),
            decision: harness_core::perm::PermissionDecision::Allow,
            reason: None,
            grant_scope: None,
        }]
    );
    drop(intents);

    assert_eq!(app.composer.prompt_buffer, "blocked by permission");
    assert_eq!(
        app.composer.prompt_cursor,
        "blocked by permission".chars().count()
    );
    assert!(app.composer.prompt_history.is_empty());
    assert!(app.activities.is_empty());
    assert!(app.active_permission().is_some());
}

pub(super) fn continue_disabled_session_shows_reason_banner() {
    let intents = Arc::new(std::sync::Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_prompt_only",
            "/tmp/sessions/run_prompt_only",
            false,
            Some("prompt runs are not resumable"),
        )],
        Some(intent_sink),
    );

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "switch".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let intents = intents.lock().unwrap_or_abort();
    assert!(intents.is_empty());
    drop(intents);
    assert!(app.session_history_visible);
    assert_eq!(
        app.continue_disabled_banner.as_deref(),
        Some("continue unavailable: prompt runs are not resumable")
    );
    assert!(app
        .runtime_state()
        .summary
        .contains("continue unavailable: prompt runs are not resumable"));
}

pub(super) fn replay_session_intent_never_enables_prompt_submission() {
    let intents = Arc::new(std::sync::Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = app::AppState::new_startup(
        vec![startup_session_entry(
            "run_replay",
            "/tmp/sessions/run_replay",
            true,
            None,
        )],
        Some(intent_sink),
    );
    app.composer.prompt_buffer = "do not submit".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();

    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for ch in "new".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(ch)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let intents = intents.lock().unwrap_or_abort();
    assert_eq!(intents.as_slice(), &[UiIntent::NewSession]);
    drop(intents);
    assert_eq!(app.composer.prompt_buffer, "do not submit");
    assert!(app.composer.prompt_history.is_empty());
}

pub(super) fn overlay_wheel_routing_preserved() {
    let frame_area = ratatui::layout::Rect::new(0, 0, 140, 40);
    let mut palette_overlay = app::AppState::new_live(None, false, None);
    palette_overlay.details_scroll = 6;
    palette_overlay.transcript_view.transcript_scroll = 4;
    palette_overlay.transcript_view.follow_mode = false;
    palette_overlay.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    palette_overlay.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 5,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        frame_area,
        Some(crate::ui::WheelTarget::Transcript),
        None,
        None,
    );
    palette_overlay.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 70,
            row: 8,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        frame_area,
        Some(crate::ui::WheelTarget::Inspector),
        None,
        None,
    );

    assert!(palette_overlay.palette_visible);
    assert_eq!(palette_overlay.details_scroll, 6);
    assert_eq!(palette_overlay.transcript_view.transcript_scroll, 4);
    assert!(!palette_overlay.transcript_view.follow_mode);

    let mut permission_overlay = app::AppState::new_live(None, false, None);
    permission_overlay.details_scroll = 8;
    permission_overlay.transcript_view.transcript_scroll = 3;
    permission_overlay.transcript_view.follow_mode = false;
    permission_overlay.ingest_event(permission_requested_event(
        1,
        "perm_overlay_wheel",
        "tool_call_overlay_wheel",
    ));

    permission_overlay.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollUp,
            column: 5,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        frame_area,
        Some(crate::ui::WheelTarget::Transcript),
        None,
        None,
    );
    permission_overlay.handle_mouse(
        crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::ScrollDown,
            column: 70,
            row: 8,
            modifiers: crossterm::event::KeyModifiers::NONE,
        },
        frame_area,
        Some(crate::ui::WheelTarget::Inspector),
        None,
        None,
    );

    assert!(permission_overlay.active_permission().is_some());
    assert_eq!(permission_overlay.details_scroll, 8);
    assert_eq!(permission_overlay.transcript_view.transcript_scroll, 3);
    assert!(!permission_overlay.transcript_view.follow_mode);
}

pub(super) fn replay_secondary_surfaces_remain_reachable_after_live_shell_refactor() {
    let mut replay = app::AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-session"),
        session_view_events(),
    );

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
    replay.handle_key(key(crossterm::event::KeyCode::Esc));

    replay.handle_key(key(crossterm::event::KeyCode::Char('?')));
    assert_eq!(replay.review_surface(), Some(app::ReviewSurface::Help));
    let replay_help_debug = render_live_buffer(&replay, 80, 24);
    assert!(!replay_help_debug.contains("Tabs"));
    assert!(replay_help_debug.contains("Replay · read-only"));
    assert!(replay_help_debug.contains("read-only"));
    assert!(replay_help_debug.contains("Keyboard Shortcuts:"));

    replay.handle_key(key(crossterm::event::KeyCode::Esc));
    assert_eq!(replay.review_surface(), None);
}

pub(super) fn composer_enter_submits_and_shift_enter_inserts_newline() {
    use std::sync::Mutex;

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = app::AppState::new_live(None, false, Some(intent_sink));

    for c in "hello".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::SHIFT,
    ));
    for c in "world".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let intents = intents.lock().unwrap_or_abort();
    assert_eq!(
        intents.as_slice(),
        &[UiIntent::SubmitPrompt {
            text: "hello\nworld".to_string(),
            selected_file_tags: Vec::new(),
            selected_agent_tags: Vec::new(),
            selected_resource_tags: Vec::new(),
            launch_metadata: app::LaunchMetadata::default(),
        }]
    );
    drop(intents);

    assert!(app.composer.prompt_buffer.is_empty());
    assert_eq!(
        app.composer.prompt_history.last().map(String::as_str),
        Some("hello\nworld")
    );

    let activity = app.activities.back().unwrap_or_abort();
    assert_eq!(
        activity
            .user_message
            .as_ref()
            .map(|message| message.text.as_str()),
        Some("hello\nworld")
    );
    assert_eq!(activity.status, app::ActivityStatus::Streaming);
}

pub(super) fn composer_ctrl_j_inserts_newline() {
    let mut app = app::AppState::new_live(None, false, None);

    for c in "hello".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key_with_modifiers(
        crossterm::event::KeyCode::Char('j'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    for c in "world".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    assert_eq!(app.composer.prompt_buffer, "hello\nworld");
}

pub(super) fn composer_submits_queued_followup_while_streaming() {
    use std::sync::Mutex;

    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let intent_sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = app::AppState::new_live(None, false, Some(intent_sink));

    for c in "first".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }
    app.handle_key(key(crossterm::event::KeyCode::Enter));

    for c in "next".chars() {
        app.handle_key(key(crossterm::event::KeyCode::Char(c)));
    }

    app.ingest_event(envelope(
        1,
        Some("req_001"),
        harness_core::event::EventV1::ProviderRequestStarted(
            harness_core::event::ProviderRequestStartedEvent {
                request_id: "req_001".to_string(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5-codex".to_string(),
                prompt_summary: "first".to_string(),
                request_digest: "digest-1".to_string(),
                metadata: None,
            },
        ),
    ));
    app.ingest_event(envelope(
        2,
        Some("req_001"),
        harness_core::event::EventV1::ProviderStreamDelta(
            harness_core::event::ProviderStreamDeltaEvent {
                request_id: "req_001".to_string(),
                delta: "streaming".to_string(),
            },
        ),
    ));

    app.handle_key(key(crossterm::event::KeyCode::Enter));

    let intents = intents.lock().unwrap_or_abort();
    assert_eq!(
        intents.as_slice(),
        &[
            UiIntent::SubmitPrompt {
                text: "first".to_string(),
                selected_file_tags: Vec::new(),
                selected_agent_tags: Vec::new(),
                selected_resource_tags: Vec::new(),
                launch_metadata: app::LaunchMetadata::default(),
            },
            UiIntent::SubmitPrompt {
                text: "next".to_string(),
                selected_file_tags: Vec::new(),
                selected_agent_tags: Vec::new(),
                selected_resource_tags: Vec::new(),
                launch_metadata: app::LaunchMetadata::default(),
            },
        ]
    );
    drop(intents);

    assert!(app.composer.prompt_buffer.is_empty());
    assert_eq!(app.composer.prompt_cursor, 0);
    assert_eq!(
        app.composer.prompt_history.last().map(String::as_str),
        Some("next")
    );
    let activity = app.activities.front().unwrap_or_abort();
    assert_eq!(activity.request_id, "req_001");
    assert_eq!(activity.transcript_text, "streaming");
    assert_eq!(activity.status, app::ActivityStatus::Streaming);
    let queued_activity = app.activities.back().unwrap_or_abort();
    assert_eq!(
        queued_activity
            .user_message
            .as_ref()
            .map(|message| message.text.as_str()),
        Some("next")
    );
}

pub(super) fn session_shell_registry_only_exposes_home_and_session_shells() {
    let live_registry = app::default_shell_registry(false);
    assert_eq!(live_registry.len(), 2);
    assert_eq!(live_registry[0].label, "Home");
    assert_eq!(live_registry[1].label, "Session");

    let replay_registry = app::default_shell_registry(true);
    assert_eq!(replay_registry.len(), 2);
    assert_eq!(replay_registry[0].label, "Home");
    assert_eq!(replay_registry[1].label, "Replay");

    let replay = app::AppState::new_replay(
        std::path::PathBuf::from("/tmp/replay-session"),
        session_view_events(),
    );
    assert!(!replay.details_drawer_open());
    assert_eq!(replay.review_surface(), None);
}

pub(super) fn replay_mode_does_not_render_orchestration_summary() {
    let mut events = session_view_events();
    events.extend([
        envelope(
            100,
            None,
            harness_core::event::EventV1::AgentSpawned(harness_core::event::AgentSpawnedEvent {
                agent_id: "agent_replay".to_string(),
                profile: "researcher".to_string(),
                parent_agent_id: None,
            }),
        ),
        envelope_with_actor(
            101,
            Some("req_replay_orch"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("agent_replay".to_string()),
            ),
            harness_core::event::EventV1::TaskScheduled(harness_core::event::TaskScheduledEvent {
                task_id: "task_replay_orch".to_string(),
                state: harness_core::event::TaskScheduleState::Queued,
                queue_key: Some("agent:queued:replay".to_string()),
            }),
        ),
        envelope_with_actor(
            102,
            Some("req_replay_orch"),
            harness_core::event::EventActor::new(
                harness_core::event::ActorKind::Worker,
                Some("agent_replay".to_string()),
            ),
            harness_core::event::EventV1::StaleDetected(harness_core::event::StaleDetectedEvent {
                task_id: "task_replay_orch".to_string(),
                stale_for_ms: 3001,
            }),
        ),
    ]);

    let mut replay =
        app::AppState::new_replay(std::path::PathBuf::from("/tmp/replay-session"), events);

    let replay_run = render_live_lines(&replay, 120, 30);
    assert!(!replay_run.contains("Orchestration"));
    assert!(!replay_run.contains("agents "));
    assert!(!replay.details_drawer_open());

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

    replay.handle_key(key(crossterm::event::KeyCode::Char('?')));
    let replay_help = render_live_lines(&replay, 120, 30);
    assert!(!replay_help.contains("Orchestration"));
    assert!(!replay_help.contains("agents "));
}
