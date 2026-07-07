use super::*;
use crate::UnwrapOrAbort;

pub(super) fn prompt_stash_push_clears_composer_and_persists_entry() {
    let tempdir = tempfile::tempdir().unwrap_or_abort();
    let session_dir = tempdir.path().join("session");
    let stash_path = prompt_stash::prompt_stash_path_for_session_dir(&session_dir);
    let mut app = AppState::new_live_with_prompt_history_path(
        Some(session_dir),
        false,
        None,
        Some(stash_path.clone()),
    );
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "stashed draft".to_string();
    app.composer.prompt_cursor = 7;
    app.composer.selection_anchor = Some(3);

    app.execute_action(Action::PromptStash);

    assert!(app.composer.prompt_buffer.is_empty());
    assert_eq!(app.composer.prompt_cursor, 0);
    assert!(app.composer.selection_anchor.is_none());
    assert_eq!(app.prompt_stash.entries.len(), 1);
    let entry = &app.prompt_stash.entries[0];
    assert_eq!(entry.text, "stashed draft");
    assert_eq!(entry.cursor, 7);
    assert_eq!(entry.selection_anchor, Some(3));
    assert!(stash_path.exists());
}

pub(super) fn prompt_stash_pop_restores_text_cursor_and_selection() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "first draft".to_string();
    app.composer.prompt_cursor = 4;
    app.composer.selection_anchor = Some(2);

    app.execute_action(Action::PromptStash);
    assert!(app.composer.prompt_buffer.is_empty());

    app.execute_action(Action::PromptStashPop);

    assert_eq!(app.composer.prompt_buffer, "first draft");
    assert_eq!(app.composer.prompt_cursor, 4);
    assert_eq!(app.composer.selection_anchor, Some(2));
    assert!(app.prompt_stash.entries.is_empty());
}

pub(super) fn prompt_stash_pop_with_empty_stash_is_noop() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "current".to_string();
    app.composer.prompt_cursor = 3;

    app.execute_action(Action::PromptStashPop);

    assert_eq!(app.composer.prompt_buffer, "current");
    assert_eq!(app.composer.prompt_cursor, 3);
    assert!(app.prompt_stash.entries.is_empty());
}

pub(super) fn prompt_stash_push_with_empty_composer_is_noop() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;

    app.execute_action(Action::PromptStash);

    assert!(app.composer.prompt_buffer.is_empty());
    assert!(app.prompt_stash.entries.is_empty());
}

pub(super) fn prompt_stash_list_dialog_opens_and_closes() {
    let mut app = AppState::new_live(None, false, None);
    app.prompt_stash.entries.push(PromptStashEntry {
        text: "entry one".to_string(),
        cursor: 3,
        selection_anchor: None,
        timestamp: 1_000,
    });

    app.execute_action(Action::PromptStashList);
    assert!(app.prompt_stash.list_visible);
    assert_eq!(app.prompt_stash.list_selected, 0);

    app.handle_key(key(KeyCode::Esc));
    assert!(!app.prompt_stash.list_visible);
}

pub(super) fn prompt_stash_list_dialog_renders_entries() {
    let mut app = AppState::new_live(None, false, None);
    app.prompt_stash.entries.push(PromptStashEntry {
        text: "first stashed".to_string(),
        cursor: 0,
        selection_anchor: None,
        timestamp: 1_000,
    });
    app.prompt_stash.entries.push(PromptStashEntry {
        text: "second stashed".to_string(),
        cursor: 0,
        selection_anchor: None,
        timestamp: 2_000,
    });
    app.prompt_stash.list_visible = true;
    app.prompt_stash.list_selected = 0;

    let rendered = render_debug(&app, 80, 24);
    assert!(rendered.contains("first stashed"));
    assert!(rendered.contains("second stashed"));
    assert!(rendered.contains("Prompt stash"));
}

pub(super) fn prompt_stash_list_delete_removes_selected_entry() {
    let mut app = AppState::new_live(None, false, None);
    app.prompt_stash.entries.push(PromptStashEntry {
        text: "first".to_string(),
        cursor: 0,
        selection_anchor: None,
        timestamp: 1_000,
    });
    app.prompt_stash.entries.push(PromptStashEntry {
        text: "second".to_string(),
        cursor: 0,
        selection_anchor: None,
        timestamp: 2_000,
    });
    app.prompt_stash.list_visible = true;
    app.prompt_stash.list_selected = 0;

    app.handle_key(key_with_modifiers(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL,
    ));

    assert_eq!(app.prompt_stash.entries.len(), 1);
    assert_eq!(app.prompt_stash.entries[0].text, "second");
}

pub(super) fn prompt_stash_list_restore_loads_selected_entry_to_composer() {
    let mut app = AppState::new_live(None, false, None);
    app.prompt_stash.entries.push(PromptStashEntry {
        text: "stashed text".to_string(),
        cursor: 5,
        selection_anchor: Some(2),
        timestamp: 1_000,
    });
    app.prompt_stash.list_visible = true;
    app.prompt_stash.list_selected = 0;

    app.handle_key(key(KeyCode::Enter));

    assert_eq!(app.composer.prompt_buffer, "stashed text");
    assert_eq!(app.composer.prompt_cursor, 5);
    assert_eq!(app.composer.selection_anchor, Some(2));
    assert!(!app.prompt_stash.list_visible);
    assert!(app.prompt_stash.entries.is_empty());
}

pub(super) fn prompt_stash_persists_across_session_restart() {
    let tempdir = tempfile::tempdir().unwrap_or_abort();
    let session_dir = tempdir.path().join("session");
    let stash_path = prompt_stash::prompt_stash_path_for_session_dir(&session_dir);

    let mut first = AppState::new_live_with_prompt_history_path(
        Some(session_dir.clone()),
        false,
        None,
        Some(prompt_history::prompt_history_path_for_session_dir(
            &session_dir,
        )),
    );
    first.focus = Focus::Prompt;
    first.composer.prompt_buffer = "persisted stash".to_string();
    first.composer.prompt_cursor = 5;
    first.execute_action(Action::PromptStash);
    assert!(stash_path.exists());

    let restarted = AppState::new_live_with_prompt_history_path(
        Some(session_dir),
        false,
        None,
        Some(prompt_history::prompt_history_path_for_session_dir(
            &tempdir.path().join("session"),
        )),
    );

    assert_eq!(restarted.prompt_stash.entries.len(), 1);
    assert_eq!(restarted.prompt_stash.entries[0].text, "persisted stash");
    assert_eq!(restarted.prompt_stash.entries[0].cursor, 5);
}

pub(super) fn queued_prompt_count_tracks_queued_activities() {
    let mut app = AppState::new_live(None, false, None);
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
    app.ingest_event(envelope(
        3,
        "req_queued",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_queued".to_string(),
            text: "queued".to_string(),
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

    assert_eq!(app.queued_prompt_count, 1);

    app.ingest_event(envelope(
        5,
        "req_queued",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_queued".to_string(),
            provider_id: "default".to_string(),
            model_id: "gpt-5.4-mini".to_string(),
            prompt_summary: "queued".to_string(),
            request_digest: "digest-queued".to_string(),
            metadata: None,
        }),
    ));

    assert_eq!(app.queued_prompt_count, 0);
}

pub(super) fn queued_prompt_indicator_renders_when_count_positive() {
    let mut app = AppState::new_live(None, false, None);
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
    app.ingest_event(envelope(
        3,
        "req_queued",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_queued".to_string(),
            text: "queued".to_string(),
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

    assert!(app.queued_prompt_count > 0);

    let rendered = render_debug(&app, 140, 40);
    assert!(
        rendered.contains("queued 1"),
        "expected queued indicator in rendered composer metadata"
    );
}
