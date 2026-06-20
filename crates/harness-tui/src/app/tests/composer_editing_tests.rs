use super::*;

pub(super) fn move_word_left_skips_separators_then_word() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello world".to_string();
    app.composer.prompt_cursor = 11;

    app.execute_action(Action::MoveWordLeft);

    assert_eq!(app.composer.prompt_cursor, 6);
    assert_eq!(app.composer.selection_anchor, None);
}

pub(super) fn move_word_right_skips_word_then_separators() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello world".to_string();
    app.composer.prompt_cursor = 0;

    app.execute_action(Action::MoveWordRight);

    assert_eq!(app.composer.prompt_cursor, 6);
    assert_eq!(app.composer.selection_anchor, None);
}

pub(super) fn move_word_left_at_start_stays_at_zero() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello".to_string();
    app.composer.prompt_cursor = 0;

    app.execute_action(Action::MoveWordLeft);

    assert_eq!(app.composer.prompt_cursor, 0);
}

pub(super) fn move_word_right_at_end_stays_at_end() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello".to_string();
    app.composer.prompt_cursor = 5;

    app.execute_action(Action::MoveWordRight);

    assert_eq!(app.composer.prompt_cursor, 5);
}

pub(super) fn move_word_left_handles_leading_separators() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "  hello".to_string();
    app.composer.prompt_cursor = 7;

    app.execute_action(Action::MoveWordLeft);

    assert_eq!(app.composer.prompt_cursor, 2);
}

pub(super) fn delete_word_backward_removes_word_and_pushes_undo() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello world".to_string();
    app.composer.prompt_cursor = 11;

    app.execute_action(Action::DeleteWordBackward);

    assert_eq!(app.composer.prompt_buffer, "hello ");
    assert_eq!(app.composer.prompt_cursor, 6);
    assert_eq!(app.composer.undo_stack.len(), 1);

    app.execute_action(Action::Undo);

    assert_eq!(app.composer.prompt_buffer, "hello world");
    assert_eq!(app.composer.prompt_cursor, 11);
    assert_eq!(app.composer.selection_anchor, None);
}

pub(super) fn delete_word_forward_removes_word_and_pushes_undo() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello world".to_string();
    app.composer.prompt_cursor = 0;

    app.execute_action(Action::DeleteWordForward);

    assert_eq!(app.composer.prompt_buffer, "world");
    assert_eq!(app.composer.prompt_cursor, 0);

    app.execute_action(Action::Undo);

    assert_eq!(app.composer.prompt_buffer, "hello world");
    assert_eq!(app.composer.prompt_cursor, 0);
}

pub(super) fn redo_re_applies_after_undo() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello world".to_string();
    app.composer.prompt_cursor = 11;

    app.execute_action(Action::DeleteWordBackward);
    assert_eq!(app.composer.prompt_buffer, "hello ");

    app.execute_action(Action::Undo);
    assert_eq!(app.composer.prompt_buffer, "hello world");

    app.execute_action(Action::Redo);
    assert_eq!(app.composer.prompt_buffer, "hello ");
    assert_eq!(app.composer.prompt_cursor, 6);
}

pub(super) fn undo_restores_selection_anchor() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello world".to_string();
    app.composer.prompt_cursor = 5;
    app.composer.selection_anchor = Some(0);

    app.execute_action(Action::Char('X'));

    assert_eq!(app.composer.prompt_buffer, "X world");
    assert_eq!(app.composer.selection_anchor, None);

    app.execute_action(Action::Undo);

    assert_eq!(app.composer.prompt_buffer, "hello world");
    assert_eq!(app.composer.prompt_cursor, 5);
    assert_eq!(app.composer.selection_anchor, Some(0));
}

pub(super) fn select_char_left_extends_selection() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello".to_string();
    app.composer.prompt_cursor = 5;

    app.execute_action(Action::SelectCharLeft);

    assert_eq!(app.composer.prompt_cursor, 4);
    assert_eq!(app.composer.selection_anchor, Some(5));

    app.execute_action(Action::SelectCharLeft);

    assert_eq!(app.composer.prompt_cursor, 3);
    assert_eq!(app.composer.selection_anchor, Some(5));
}

pub(super) fn select_word_right_extends_selection() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello world".to_string();
    app.composer.prompt_cursor = 0;

    app.execute_action(Action::SelectWordRight);

    assert_eq!(app.composer.prompt_cursor, 6);
    assert_eq!(app.composer.selection_anchor, Some(0));
}

pub(super) fn select_all_selects_entire_buffer() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello world".to_string();
    app.composer.prompt_cursor = 0;

    app.execute_action(Action::SelectAll);

    assert_eq!(app.composer.selection_anchor, Some(0));
    assert_eq!(app.composer.prompt_cursor, 11);
}

pub(super) fn select_line_selects_current_line() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello\nworld\nfoo".to_string();
    app.composer.prompt_cursor = 7;

    app.execute_action(Action::SelectLine);

    assert_eq!(app.composer.selection_anchor, Some(6));
    assert_eq!(app.composer.prompt_cursor, 11);
}

pub(super) fn move_line_start_clears_selection() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello\nworld".to_string();
    app.composer.prompt_cursor = 10;
    app.composer.selection_anchor = Some(6);

    app.execute_action(Action::MoveLineStart);

    assert_eq!(app.composer.prompt_cursor, 6);
    assert_eq!(app.composer.selection_anchor, None);
}

pub(super) fn move_line_end_clears_selection() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello\nworld".to_string();
    app.composer.prompt_cursor = 6;
    app.composer.selection_anchor = Some(0);

    app.execute_action(Action::MoveLineEnd);

    assert_eq!(app.composer.prompt_cursor, 11);
    assert_eq!(app.composer.selection_anchor, None);
}

pub(super) fn move_buffer_start_clears_selection() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello\nworld".to_string();
    app.composer.prompt_cursor = 8;
    app.composer.selection_anchor = Some(0);

    app.execute_action(Action::MoveBufferStart);

    assert_eq!(app.composer.prompt_cursor, 0);
    assert_eq!(app.composer.selection_anchor, None);
}

pub(super) fn move_buffer_end_clears_selection() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello\nworld".to_string();
    app.composer.prompt_cursor = 0;
    app.composer.selection_anchor = Some(5);

    app.execute_action(Action::MoveBufferEnd);

    assert_eq!(app.composer.prompt_cursor, 11);
    assert_eq!(app.composer.selection_anchor, None);
}

pub(super) fn delete_line_removes_entire_line_including_newline() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello\nworld\nfoo".to_string();
    app.composer.prompt_cursor = 8;

    app.execute_action(Action::DeleteLine);

    assert_eq!(app.composer.prompt_buffer, "hello\nfoo");
    assert_eq!(app.composer.prompt_cursor, 6);

    app.execute_action(Action::Undo);

    assert_eq!(app.composer.prompt_buffer, "hello\nworld\nfoo");
    assert_eq!(app.composer.prompt_cursor, 8);
}

pub(super) fn kill_to_line_start_deletes_from_cursor_to_line_start() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello world".to_string();
    app.composer.prompt_cursor = 8;

    app.execute_action(Action::KillToLineStart);

    assert_eq!(app.composer.prompt_buffer, "rld");
    assert_eq!(app.composer.prompt_cursor, 0);
}

pub(super) fn kill_to_line_end_deletes_from_cursor_to_line_end() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello world".to_string();
    app.composer.prompt_cursor = 5;

    app.execute_action(Action::KillToLineEnd);

    assert_eq!(app.composer.prompt_buffer, "hello");
    assert_eq!(app.composer.prompt_cursor, 5);
}

pub(super) fn typing_after_select_replaces_selection() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello world".to_string();
    app.composer.prompt_cursor = 11;
    app.composer.selection_anchor = Some(6);

    app.execute_action(Action::Char('X'));

    assert_eq!(app.composer.prompt_buffer, "hello X");
    assert_eq!(app.composer.prompt_cursor, 7);
    assert_eq!(app.composer.selection_anchor, None);
}

pub(super) fn backspace_with_selection_deletes_selection() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello world".to_string();
    app.composer.prompt_cursor = 11;
    app.composer.selection_anchor = Some(6);

    app.execute_action(Action::Backspace);

    assert_eq!(app.composer.prompt_buffer, "hello ");
    assert_eq!(app.composer.prompt_cursor, 6);
    assert_eq!(app.composer.selection_anchor, None);
}

pub(super) fn undo_stack_caps_at_max_entries() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;

    for _ in 0..150 {
        app.execute_action(Action::Char('a'));
    }

    assert!(app.composer.undo_stack.len() <= 100);
}

pub(super) fn history_navigation_preserves_draft_via_undo() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_history = vec!["old prompt".to_string()];
    app.composer.prompt_buffer = "draft text".to_string();
    app.composer.prompt_cursor = 0;

    app.execute_action(Action::HistoryUp);
    assert_eq!(app.composer.prompt_buffer, "old prompt");

    app.execute_action(Action::Undo);
    assert_eq!(app.composer.prompt_buffer, "draft text");
    assert_eq!(app.composer.prompt_cursor, 0);
}

pub(super) fn cursor_left_clears_selection() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello".to_string();
    app.composer.prompt_cursor = 3;
    app.composer.selection_anchor = Some(0);

    app.execute_action(Action::CursorLeft);

    assert_eq!(app.composer.prompt_cursor, 2);
    assert_eq!(app.composer.selection_anchor, None);
}

pub(super) fn word_boundary_detects_punctuation_as_separator() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "foo.bar baz".to_string();
    app.composer.prompt_cursor = 11;

    app.execute_action(Action::MoveWordLeft);

    assert_eq!(app.composer.prompt_cursor, 8);
}
