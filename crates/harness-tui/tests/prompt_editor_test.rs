//! Todo 13: Prompt editor, history, paste, shell mode, and completions.
//!
//! Differential-TDD contract shard for the clean-room parity program. These
//! tests exercise the PUBLIC `harness_tui::app` surface only — they drive the
//! real `AppState::handle_key` keymap dispatch for keyboard contracts and a
//! small public completion API for the overlay state machine. No snapshot-only
//! acceptance: every test asserts a real state transition or emitted intent.
//!
//! Reference contract rows (action / action_def) proven here:
//! - SendPrompt        (Enter, PromptFocused)            -> submit / shell run
//! - BashMode          (`!` on empty prompt)             -> enter shell mode
//! - InsertNewline     (Shift+Enter / Ctrl+J / Alt+Enter) -> newline, no submit
//! - HistoryUp/Down    (Up/Down at buffer edge)          -> recall, draft keep
//! - Cursor/Select/Move/Delete/Kill/Undo/Redo vocabulary -> composer editing
//! - bracketed paste payload handling                    -> unit tests in prompt_input.rs
//! - completion overlay (open/cycle/accept/dismiss)      -> unit tests in composer_editing.rs
//!
//! The paste (D) and completion-overlay (E) contract sections were relocated
//! into owning-module unit tests (`prompt_input::paste_tests`,
//! `composer_editing::completion_tests`) because their entry points are
//! `pub(crate)` and cannot be driven from an integration-test crate without
//! widening the public API.  All assertions are preserved verbatim for Todo 13.
//!
//! Proof dimensions: P1 (contract), P2 (owner), P3 (terminal), P6 (rejection).

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "parity contract tests use fail-fast asserts for missing state"
)]

use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::app::{AppState, Focus, UiIntent};

// ---------------------------------------------------------------------------
// Test helpers (public surface only)
// ---------------------------------------------------------------------------

/// A live app. `AppState::new_live` already focuses the prompt and leaves the
/// composer enabled with no events, matching the internal prompt tests.
fn live_app() -> AppState {
    AppState::new_live(None, false, None)
}

/// A live app wired to capture emitted `UiIntent`s.
fn live_app_capture() -> (AppState, Arc<Mutex<Vec<UiIntent>>>) {
    let intents = Arc::new(Mutex::new(Vec::<UiIntent>::new()));
    let sink = {
        let intents = Arc::clone(&intents);
        Arc::new(move |intent: UiIntent| intents.lock().unwrap().push(intent))
    };
    (AppState::new_live(None, false, Some(sink)), intents)
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn keym(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, mods)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// Type a string char-by-char through the real keymap dispatch.
fn type_str(app: &mut AppState, text: &str) {
    for ch in text.chars() {
        app.handle_key(key(KeyCode::Char(ch)));
    }
}

/// Set the buffer directly with the cursor at the end (no undo history pushed).
fn set_buffer(app: &mut AppState, text: &str) {
    app.composer.prompt_buffer = text.to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    app.composer.selection_anchor = None;
}

/// Set the buffer with an explicit cursor (no undo history pushed).
fn set_buffer_at(app: &mut AppState, text: &str, cursor: usize) {
    app.composer.prompt_buffer = text.to_string();
    app.composer.prompt_cursor = cursor;
    app.composer.selection_anchor = None;
}

// ===========================================================================
// A. Editor text buffer, cursor, selection, undo/redo (via real keymap)
// ===========================================================================

/// A1: typing inserts at the cursor and advances it.
#[test]
fn editor_typing_inserts_and_advances_cursor() {
    // arrange
    // Given: a focused live app with an empty composer
    let mut app = live_app();
    assert!(app.composer.prompt_buffer.is_empty());

    // When: two characters are typed
    app.handle_key(key(KeyCode::Char('h')));
    app.handle_key(key(KeyCode::Char('i')));

    // act
    // Then: both land in the buffer with the cursor after them
    // assert
    assert_eq!(app.composer.prompt_buffer, "hi");
    assert_eq!(app.composer.prompt_cursor, 2);
}

/// A2: plain cursor movement clamps at both ends and clears any selection.
#[test]
fn editor_cursor_movement_clamps_at_bounds() {
    // arrange
    // Given: "abc" with the cursor at the end
    let mut app = live_app();
    set_buffer(&mut app, "abc");

    // When/Then: stepping left walks to zero and clamps
    app.handle_key(key(KeyCode::Left));
    assert_eq!(app.composer.prompt_cursor, 2);
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Left));
    assert_eq!(app.composer.prompt_cursor, 0);
    app.handle_key(key(KeyCode::Left));
    assert_eq!(app.composer.prompt_cursor, 0, "cursor must clamp at start");

    // When/Then: stepping right walks to the end and clamps
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.composer.prompt_cursor, 3);
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.composer.prompt_cursor, 3, "cursor must clamp at end");

    // act
    // And: plain movement never leaves a selection behind
    // assert
    assert_eq!(app.composer.selection_anchor, None);
}

/// A3: backspace deletes backward, delete deletes forward.
#[test]
fn editor_backspace_and_delete_remove_chars() {
    // arrange
    // Given: "abc" cursor at end
    let mut app = live_app();
    set_buffer(&mut app, "abc");

    // When: backspace twice
    app.handle_key(key(KeyCode::Backspace));
    app.handle_key(key(KeyCode::Backspace));
    // Then
    assert_eq!(app.composer.prompt_buffer, "a");
    assert_eq!(app.composer.prompt_cursor, 1);

    // act
    // Given: "abc" cursor at start
    set_buffer_at(&mut app, "abc", 0);
    // When: forward-delete once
    app.handle_key(key(KeyCode::Delete));
    // Then
    // assert
    assert_eq!(app.composer.prompt_buffer, "bc");
    assert_eq!(app.composer.prompt_cursor, 0);
}

/// A4: Ctrl+Left / Ctrl+Right jump by word.
#[test]
fn editor_word_navigation_jumps_by_word() {
    // arrange
    // Given: "foo bar baz" cursor at end
    let mut app = live_app();
    set_buffer(&mut app, "foo bar baz");

    // When: Ctrl+Left twice (baz -> bar)
    app.handle_key(ctrl_key_left());
    assert_eq!(app.composer.prompt_cursor, 8, "lands at start of 'baz'");
    app.handle_key(ctrl_key_left());
    assert_eq!(app.composer.prompt_cursor, 4, "lands at start of 'bar'");

    // act
    // When: from column 0 Ctrl+Right spans the first word + separator
    set_buffer_at(&mut app, "foo bar baz", 0);
    app.handle_key(ctrl_key_right());
    // assert
    assert_eq!(app.composer.prompt_cursor, 4, "lands just after 'foo '");
}

fn ctrl_key_left() -> KeyEvent {
    keym(KeyCode::Left, KeyModifiers::CONTROL)
}

fn ctrl_key_right() -> KeyEvent {
    keym(KeyCode::Right, KeyModifiers::CONTROL)
}

/// A5: Home / End move to the current line edges.
#[test]
fn editor_home_and_end_move_to_line_edges() {
    // arrange
    // Given: "hello" cursor in the middle
    let mut app = live_app();
    set_buffer_at(&mut app, "hello", 3);

    // act
    // When/Then
    app.handle_key(key(KeyCode::Home));
    // assert
    assert_eq!(app.composer.prompt_cursor, 0);
    app.handle_key(key(KeyCode::End));
    assert_eq!(app.composer.prompt_cursor, 5);
}

/// A6: Up/Down move the caret vertically across lines, preserving column, and
/// stop at the buffer edges without spilling into history while the caret is
/// away from the buffer boundary.
#[test]
fn editor_vertical_cursor_moves_across_lines() {
    // arrange
    // Given: a three-line buffer with the caret on the last line
    let mut app = live_app();
    set_buffer_at(&mut app, "ab\ncd\nef", 7);

    // When/Then: Up walks to the same column on the line above
    app.handle_key(key(KeyCode::Up));
    assert_eq!(
        app.composer.prompt_cursor, 4,
        "line 2 -> line 1, column kept"
    );
    app.handle_key(key(KeyCode::Up));
    assert_eq!(
        app.composer.prompt_cursor, 1,
        "line 1 -> line 0, column kept"
    );

    // When: Up on the top line cannot move further
    app.handle_key(key(KeyCode::Up));
    // Then: caret is unchanged and, not being at the buffer start, history is
    // left untouched
    assert_eq!(app.composer.prompt_cursor, 1, "clamps at top line");
    assert_eq!(app.composer.prompt_history_index, None);

    // When/Then: Down walks back down the lines
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.composer.prompt_cursor, 4);
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.composer.prompt_cursor, 7);

    // act
    // When: Down on the bottom line cannot move further
    app.handle_key(key(KeyCode::Down));
    // Then: caret unchanged and, not at the buffer end, no history recall
    // assert
    assert_eq!(app.composer.prompt_cursor, 7, "clamps at bottom line");
    assert_eq!(app.composer.prompt_history_index, None);
}

/// A7: Shift+arrows build a selection; SelectAll spans the buffer; plain
/// movement clears the selection.
#[test]
fn editor_selection_anchor_and_select_all() {
    // arrange
    // Given: "hello" cursor at end
    let mut app = live_app();
    set_buffer(&mut app, "hello");

    // When: Shift+Left twice
    app.handle_key(keym(KeyCode::Left, KeyModifiers::SHIFT));
    assert_eq!(
        app.composer.selection_anchor,
        Some(5),
        "anchor planted at origin"
    );
    assert_eq!(app.composer.prompt_cursor, 4);
    app.handle_key(keym(KeyCode::Left, KeyModifiers::SHIFT));
    assert_eq!(app.composer.selection_anchor, Some(5), "anchor is sticky");
    assert_eq!(app.composer.prompt_cursor, 3);

    // When: SelectAll
    app.handle_key(ctrl('a'));
    assert_eq!(app.composer.selection_anchor, Some(0));
    assert_eq!(app.composer.prompt_cursor, 5);

    // act
    // When: plain Left clears the selection
    app.handle_key(key(KeyCode::Left));
    // assert
    assert_eq!(app.composer.selection_anchor, None);
}

/// A8: Ctrl+W deletes the word behind the cursor.
#[test]
fn editor_delete_word_backward_removes_word() {
    // arrange
    // Given: "foo bar" cursor at end
    let mut app = live_app();
    set_buffer(&mut app, "foo bar");

    // When
    app.handle_key(ctrl('w'));

    // act
    // Then: the trailing word is gone, the separator stays
    // assert
    assert_eq!(app.composer.prompt_buffer, "foo ");
    assert_eq!(app.composer.prompt_cursor, 4);
}

/// A9: Ctrl+U kills from the cursor to line start.
#[test]
fn editor_kill_to_line_start() {
    // arrange
    // Given: "hello world" cursor after "hello"
    let mut app = live_app();
    set_buffer_at(&mut app, "hello world", 5);

    // When
    app.handle_key(ctrl('u'));

    // act
    // Then
    // assert
    assert_eq!(app.composer.prompt_buffer, " world");
    assert_eq!(app.composer.prompt_cursor, 0);
}

/// A10: Ctrl+K kills from the cursor to line end.
#[test]
fn editor_kill_to_line_end() {
    // arrange
    // Given: "hello world" cursor after "hello"
    let mut app = live_app();
    set_buffer_at(&mut app, "hello world", 5);

    // When
    app.handle_key(ctrl('k'));

    // act
    // Then
    // assert
    assert_eq!(app.composer.prompt_buffer, "hello");
    assert_eq!(app.composer.prompt_cursor, 5);
}

/// A11: typing builds undo history; Ctrl+Z undoes, Ctrl+Shift+Z redoes.
#[test]
fn editor_undo_and_redo_round_trip() {
    // arrange
    // Given: three typed characters (each insert pushes an undo snapshot)
    let mut app = live_app();
    type_str(&mut app, "abc");
    assert_eq!(app.composer.prompt_buffer, "abc");

    // When/Then: two undos walk the buffer back
    app.handle_key(ctrl('z'));
    assert_eq!(app.composer.prompt_buffer, "ab");
    app.handle_key(ctrl('z'));
    assert_eq!(app.composer.prompt_buffer, "a");

    // act
    // When/Then: redo restores one step
    app.handle_key(keym(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    // assert
    assert_eq!(app.composer.prompt_buffer, "ab");
}

/// A12: InsertNewline (Ctrl+J) inserts a newline WITHOUT submitting.
#[test]
fn editor_insert_newline_does_not_submit() {
    // arrange
    // Given: "ab" typed in a live app
    let (mut app, intents) = live_app_capture();
    type_str(&mut app, "ab");

    // When: Ctrl+J newline, then "cd"
    app.handle_key(ctrl('j'));
    type_str(&mut app, "cd");

    // act
    // Then: the buffer spans two lines, nothing was sent, no history recorded
    // assert
    assert_eq!(app.composer.prompt_buffer, "ab\ncd");
    assert_eq!(app.composer.prompt_cursor, 5);
    assert!(intents.lock().unwrap().is_empty());
    assert!(app.composer.prompt_history.is_empty());
}

// ===========================================================================
// B. Prompt history: up/down navigation + draft preservation
// ===========================================================================

/// B1-B5: Up recalls newest-first (saturating), Down walks forward and finally
/// restores the preserved draft past the newest entry. History recall requires
/// the caret at the buffer start; a recall lands the caret at the END, so the
/// caret is moved Home before each further upward step.
#[test]
fn history_up_down_navigates_and_restores_draft() {
    // arrange
    // Given: two history entries and an empty draft (cursor at start)
    let mut app = live_app();
    app.composer.prompt_history = vec!["first".to_string(), "second".to_string()];
    set_buffer_at(&mut app, "", 0);

    // When: Up recalls the newest entry
    app.handle_key(key(KeyCode::Up));
    // Then
    assert_eq!(app.composer.prompt_buffer, "second");
    assert_eq!(app.composer.prompt_history_index, Some(1));

    // Given: move the caret to the start so history recall can fire again
    app.handle_key(key(KeyCode::Home));
    assert_eq!(app.composer.prompt_cursor, 0);

    // When: Up again recalls the older entry
    app.handle_key(key(KeyCode::Up));
    // Then
    assert_eq!(app.composer.prompt_buffer, "first");
    assert_eq!(app.composer.prompt_history_index, Some(0));

    // Given: reposition to the start once more
    app.handle_key(key(KeyCode::Home));

    // When: Up at the oldest entry saturates
    app.handle_key(key(KeyCode::Up));
    // Then
    assert_eq!(app.composer.prompt_buffer, "first");
    assert_eq!(app.composer.prompt_history_index, Some(0));

    // When: Down walks back to the newer entry (caret already at the end)
    app.handle_key(key(KeyCode::Down));
    // Then
    assert_eq!(app.composer.prompt_buffer, "second");
    assert_eq!(app.composer.prompt_history_index, Some(1));

    // act
    // When: Down past the newest entry restores the (empty) draft
    app.handle_key(key(KeyCode::Down));
    // Then
    // assert
    assert_eq!(app.composer.prompt_buffer, "");
    assert_eq!(app.composer.prompt_history_index, None);
}

/// B6: a non-empty in-progress draft is preserved across history navigation.
#[test]
fn history_preserves_nonempty_draft() {
    // arrange
    // Given: history plus a draft "mydraft" with the cursor at the start
    let mut app = live_app();
    app.composer.prompt_history = vec!["first".to_string(), "second".to_string()];
    set_buffer_at(&mut app, "mydraft", 0);

    // When: Up recalls history (draft is stashed first)
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.composer.prompt_buffer, "second");
    assert_eq!(app.composer.prompt_history_index, Some(1));

    // act
    // When: Down past the end restores the original draft verbatim
    app.handle_key(key(KeyCode::Down));
    // Then
    // assert
    assert_eq!(app.composer.prompt_buffer, "mydraft");
    assert_eq!(app.composer.prompt_cursor, 0);
    assert_eq!(app.composer.prompt_history_index, None);
}

/// B-Reject: with empty history, Up/Down leave the draft untouched.
#[test]
fn history_navigation_is_noop_when_history_empty() {
    // arrange
    // Given: no history, a draft present, cursor at start
    let mut app = live_app();
    set_buffer_at(&mut app, "draft", 0);

    // When
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Down));

    // act
    // Then
    // assert
    assert_eq!(app.composer.prompt_buffer, "draft");
    assert_eq!(app.composer.prompt_history_index, None);
}

// ===========================================================================
// C. Shell mode toggle (BashMode reference action)
// ===========================================================================

/// C1: typing `!` on an empty prompt enters shell mode and does NOT insert it.
#[test]
fn shell_mode_enters_on_bang_when_prompt_empty() {
    // arrange
    // Given: an empty focused prompt
    let mut app = live_app();
    assert!(!app.composer.shell_mode);

    // When
    app.handle_key(key(KeyCode::Char('!')));

    // act
    // Then
    // assert
    assert!(
        app.composer.shell_mode,
        "! on empty prompt enters shell mode"
    );
    assert!(
        app.composer.prompt_buffer.is_empty(),
        "! is consumed, not inserted"
    );
}

/// C-Reject: `!` with existing text is inserted literally and stays in normal
/// mode (P6 rejection).
#[test]
fn shell_mode_does_not_enter_when_prompt_has_text() {
    // arrange
    // Given: a non-empty prompt
    let mut app = live_app();
    set_buffer(&mut app, "hello");

    // When
    app.handle_key(key(KeyCode::Char('!')));

    // act
    // Then
    // assert
    assert!(!app.composer.shell_mode);
    assert_eq!(app.composer.prompt_buffer, "hello!");
}

/// C2: Enter in shell mode emits `RunShellCommand`, clears the buffer, and
/// exits shell mode (SendPrompt routed to the shell path).
#[test]
fn shell_mode_enter_emits_run_shell_command_and_clears() {
    // arrange
    // Given: shell mode with a typed command
    let (mut app, intents) = live_app_capture();
    app.composer.shell_mode = true;
    type_str(&mut app, "ls -la");

    // When
    app.handle_key(key(KeyCode::Enter));

    // act
    // Then: a single RunShellCommand intent with the trimmed command
    let captured = intents.lock().unwrap();
    // assert
    assert_eq!(captured.len(), 1);
    match &captured[0] {
        UiIntent::RunShellCommand { command } => assert_eq!(command, "ls -la"),
        other => panic!("expected RunShellCommand, got {other:?}"),
    }
    assert!(app.composer.prompt_buffer.is_empty());
    assert!(!app.composer.shell_mode);
}

/// C3: Esc exits shell mode.
#[test]
fn shell_mode_exits_on_escape() {
    // arrange
    // Given
    let mut app = live_app();
    app.composer.shell_mode = true;

    // When
    app.handle_key(key(KeyCode::Esc));

    // act
    // Then
    // assert
    assert!(!app.composer.shell_mode);
}

/// C4: Backspace on an empty shell prompt exits shell mode.
#[test]
fn shell_mode_exits_on_backspace_at_empty_prompt() {
    // arrange
    // Given
    let mut app = live_app();
    app.composer.shell_mode = true;

    // When
    app.handle_key(key(KeyCode::Backspace));

    // act
    // Then
    // assert
    assert!(!app.composer.shell_mode);
}

// ===========================================================================
// D. Paste handling (bracketed-paste payload contract)
// ===========================================================================
// Paste contract tests (D1–D3, D-Reject) have been relocated to the owning
// module's unit tests in `prompt_input::paste_tests` because `handle_paste` is
// `pub(crate)` and cannot be called from an integration-test crate without
// widening the public API.  All assertions are preserved verbatim for Todo 13.

// ===========================================================================
// E. Completion overlay: open / cycle / accept / dismiss
// ===========================================================================
// Completion overlay tests (E1–E7, E-Reject) have been relocated to the owning
// module's unit tests in `composer_editing::completion_tests` because the
// completion API is `pub(crate)` (not yet promoted to the public surface —
// Todo 13 will own that promotion with proper state fields routed through
// Todo 6).  All assertions are preserved verbatim for Todo 13.
