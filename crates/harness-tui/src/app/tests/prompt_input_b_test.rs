use super::*;

pub(super) fn shell_mode_is_unavailable_in_startup_and_replay() {
    let mut startup = AppState::new_startup(Vec::new(), None);
    startup.focus = Focus::Prompt;
    startup.handle_key(key(KeyCode::Char('!')));

    assert_eq!(startup.composer.mode(), ComposerMode::Prompt);
    assert_eq!(startup.composer.prompt_buffer, "");
    assert_eq!(
        startup.status_banner.as_deref(),
        Some("Shell mode requires an active session")
    );

    let mut replay = AppState::new_replay(PathBuf::from("/tmp/replay-session"), Vec::new());
    replay.focus = Focus::Prompt;
    replay.handle_key(key(KeyCode::Char('!')));

    assert_eq!(replay.composer.mode(), ComposerMode::Prompt);
    assert_eq!(replay.composer.prompt_buffer, "");
}

pub(super) fn prompt_stash_save_pop_list_delete_and_queue_indicator_work() {
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.focus = Focus::Prompt;
    app.replace_prompt_input("draft one".to_string());
    app.composer.prompt_cursor = 5;

    app.execute_action(Action::PromptStash);

    assert_eq!(app.composer.prompt_buffer, "");
    assert_eq!(app.composer.stash_len(), 1);
    assert!(!app.composer.stash_dialog_visible());

    app.replace_prompt_input("draft two".to_string());
    app.composer.prompt_cursor = 6;
    app.execute_action(Action::PromptStash);
    assert_eq!(app.composer.stash_len(), 2);

    app.execute_action(Action::PromptStashList);
    assert!(app.composer.stash_dialog_visible());

    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.composer.prompt_buffer, "draft one");
    assert_eq!(app.composer.prompt_cursor, 5);
    assert_eq!(app.composer.stash_len(), 1);

    app.execute_action(Action::PromptStashList);
    app.handle_key(key(KeyCode::Delete));
    assert_eq!(app.composer.stash_len(), 0);

    app.replace_prompt_input("draft three".to_string());
    app.composer.prompt_cursor = 7;
    app.execute_action(Action::PromptStash);
    app.execute_action(Action::PromptStashPop);

    assert_eq!(app.composer.prompt_buffer, "draft three");
    assert_eq!(app.composer.prompt_cursor, 7);

    app.set_pending_prompt_count_for_test(2);
    assert_eq!(app.composer.queued_prompt_count(), 2);

    app.execute_action(Action::QueuedPrompts);
    assert!(app.composer.queued_prompt_dialog_visible());
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Delete));
    assert_eq!(app.composer.queued_prompt_count(), 1);
    assert_eq!(
        app.composer
            .queued_prompt_entries()
            .first()
            .map(|entry| entry.text()),
        Some("queued prompt 2")
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.composer.queued_prompt_dialog_visible());
}

pub(super) fn prompt_stash_overlay_renders_prompt_previews() {
    // Given
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);
    app.focus = Focus::Prompt;
    app.replace_prompt_input("draft one for later".to_string());
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.execute_action(Action::PromptStash);

    // When
    app.execute_action(Action::PromptStashList);
    let rendered = render_prompt_input_screen(&app, 100, 30);

    // Then
    assert!(rendered.contains("Prompt stash"), "{rendered}");
    assert!(rendered.contains("draft one for later"), "{rendered}");
}

pub(super) fn prompt_stash_overlay_renders_empty_state() {
    // Given
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, None);

    // When
    app.execute_action(Action::PromptStashList);
    let rendered = render_prompt_input_screen(&app, 100, 30);

    // Then
    assert!(rendered.contains("Prompt stash"), "{rendered}");
    assert!(rendered.contains("No stashed prompts"), "{rendered}");
}

pub(super) fn queued_prompt_overlay_renders_queued_previews() {
    // Given
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.ingest_event(envelope(
        1,
        "req_active",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_active".to_string(),
            text: "active".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_active",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_active".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "active".to_string(),
            request_digest: "digest-active".to_string(),
            metadata: None,
        }),
    ));
    app.composer.prompt_buffer = "follow up while busy".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.submit_prompt();

    // When
    app.execute_action(Action::QueuedPrompts);
    let rendered = render_prompt_input_screen(&app, 100, 30);

    // Then
    assert_eq!(app.composer.queued_prompt_count(), 1);
    assert!(rendered.contains("Queued prompts"), "{rendered}");
    assert!(rendered.contains("follow up while busy"), "{rendered}");
}

pub(super) fn queued_prompt_delete_cancels_scheduled_turn() {
    // Given
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_live(None, false, Some(sink));
    app.focus = Focus::Prompt;
    app.ingest_event(envelope(
        1,
        "req_active",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_active".to_string(),
            text: "active".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_active",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_active".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "active".to_string(),
            request_digest: "digest-active".to_string(),
            metadata: None,
        }),
    ));
    app.composer.prompt_buffer = "cancel before run".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.submit_prompt();
    app.ingest_event(envelope(
        3,
        "req_queued",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_queued".to_string(),
            text: "cancel before run".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_queued",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_queued".to_string(),
            state: TaskScheduleState::Queued,
            queue_key: Some("provider_model:default:gpt-5.4-mini".to_string()),
        }),
    ));

    // When
    app.execute_action(Action::QueuedPrompts);
    app.handle_key(key(KeyCode::Delete));

    // Then
    assert_eq!(app.composer.queued_prompt_count(), 0);
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[
            UiIntent::SubmitPrompt {
                text: "cancel before run".to_string(),
                selected_file_tags: Vec::new(),
                selected_agent_tags: Vec::new(),
                selected_resource_tags: Vec::new(),
                launch_metadata: LaunchMetadata::default(),
            },
            UiIntent::CancelQueuedPrompt {
                task_id: "task_queued".to_string(),
            },
        ]
    );
}

pub(super) fn queued_prompt_delete_before_task_id_cancels_when_scheduled() {
    // Given
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_live(None, false, Some(sink));
    app.focus = Focus::Prompt;
    app.ingest_event(envelope(
        1,
        "req_active",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_active".to_string(),
            text: "active".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_active",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_active".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "active".to_string(),
            request_digest: "digest-active".to_string(),
            metadata: None,
        }),
    ));
    app.composer.prompt_buffer = "cancel while pending id".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.submit_prompt();
    app.execute_action(Action::QueuedPrompts);
    app.handle_key(key(KeyCode::Delete));

    // When
    app.ingest_event(envelope(
        3,
        "req_queued",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_queued".to_string(),
            text: "cancel while pending id".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_queued",
        EventV1::TaskScheduled(TaskScheduledEvent {
            task_id: "task_late_queued".to_string(),
            state: TaskScheduleState::Queued,
            queue_key: Some("provider_model:default:gpt-5.4-mini".to_string()),
        }),
    ));

    // Then
    assert_eq!(app.composer.queued_prompt_count(), 0);
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[
            UiIntent::SubmitPrompt {
                text: "cancel while pending id".to_string(),
                selected_file_tags: Vec::new(),
                selected_agent_tags: Vec::new(),
                selected_resource_tags: Vec::new(),
                launch_metadata: LaunchMetadata::default(),
            },
            UiIntent::CancelQueuedPrompt {
                task_id: "task_late_queued".to_string(),
            },
        ]
    );
}

pub(super) fn queued_prompt_overlay_renders_empty_state() {
    // Given
    let mut app = AppState::new_live(None, false, None);

    // When
    app.execute_action(Action::QueuedPrompts);
    let rendered = render_prompt_input_screen(&app, 100, 30);

    // Then
    assert!(rendered.contains("Queued prompts"), "{rendered}");
    assert!(rendered.contains("No queued prompts"), "{rendered}");
}

fn render_prompt_input_screen(app: &AppState, width: u16, height: u16) -> String {
    let backend = ratatui::backend::TestBackend::new(width, height);
    let mut terminal = ratatui::Terminal::new(backend).expect("create prompt input terminal");
    terminal
        .draw(|frame| render_app(frame, app))
        .expect("draw prompt input frame");
    terminal
        .backend()
        .buffer()
        .content
        .chunks(usize::from(width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}
