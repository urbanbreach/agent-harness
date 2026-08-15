use super::*;
use crate::UnwrapOrAbort;
use std::time::{Duration, Instant};

pub(super) fn ctrl_j_inserts_newline_without_submitting() {
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

    assert_eq!(app.composer.prompt_buffer, "hello\nworld");
    assert_eq!(app.composer.prompt_history.len(), 0);
}

pub(super) fn paste_multiline_text_inserts_newlines_without_submitting() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = AppState::new_live(None, false, Some(sink));

    app.handle_paste("alpha\r\n\r\nbeta\rgamma");

    assert_eq!(app.composer.prompt_buffer, "alpha\n\nbeta\ngamma");
    assert_eq!(
        app.composer.prompt_cursor,
        app.composer.prompt_buffer.chars().count()
    );
    assert!(app.composer.prompt_history.is_empty());
    assert!(intents.lock().unwrap_or_abort().is_empty());
}

pub(super) fn multiline_history_keys_move_cursor_before_recalling_history() {
    let mut app = AppState::new_live(None, false, None);
    app.composer.prompt_history = vec!["older prompt".to_string()];
    app.composer.prompt_buffer = "alpha\nbeta".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();

    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.composer.prompt_buffer, "alpha\nbeta");
    assert_eq!(app.composer.prompt_cursor, 4);
    assert_eq!(app.composer.prompt_history_index, None);

    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.composer.prompt_cursor, 4);
    assert_eq!(app.composer.prompt_history_index, None);

    app.composer.prompt_cursor = 0;
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.composer.prompt_buffer, "older prompt");
    assert_eq!(app.composer.prompt_history_index, Some(0));

    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.composer.prompt_buffer, "alpha\nbeta");
    assert_eq!(app.composer.prompt_cursor, 0);
    assert_eq!(app.composer.prompt_history_index, None);
}

pub(super) fn prompt_history_persists_and_restores_draft_after_recall() {
    // arrange
    let tempdir = tempfile::tempdir().unwrap_or_abort();
    let history_path = tempdir
        .path()
        .join("sessions")
        .join("tui")
        .join("prompt-history.json");
    let mut live =
        AppState::new_live_with_prompt_history_path(None, false, None, Some(history_path.clone()));

    // act
    for ch in "persisted prompt".chars() {
        live.handle_key(key(KeyCode::Char(ch)));
    }
    live.handle_key(key(KeyCode::Enter));

    // assert
    assert!(
        history_path.exists(),
        "prompt history should be stored under the session data dir"
    );

    let mut restarted =
        AppState::new_startup_with_prompt_history_path(Vec::new(), None, Some(history_path));
    assert_eq!(
        restarted.composer.prompt_history,
        vec!["persisted prompt".to_string()]
    );

    restarted.focus = Focus::Prompt;
    restarted.composer.prompt_buffer = "draft text".to_string();
    restarted.composer.prompt_cursor = 0;
    restarted.handle_key(key(KeyCode::Up));
    assert_eq!(restarted.composer.prompt_buffer, "persisted prompt");
    assert_eq!(restarted.composer.prompt_history_index, Some(0));

    restarted.handle_key(key(KeyCode::Down));
    assert_eq!(restarted.composer.prompt_buffer, "draft text");
    assert_eq!(restarted.composer.prompt_cursor, 0);
    assert_eq!(restarted.composer.prompt_history_index, None);
}

pub(super) fn startup_auto_submit_persists_prompt_history_once() {
    // arrange
    let tempdir = tempfile::tempdir().unwrap_or_abort();
    let history_path = tempdir
        .path()
        .join("sessions")
        .join("tui")
        .join("prompt-history.json");
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut startup = AppState::new_startup_with_prompt_history_path(
        Vec::new(),
        Some(sink),
        Some(history_path.clone()),
    );

    // act
    for ch in "fresh session".chars() {
        startup.handle_key(key(KeyCode::Char(ch)));
    }
    startup.handle_key(key(KeyCode::Enter));
    let live = AppState::new_live_with_prompt_history_path(None, false, None, Some(history_path));

    // assert
    assert!(matches!(
        intents.lock().unwrap_or_abort().as_slice(),
        [UiIntent::NewSession]
    ));
    assert_eq!(
        live.composer.prompt_history,
        vec!["fresh session".to_string()]
    );
}

pub(super) fn live_bootstrap_auto_submit_echoes_and_emits_first_prompt() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new();
    app.focus = Focus::Prompt;
    app.on_ui_intent = Some(sink);

    app.apply_pending_live_prompt(PendingLivePrompt {
        text: "boot prompt".to_string(),
        auto_submit: true,
    });

    assert!(app.composer.prompt_buffer.is_empty());
    assert_eq!(app.composer.prompt_history, vec!["boot prompt".to_string()]);
    assert_eq!(
        app.activities
            .back()
            .and_then(|activity| activity.user_message.as_ref())
            .map(|message| message.text.as_str()),
        Some("boot prompt")
    );
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::SubmitPrompt {
            text: "boot prompt".to_string(),
            selected_file_tags: Vec::new(),
            selected_agent_tags: Vec::new(),
            selected_resource_tags: Vec::new(),
            attachments: Vec::new(),
            launch_metadata: LaunchMetadata::default(),
        }]
    );
}

pub(super) fn startup_auto_submit_owns_status_over_empty_session_seed() {
    // Given: the startup handoff auto-submits the first prompt before any runtime event exists.
    let mut app = AppState::new();
    app.apply_pending_live_prompt(PendingLivePrompt {
        text: "boot prompt".to_string(),
        auto_submit: true,
    });

    // When: the empty-history live shell applies its startup seed presentation state.
    app.set_starting_session_seed(true);

    // Then: the active turn keeps ownership of the bottom-left live status.
    assert!(!app.starting_session_seed_visible());
    assert!(app.has_live_turn_activity());
    assert!(crate::layout::FrameLayoutPlan::for_app(
        &app,
        ratatui::layout::Rect::new(0, 0, 120, 40),
    )
    .status
    .is_some());
}

pub(super) fn first_esc_on_nonempty_idle_prompt_shows_press_again_hint_without_clearing() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "draft text".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();

    app.handle_key(key(KeyCode::Esc));

    assert_eq!(app.composer.prompt_buffer, "draft text");
    assert!(app.clear_prompt_confirmation_pending());
    assert_eq!(
        app.toast().map(|toast| toast.message.as_str()),
        Some(AppState::clear_prompt_hint_for_test())
    );
    assert!(app.composer.prompt_history.is_empty());
}

pub(super) fn second_esc_within_800ms_clears_prompt_and_saves_history() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "keep me".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();

    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.composer.prompt_buffer, "keep me");
    assert!(app.clear_prompt_confirmation_pending());

    app.handle_key(key(KeyCode::Esc));

    assert!(app.composer.prompt_buffer.is_empty());
    assert!(!app.clear_prompt_confirmation_pending());
    assert_eq!(app.composer.prompt_history, vec!["keep me".to_string()]);
    assert!(app.toast().is_none());
}

pub(super) fn second_esc_after_800ms_restarts_clear_gesture_without_clearing() {
    let base = Instant::now();
    let offset = Arc::new(Mutex::new(Duration::ZERO));
    let clock_offset = Arc::clone(&offset);
    let mut app = AppState::new_live(None, false, None);
    app.set_now_fn_for_test(Arc::new(move || {
        base + *clock_offset.lock().unwrap_or_abort()
    }));
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "still here".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();

    app.handle_key(key(KeyCode::Esc));
    assert!(app.clear_prompt_confirmation_pending());
    assert_eq!(app.composer.prompt_buffer, "still here");

    *offset.lock().unwrap_or_abort() = Duration::from_millis(801);
    app.handle_key(key(KeyCode::Esc));

    assert_eq!(app.composer.prompt_buffer, "still here");
    assert!(app.clear_prompt_confirmation_pending());
    assert!(app.composer.prompt_history.is_empty());
    assert_eq!(
        app.toast().map(|toast| toast.message.as_str()),
        Some(AppState::clear_prompt_hint_for_test())
    );
}

pub(super) fn esc_while_turn_running_does_not_cancel_on_single_press() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = AppState::new_live(None, false, Some(sink));
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "queued draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.ingest_event(envelope(
        1,
        "req_active",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_active".to_string().into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:model-1".to_string()),
            metadata: None,
        }),
    ));

    app.handle_key(key(KeyCode::Esc));

    assert!(
        intents.lock().unwrap_or_abort().is_empty(),
        "single Esc must not emit cancel/interrupt while a turn is running"
    );
    assert!(
        !intents
            .lock()
            .unwrap_or_abort()
            .iter()
            .any(|intent| matches!(intent, UiIntent::InterruptSession { .. })),
        "Esc must not cancel the running turn on first press"
    );
}

pub(super) fn double_esc_while_turn_running_does_not_emit_interrupt() {
    // Given: busy streaming turn with a non-empty draft
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = AppState::new_live(None, false, Some(sink));
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "queued draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.ingest_event(envelope(
        1,
        "req_active",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_active".to_string().into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:model-1".to_string()),
            metadata: None,
        }),
    ));

    // When: Esc twice while the turn is still running
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Esc));

    // Then: no cancel/interrupt intent; draft remains (Esc is mid-turn no-op)
    assert!(
        intents.lock().unwrap_or_abort().is_empty(),
        "double Esc must not emit any UiIntent while a turn is running"
    );
    assert!(
        !intents
            .lock()
            .unwrap_or_abort()
            .iter()
            .any(|intent| matches!(intent, UiIntent::InterruptSession { .. })),
        "double Esc must not emit InterruptSession while a turn is running"
    );
    assert!(!app.interrupt_confirmation_pending());
    assert_eq!(app.composer.prompt_buffer, "queued draft");
}

pub(super) fn ctrl_c_clears_draft_then_cancels_running_turn() {
    // Given: busy turn with a non-empty draft
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = AppState::new_live(None, false, Some(sink));
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "queued draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.ingest_event(envelope(
        1,
        "req_active",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_active".to_string().into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:model-1".to_string()),
            metadata: None,
        }),
    ));

    // When: first Ctrl+C clears draft without canceling
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(app.composer.prompt_buffer, "");
    assert!(
        intents.lock().unwrap_or_abort().is_empty(),
        "first Ctrl+C with non-empty draft must clear only"
    );

    // When: second Ctrl+C cancels the running turn
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));

    // Then: InterruptSession for the active task
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::InterruptSession {
            task_ids: vec!["task_active".to_string()],
            reason: InterruptReason::User,
        }]
    );
}

fn app_waiting_on_background_output(block: bool) -> (AppState, Arc<Mutex<Vec<UiIntent>>>) {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };
    let mut app = AppState::new_live(None, false, Some(sink));
    app.focus = Focus::Prompt;
    app.ingest_event(envelope(
        1,
        "req_send_now",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_send_now".into(),
            text: "wait for the background task".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_send_now",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_send_now".into(),
            state: TaskScheduleState::Started,
            queue_key: Some("provider_model:default:model-1".to_string()),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_send_now",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_send_now".into(),
            provider_id: "default".to_string(),
            model_id: "model-1".to_string(),
            prompt_summary: "wait for the background task".to_string(),
            request_digest: "digest-send-now".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_send_now",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tool_send_now".into(),
            tool_id: "background_output".to_string(),
            args_summary: format!(r#"{{"task_id":"bg_1","block":{block}}}"#),
            args_digest: "digest-send-now-tool".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        5,
        "req_send_now",
        EventV1::ToolCallStarted(ToolCallStartedEvent {
            tool_call_id: "tool_send_now".into(),
        }),
    ));
    app.queued_prompt_count = 1;
    (app, intents)
}

pub(super) fn empty_enter_promotes_queued_prompt_during_sendable_wait() {
    // Given: a queued prompt while background_output is blocking the active turn.
    let (mut app, intents) = app_waiting_on_background_output(true);

    // When: Enter is pressed with an empty composer.
    app.handle_key(key(KeyCode::Enter));

    // Then: Harness interrupts the active turn specifically to promote the queue.
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::InterruptSession {
            task_ids: vec!["task_send_now".to_string()],
            reason: InterruptReason::SendNow,
        }]
    );
}

pub(super) fn empty_enter_does_not_promote_during_nonblocking_background_poll() {
    // Given: a queued prompt while background_output is performing an instant poll.
    let (mut app, intents) = app_waiting_on_background_output(false);

    // When: Enter is pressed with an empty composer.
    app.handle_key(key(KeyCode::Enter));

    // Then: the instant poll is not interrupted or mistaken for a parked wait.
    assert!(intents.lock().unwrap_or_abort().is_empty());
}

pub(super) fn submit_prompt_while_turn_streams_echoes_as_queued_and_emits_intent() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().unwrap_or_abort().push(intent);
        })
    };

    let mut app = AppState::new_live(None, false, Some(sink));
    app.focus = Focus::Prompt;
    app.ingest_event(envelope(
        1,
        "req_active",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_active".into(),
            text: "active".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_active",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_active".into(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "active".to_string(),
            request_digest: "digest-active".to_string(),
            metadata: None,
        }),
    ));
    app.replace_prompt_input("next prompt".to_string());

    app.submit_prompt();

    assert!(app.composer.prompt_buffer.is_empty());
    assert!(
        !app.transcript_page_flip_preserving(),
        "queued local echo must not move the transcript before the turn activates"
    );
    assert_eq!(app.activities.len(), 2);
    assert_eq!(
        app.activities
            .back()
            .and_then(|activity| activity.user_message.as_ref())
            .map(|message| message.text.as_str()),
        Some("next prompt")
    );
    assert_eq!(
        app.activities.back().map(|activity| activity.status),
        Some(ActivityStatus::Queued)
    );
    assert_eq!(
        intents.lock().unwrap_or_abort().as_slice(),
        &[UiIntent::SubmitPrompt {
            text: "next prompt".to_string(),
            selected_file_tags: Vec::new(),
            selected_agent_tags: Vec::new(),
            selected_resource_tags: Vec::new(),
            attachments: Vec::new(),
            launch_metadata: LaunchMetadata::default(),
        }]
    );

    app.ingest_event(envelope(
        3,
        "req_next",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_next".into(),
            text: "next prompt".to_string(),
        }),
    ));
    assert_eq!(
        app.activities.back().map(|activity| activity.status),
        Some(ActivityStatus::Queued)
    );
    assert!(!app.transcript_page_flip_preserving());

    app.ingest_event(envelope(
        4,
        "req_active",
        EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
            request_id: "req_active".into(),
            finish_reason: "stop".to_string(),
            output_digest: None,
            usage: None,
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        5,
        "req_next",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_next".into(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "next prompt".to_string(),
            request_digest: "digest-next".to_string(),
            metadata: None,
        }),
    ));

    assert_eq!(
        app.activities.back().map(|activity| activity.status),
        Some(ActivityStatus::Streaming)
    );
    assert!(
        app.transcript_page_flip_preserving(),
        "queued prompt must page-flip when its event activates the turn"
    );
}
