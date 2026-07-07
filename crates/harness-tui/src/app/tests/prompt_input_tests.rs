use super::*;
use crate::UnwrapOrAbort;

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
            launch_metadata: LaunchMetadata::default(),
        }]
    );
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
    app.composer.prompt_buffer = "next prompt".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();

    app.submit_prompt();

    assert!(app.composer.prompt_buffer.is_empty());
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
            launch_metadata: LaunchMetadata::default(),
        }]
    );
}
