use super::*;

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
            intents.lock().expect("lock intents").push(intent);
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
    assert!(intents.lock().expect("lock intents").is_empty());
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
    let tempdir = tempfile::tempdir().expect("tempdir");
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
    let tempdir = tempfile::tempdir().expect("tempdir");
    let history_path = tempdir
        .path()
        .join("sessions")
        .join("tui")
        .join("prompt-history.json");
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
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
        intents.lock().expect("lock intents").as_slice(),
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
        intents.lock().expect("lock intents").as_slice(),
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
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::SubmitPrompt {
            text: "next prompt".to_string(),
            selected_file_tags: Vec::new(),
            selected_agent_tags: Vec::new(),
            selected_resource_tags: Vec::new(),
            launch_metadata: LaunchMetadata::default(),
        }]
    );
}

pub(super) fn composer_word_delete_undo_redo_preserves_graphemes_and_file_tags() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(tempdir.path().join("src")).expect("create src");
    std::fs::write(tempdir.path().join("src/main.rs"), "fn main() {}").expect("write main");
    let mut app = AppState::new_live(None, false, None);
    app.set_file_mention_workspace_root_for_test(tempdir.path().to_path_buf());
    app.focus = Focus::Prompt;

    for ch in "@main".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));
    for ch in " about e\u{301}clair tail".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }

    assert_eq!(
        app.composer.prompt_buffer,
        "@src/main.rs  about e\u{301}clair tail"
    );
    assert_eq!(app.file_mention_tags.len(), 1);
    assert_eq!(app.composer.prompt_cursor, 32);

    app.execute_action(Action::InputDeleteWordBackward);

    assert_eq!(
        app.composer.prompt_buffer,
        "@src/main.rs  about e\u{301}clair "
    );
    assert_eq!(app.composer.prompt_cursor, 28);
    assert_eq!(app.file_mention_tags[0].start, 0);
    assert_eq!(app.file_mention_tags[0].end, "@src/main.rs".chars().count());
    assert_eq!(app.selected_file_tags()[0].source.value, "@src/main.rs");

    app.execute_action(Action::InputUndo);

    assert_eq!(
        app.composer.prompt_buffer,
        "@src/main.rs  about e\u{301}clair tail"
    );
    assert_eq!(app.composer.prompt_cursor, 32);
    assert_eq!(app.selected_file_tags()[0].source.value, "@src/main.rs");

    app.execute_action(Action::InputRedo);

    assert_eq!(
        app.composer.prompt_buffer,
        "@src/main.rs  about e\u{301}clair "
    );
    assert_eq!(app.composer.prompt_cursor, 28);
}

pub(super) fn composer_selection_line_and_buffer_actions_are_grapheme_safe() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.replace_prompt_input("alpha\ne\u{301}clair beta".to_string());

    assert_eq!(app.composer.prompt_cursor, 18);

    app.execute_action(Action::InputLineStart);
    assert_eq!(app.composer.prompt_cursor, 6);

    app.execute_action(Action::InputSelectWordForward);
    assert_eq!(app.composer.selection_range(), Some(6..13));

    app.execute_action(Action::InputDeleteToLineEnd);
    assert_eq!(app.composer.prompt_buffer, "alpha\n");
    assert_eq!(app.composer.selection_range(), None);

    app.execute_action(Action::InputUndo);
    app.execute_action(Action::InputSelectAll);

    assert_eq!(app.composer.selection_range(), Some(0..18));
}

pub(super) fn shell_mode_routes_submit_through_bash_intent_and_exits_at_edges() {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink: Arc<dyn Fn(UiIntent) + Send + Sync> = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| {
            intents.lock().expect("lock intents").push(intent);
        })
    };
    let mut app = AppState::new_live(Some(PathBuf::from("/tmp/session")), false, Some(sink));
    app.focus = Focus::Prompt;

    app.handle_key(key(KeyCode::Char('!')));
    assert_eq!(app.composer.mode(), ComposerMode::Shell);
    assert_eq!(app.composer.prompt_buffer, "");

    for ch in "git status".chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.composer.mode(), ComposerMode::Prompt);
    assert_eq!(app.composer.prompt_buffer, "");
    assert_eq!(
        intents.lock().expect("lock intents").as_slice(),
        &[UiIntent::RunShellCommand {
            command: "git status".to_string(),
        }]
    );

    app.handle_key(key(KeyCode::Char('!')));
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.composer.mode(), ComposerMode::Prompt);

    app.handle_key(key(KeyCode::Char('!')));
    app.handle_key(key(KeyCode::Backspace));
    assert_eq!(app.composer.mode(), ComposerMode::Prompt);
}
