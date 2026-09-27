use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::app::AppState;
use harness_tui::attachment_lifecycle::{AttachmentIngestor, AttachmentPolicy, CancellationToken};
use harness_tui::completion_controller::{
    CompletionItem, CompletionRange, CompletionSource, CompletionTrigger,
};
use harness_tui::composer_atoms::AttachmentId;
use harness_tui::prompt_queue_actions::QueueAction;
use harness_tui::theme_tokens::ViewportId;

#[test]
fn production_app_state_routes_keyboard_input_through_atom_composer() {
    // arrange
    // Given: a live AppState with the production key dispatcher.
    let mut app = AppState::new_live(None, false, None);

    // When: a user types through the real AppState boundary.
    app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

    // Then: the production view model exposes grapheme-backed composer state and submission data.
    let view = app.composer_view_model(ViewportId::Standard100x30);
    assert_eq!(view.text, "hi");
    assert_eq!(view.wrapped_atom_ids().len(), 2);

    // act
    let submission = app
        .composer_submission()
        .expect("valid production composer submission");
    // assert
    assert_eq!(submission.text, "hi");
    assert!(submission.attachments.is_empty());
}

#[test]
fn production_app_state_routes_completion_and_queue_actions() {
    // arrange
    // Given: a live production composer with a completion request and an idle queue.
    let mut app = AppState::new_live(None, false, None);
    let request = app.composer_begin_completion(CompletionTrigger::new(
        CompletionRange::new(0, 0).expect("valid completion range"),
        "",
        CompletionSource::Slash,
    ));
    app.composer_apply_completion_results(
        &request,
        vec![CompletionItem::new(1, "status", "status")],
    )
    .expect("current completion results apply");

    // When: Enter accepts the active completion and the queue is edited through AppState.
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.composer_apply_queue_action(QueueAction::Queue {
        queued_id: "queued-1".to_owned(),
        text: "queued text".to_owned(),
    })
    .expect("queue action applies");
    app.composer_apply_queue_action(QueueAction::Edit {
        queued_id: "queued-1".to_owned(),
        text: "edited text".to_owned(),
    })
    .expect("queue edit applies");

    // act
    // Then: completion output and queue state are owned by the production composer.
    // assert
    assert_eq!(
        app.composer_view_model(ViewportId::Standard100x30).text,
        "status"
    );
    assert_eq!(app.composer_queue_state().queued[0].text, "edited text");
}

#[test]
fn production_app_state_routes_attachment_ingest_and_submit() {
    // arrange
    // Given: a bounded attachment ingestor and a real AppState composer.
    let root = tempfile::tempdir().expect("temporary workspace");
    let policy = AttachmentPolicy::new(root.path()).expect("workspace policy");
    let attachment = AttachmentIngestor::new(policy)
        .ingest_clipboard(b"hello attachment", &CancellationToken::new())
        .expect("clipboard attachment");
    let mut app = AppState::new_live(None, false, None);

    // When: the attachment is inserted into the production composer and submitted.
    app.composer_attach(AttachmentId::new(7), attachment)
        .expect("attachment attaches");
    let submission = app.composer_submission().expect("attachment submission");

    // act
    // Then: attachment identity and bytes survive the typed production submission boundary.
    // assert
    assert_eq!(submission.attachments.len(), 1);
    assert_eq!(submission.attachments[0].id, AttachmentId::new(7));
    assert_eq!(submission.attachments[0].bytes, b"hello attachment");
}

// Invoke shipped key bindings through the public dispatcher, including the
// surviving Ctrl+Shift+Z redo binding rather than the conflicting Ctrl+Y.
// Buffer-start has no surviving default: its Ctrl+Home becomes FirstMessage.
fn shortcut(app: &mut AppState, action: harness_tui::Action) {
    let event = app
        .keymap
        .get_bindings(action)
        .into_iter()
        .map(|key| KeyEvent::new(key.code, key.modifiers))
        .find(|event| app.keymap.get_action(event) == Some(action));
    assert!(event.is_some(), "no surviving key binding for {action:?}");
    if let Some(event) = event {
        app.handle_key(event);
    }
}

fn draft(text: &str, cursor: usize, anchor: Option<usize>) -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.composer.prompt_buffer = text.into();
    app.composer.prompt_cursor = cursor;
    app.composer.selection_anchor = anchor;
    app.keymap
        .apply_overrides(&std::collections::BTreeMap::from([(
            "move_buffer_start".into(),
            "F12".into(),
        )]));
    app
}

#[test]
fn prompt_shortcuts_preserve_boundaries_and_selection() {
    use harness_tui::Action::*;
    let text = "界\none.two tail\nlast";
    for (action, cursor, anchor) in [
        (MoveWordLeft, 6, None),
        (MoveWordRight, 10, None),
        (MoveLineStart, 2, None),
        (MoveLineEnd, 14, None),
        (MoveBufferStart, 0, None),
        (MoveBufferEnd, 19, None),
        (CursorLeft, 7, None),
        (SelectCharLeft, 7, Some(1)),
        (SelectCharRight, 9, Some(1)),
        (SelectWordLeft, 6, Some(1)),
        (SelectWordRight, 10, Some(1)),
        (SelectLine, 14, Some(2)),
        (SelectAll, 19, Some(0)),
    ] {
        let mut app = draft(text, 8, Some(1));
        shortcut(&mut app, action);
        assert_eq!(app.composer.prompt_buffer, text, "{action:?}");
        assert_eq!(app.composer.prompt_cursor, cursor, "{action:?}");
        assert_eq!(app.composer.selection_anchor, anchor, "{action:?}");
    }
    let mut app = draft("界e\u{301}👩‍💻", 6, None);
    for cursor in [3, 1, 0, 0] {
        shortcut(&mut app, SelectCharLeft);
        assert_eq!(app.composer.prompt_cursor, cursor);
        assert_eq!(app.composer.selection_anchor, Some(6));
    }
    for cursor in [1, 3, 6, 6] {
        shortcut(&mut app, SelectCharRight);
        assert_eq!(app.composer.prompt_cursor, cursor);
        assert_eq!(app.composer.selection_anchor, Some(6));
    }
    for (text, cursor, action, expected) in [
        ("", 0, MoveWordLeft, 0),
        ("hello", 0, MoveWordLeft, 0),
        ("hello", 5, MoveWordRight, 5),
        ("  hello", 7, MoveWordLeft, 2),
    ] {
        let mut app = draft(text, cursor, Some(0));
        shortcut(&mut app, action);
        assert_eq!(app.composer.prompt_cursor, expected);
        assert_eq!(app.composer.selection_anchor, None);
    }
}

#[test]
fn prompt_deletion_preserves_undo_selection_and_history() {
    use harness_tui::Action::*;
    let text = "界\none.two tail\nlast";
    for (action, expected, cursor) in [
        (DeleteWordBackward, "界\none.o tail\nlast", 6),
        (DeleteWordForward, "界\none.twtail\nlast", 8),
        (DeleteLine, "界\nlast", 2),
        (KillToLineStart, "界\no tail\nlast", 2),
        (KillToLineEnd, "界\none.tw\nlast", 8),
    ] {
        let mut app = draft(text, 8, None);
        shortcut(&mut app, action);
        assert_eq!(app.composer.prompt_buffer, expected, "{action:?}");
        assert_eq!(app.composer.prompt_cursor, cursor, "{action:?}");
        shortcut(&mut app, Undo);
        assert_eq!(app.composer.prompt_buffer, text, "{action:?}");
        assert_eq!(app.composer.prompt_cursor, 8, "{action:?}");
        shortcut(&mut app, Redo);
        assert_eq!(app.composer.prompt_buffer, expected, "{action:?}");
        assert_eq!(app.composer.prompt_cursor, cursor, "{action:?}");
    }
    let mut app = draft("hello world", 5, Some(0));
    app.handle_key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE));
    assert_eq!(app.composer.prompt_buffer, "X world");
    shortcut(&mut app, Undo);
    assert_eq!(app.composer.prompt_buffer, "hello world");
    assert_eq!(app.composer.prompt_cursor, 5);
    assert_eq!(app.composer.selection_anchor, Some(0));
    shortcut(&mut app, Backspace);
    assert_eq!(app.composer.prompt_buffer, " world");
    assert_eq!(app.composer.prompt_cursor, 0);
    assert_eq!(app.composer.selection_anchor, None);
    shortcut(&mut app, Redo);
    assert_eq!(app.composer.prompt_buffer, " world");

    let mut app = draft("draft text", 0, None);
    app.composer.prompt_history.push("old prompt".into());
    shortcut(&mut app, HistoryUp);
    assert_eq!(app.composer.prompt_buffer, "old prompt");
    shortcut(&mut app, Undo);
    assert_eq!(app.composer.prompt_buffer, "draft text");
    assert_eq!(app.composer.prompt_cursor, 0);
}
