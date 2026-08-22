use super::*;

fn collect_chars(text: &str) -> Vec<char> {
    text.chars().collect()
}

fn find_word_start_left(chars: &[char], cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }
    let mut pos = cursor;
    while pos > 0 && !chars[pos - 1].is_alphanumeric() {
        pos -= 1;
    }
    while pos > 0 && chars[pos - 1].is_alphanumeric() {
        pos -= 1;
    }
    pos
}

fn find_word_end_right(chars: &[char], cursor: usize) -> usize {
    let len = chars.len();
    if cursor >= len {
        return len;
    }
    let mut pos = cursor;
    while pos < len && chars[pos].is_alphanumeric() {
        pos += 1;
    }
    while pos < len && !chars[pos].is_alphanumeric() {
        pos += 1;
    }
    pos
}

impl AppState {
    fn composer_chars(&self) -> Vec<char> {
        collect_chars(&self.composer.prompt_buffer)
    }

    fn composer_word_start_left(&self) -> usize {
        let chars = self.composer_chars();
        find_word_start_left(&chars, self.composer.prompt_cursor)
    }

    fn composer_word_end_right(&self) -> usize {
        let chars = self.composer_chars();
        find_word_end_right(&chars, self.composer.prompt_cursor)
    }

    fn composer_line_start(&self) -> usize {
        let chars = self.composer_chars();
        let mut pos = self.composer.prompt_cursor;
        while pos > 0 && chars[pos - 1] != '\n' {
            pos -= 1;
        }
        pos
    }

    fn composer_line_end(&self) -> usize {
        let chars = self.composer_chars();
        let mut pos = self.composer.prompt_cursor;
        while pos < chars.len() && chars[pos] != '\n' {
            pos += 1;
        }
        pos
    }

    pub(in crate::app) fn delete_prompt_range(&mut self, start: usize, end: usize) {
        let count = self.prompt_char_count();
        let start = start.min(count);
        let end = end.min(count);
        if start >= end {
            return;
        }
        self.continued_live_reopen_surface_active = false;
        self.adjust_file_mention_tags_for_delete(start, end);
        let start_byte = self.prompt_char_byte_index(start);
        let end_byte = self.prompt_char_byte_index(end);
        self.composer
            .prompt_buffer
            .replace_range(start_byte..end_byte, "");
        let removed = end - start;
        if self.composer.prompt_cursor > start {
            if self.composer.prompt_cursor <= end {
                self.composer.prompt_cursor = start;
            } else {
                self.composer.prompt_cursor -= removed;
            }
        }
        self.composer.selection_anchor = None;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn composer_select_char_left(&mut self) {
        if self.composer.prompt_cursor == 0 {
            return;
        }
        if self.composer.selection_anchor.is_none() {
            self.composer.selection_anchor = Some(self.composer.prompt_cursor);
        }
        self.composer.prompt_cursor -= 1;
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn composer_select_char_right(&mut self) {
        if self.composer.prompt_cursor >= self.prompt_char_count() {
            return;
        }
        if self.composer.selection_anchor.is_none() {
            self.composer.selection_anchor = Some(self.composer.prompt_cursor);
        }
        self.composer.prompt_cursor += 1;
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn composer_select_word_left(&mut self) {
        let new_pos = self.composer_word_start_left();
        if self.composer.selection_anchor.is_none() {
            self.composer.selection_anchor = Some(self.composer.prompt_cursor);
        }
        self.composer.prompt_cursor = new_pos;
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn composer_select_word_right(&mut self) {
        let new_pos = self.composer_word_end_right();
        if self.composer.selection_anchor.is_none() {
            self.composer.selection_anchor = Some(self.composer.prompt_cursor);
        }
        self.composer.prompt_cursor = new_pos;
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn composer_select_line(&mut self) {
        let line_start = self.composer_line_start();
        let line_end = self.composer_line_end();
        self.composer.selection_anchor = Some(line_start);
        self.composer.prompt_cursor = line_end;
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn composer_select_all(&mut self) {
        self.composer.selection_anchor = Some(0);
        self.composer.prompt_cursor = self.prompt_char_count();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn composer_move_word_left(&mut self) {
        self.composer.prompt_cursor = self.composer_word_start_left();
        self.composer.selection_anchor = None;
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn composer_move_word_right(&mut self) {
        self.composer.prompt_cursor = self.composer_word_end_right();
        self.composer.selection_anchor = None;
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn composer_move_line_start(&mut self) {
        self.composer.prompt_cursor = self.composer_line_start();
        self.composer.selection_anchor = None;
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn composer_move_line_end(&mut self) {
        self.composer.prompt_cursor = self.composer_line_end();
        self.composer.selection_anchor = None;
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn composer_move_buffer_start(&mut self) {
        self.composer.prompt_cursor = 0;
        self.composer.selection_anchor = None;
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn composer_move_buffer_end(&mut self) {
        self.composer.prompt_cursor = self.prompt_char_count();
        self.composer.selection_anchor = None;
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn composer_delete_word_forward(&mut self) {
        let end = self.composer_word_end_right();
        if end <= self.composer.prompt_cursor {
            return;
        }
        self.composer.push_undo();
        self.delete_prompt_range(self.composer.prompt_cursor, end);
    }

    pub(in crate::app) fn composer_delete_word_backward(&mut self) {
        let start = self.composer_word_start_left();
        if start >= self.composer.prompt_cursor {
            return;
        }
        self.composer.push_undo();
        self.delete_prompt_range(start, self.composer.prompt_cursor);
    }

    pub(in crate::app) fn composer_delete_line(&mut self) {
        let line_start = self.composer_line_start();
        let chars = self.composer_chars();
        let mut line_end = self.composer_line_end();
        if line_end < chars.len() && chars[line_end] == '\n' {
            line_end += 1;
        }
        if line_start >= line_end {
            return;
        }
        self.composer.push_undo();
        self.delete_prompt_range(line_start, line_end);
    }

    pub(in crate::app) fn composer_kill_to_line_start(&mut self) {
        let line_start = self.composer_line_start();
        if line_start >= self.composer.prompt_cursor {
            return;
        }
        self.composer.push_undo();
        self.delete_prompt_range(line_start, self.composer.prompt_cursor);
    }

    pub(in crate::app) fn composer_kill_to_line_end(&mut self) {
        let line_end = self.composer_line_end();
        if self.composer.prompt_cursor >= line_end {
            return;
        }
        self.composer.push_undo();
        self.delete_prompt_range(self.composer.prompt_cursor, line_end);
    }

    pub(in crate::app) fn composer_undo(&mut self) {
        if self.composer.editor_matches_prompt_fields()
            && matches!(self.composer.editor_undo(), Ok(true))
        {
            self.sync_slash_overlay();
            self.sync_file_mention_overlay();
            return;
        }
        let Some(snapshot) = self.composer.undo_stack.pop() else {
            return;
        };
        self.composer.redo_stack.push(self.composer.snapshot());
        self.composer.restore(snapshot);
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn composer_redo(&mut self) {
        if self.composer.editor_matches_prompt_fields()
            && matches!(self.composer.editor_redo(), Ok(true))
        {
            self.sync_slash_overlay();
            self.sync_file_mention_overlay();
            return;
        }
        let Some(snapshot) = self.composer.redo_stack.pop() else {
            return;
        };
        self.composer.undo_stack.push(self.composer.snapshot());
        self.composer.restore(snapshot);
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }
}

// ---------------------------------------------------------------------------
// Completion overlay — pub(crate) surface for unit-test contracts.
// Todo 13 will promote this to the public API with proper state ownership on
// AppState/ComposerState (routed through Todo 6 for the shared-root field
// additions).  Until then the overlay borrows the existing slash-overlay
// fields (slash_visible / slash_filtered / slash_selected) and a thread-local
// for the replace range so that all completion assertions compile and run
// against the real prompt-editing path.
// ---------------------------------------------------------------------------

#[cfg(test)]
thread_local! {
    static COMPLETION_REPLACE_RANGE: std::cell::RefCell<Option<(usize, usize)>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
impl AppState {
    /// Open the completion overlay with a candidate list and the buffer char
    /// range to replace on accept.
    pub(crate) fn open_completions(&mut self, entries: Vec<String>, start: usize, end: usize) {
        if entries.is_empty() {
            self.clear_slash_menu();
            COMPLETION_REPLACE_RANGE.with(|r| *r.borrow_mut() = None);
            return;
        }
        self.slash_visible = true;
        self.slash_filtered = entries;
        self.slash_selected = 0;
        COMPLETION_REPLACE_RANGE.with(|r| *r.borrow_mut() = Some((start, end)));
    }

    pub(crate) fn completion_active(&self) -> bool {
        self.slash_visible
    }

    pub(crate) fn completion_entries(&self) -> Vec<String> {
        self.slash_filtered.clone()
    }

    pub(crate) fn completion_selected_index(&self) -> Option<usize> {
        if self.slash_visible {
            Some(self.slash_selected)
        } else {
            None
        }
    }

    pub(crate) fn completion_selected_text(&self) -> Option<String> {
        if self.slash_visible {
            self.slash_filtered.get(self.slash_selected).cloned()
        } else {
            None
        }
    }

    pub(crate) fn completion_replace_range(&self) -> Option<(usize, usize)> {
        COMPLETION_REPLACE_RANGE.with(|r| *r.borrow())
    }

    /// Accept the currently selected candidate: push a single undo snapshot,
    /// delete the target range, insert the replacement text, and deactivate.
    pub(crate) fn accept_completion(&mut self) {
        if !self.slash_visible {
            return;
        }
        let selected = self.slash_filtered.get(self.slash_selected).cloned();
        let range = COMPLETION_REPLACE_RANGE.with(|r| *r.borrow());
        self.clear_slash_menu();
        COMPLETION_REPLACE_RANGE.with(|r| *r.borrow_mut() = None);
        let (Some(selected), Some((start, end))) = (selected, range) else {
            return;
        };
        self.composer.push_undo();
        self.delete_prompt_range(start, end);
        // delete_prompt_range moves the cursor past the deleted region; reset
        // to the range start so the replacement text lands in the right spot.
        self.composer.prompt_cursor = start;
        let byte_idx = self.prompt_cursor_byte_index();
        self.composer.prompt_buffer.insert_str(byte_idx, &selected);
        self.composer.prompt_cursor += selected.chars().count();
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(crate) fn cycle_completion_forward(&mut self) {
        if !self.slash_visible || self.slash_filtered.is_empty() {
            return;
        }
        self.slash_selected = (self.slash_selected + 1) % self.slash_filtered.len();
    }

    pub(crate) fn cycle_completion_backward(&mut self) {
        if !self.slash_visible || self.slash_filtered.is_empty() {
            return;
        }
        let len = self.slash_filtered.len();
        self.slash_selected = if self.slash_selected == 0 {
            len - 1
        } else {
            self.slash_selected - 1
        };
    }

    pub(crate) fn dismiss_completion(&mut self) {
        self.clear_slash_menu();
        COMPLETION_REPLACE_RANGE.with(|r| *r.borrow_mut() = None);
    }

    /// Filter candidates by the alphanumeric word under the cursor, sort
    /// deterministically, and open the overlay targeting that word's range.
    pub(crate) fn open_completions_for_word(&mut self, candidates: &[String]) {
        let word_start = self.composer_word_start_left();
        let word_end = self.composer.prompt_cursor;
        let chars = self.composer_chars();
        let word: String = chars
            .get(word_start..word_end)
            .map(|slice| slice.iter().collect())
            .unwrap_or_default();
        let mut filtered: Vec<String> = candidates
            .iter()
            .filter(|c| c.starts_with(&word))
            .cloned()
            .collect();
        filtered.sort();
        self.open_completions(filtered, word_start, word_end);
    }
}

#[cfg(test)]
mod completion_tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::unwrap_used,
        reason = "consistency contract tests use fail-fast asserts for missing state"
    )]

    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    // --- test helpers (same shape as the integration-test originals) ---

    fn live_app() -> AppState {
        AppState::new_live(None, false, None)
    }

    fn set_buffer(app: &mut AppState, text: &str) {
        app.composer.prompt_buffer = text.to_string();
        app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
        app.composer.selection_anchor = None;
    }

    fn set_buffer_at(app: &mut AppState, text: &str, cursor: usize) {
        app.composer.prompt_buffer = text.to_string();
        app.composer.prompt_cursor = cursor;
        app.composer.selection_anchor = None;
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    // --- E1: opening with candidates activates the overlay at index 0. ---
    #[test]
    fn completion_open_activates_with_candidates() {
        // arrange
        let mut app = live_app();
        set_buffer(&mut app, "hello ");

        // act
        app.open_completions(vec!["alpha".into(), "beta".into(), "gamma".into()], 0, 0);

        // assert
        assert!(app.completion_active());
        assert_eq!(app.completion_entries(), vec!["alpha", "beta", "gamma"]);
        assert_eq!(app.completion_selected_index(), Some(0));
        assert_eq!(app.completion_selected_text(), Some("alpha".into()));
        assert_eq!(app.completion_replace_range(), Some((0, 0)));
    }

    // --- E-Reject: opening with no candidates stays inactive. ---
    #[test]
    fn completion_open_with_no_candidates_is_inactive() {
        // arrange
        let mut app = live_app();

        // act
        app.open_completions(Vec::new(), 0, 0);

        // assert
        assert!(!app.completion_active());
        assert_eq!(app.completion_selected_index(), None);
        assert_eq!(app.completion_selected_text(), None);
        assert_eq!(app.completion_replace_range(), None);
    }

    // --- E2: forward cycling wraps around the candidate list. ---
    #[test]
    fn completion_cycle_forward_wraps_around() {
        // arrange
        let mut app = live_app();
        app.open_completions(vec!["a".into(), "b".into(), "c".into()], 0, 0);

        // act
        app.cycle_completion_forward();
        // assert
        assert_eq!(app.completion_selected_index(), Some(1));
        app.cycle_completion_forward();
        assert_eq!(app.completion_selected_index(), Some(2));
        app.cycle_completion_forward();
        assert_eq!(app.completion_selected_index(), Some(0), "wraps to first");
    }

    // --- E3: backward cycling wraps around the candidate list. ---
    #[test]
    fn completion_cycle_backward_wraps_around() {
        // arrange
        let mut app = live_app();
        app.open_completions(vec!["a".into(), "b".into(), "c".into()], 0, 0);

        // act
        app.cycle_completion_backward();
        // assert
        assert_eq!(app.completion_selected_index(), Some(2), "wraps to last");
        app.cycle_completion_backward();
        assert_eq!(app.completion_selected_index(), Some(1));
    }

    // --- E4: accepting replaces the target range, moves the cursor, and
    //     deactivates. ---
    #[test]
    fn completion_accept_replaces_range_and_deactivates() {
        // arrange
        let mut app = live_app();
        set_buffer(&mut app, "abXXef");
        app.open_completions(vec!["ZZ".into()], 2, 4);

        // act
        app.accept_completion();

        // assert
        assert_eq!(app.composer.prompt_buffer, "abZZef");
        assert_eq!(app.composer.prompt_cursor, 4);
        assert!(!app.completion_active());
        assert_eq!(app.composer.selection_anchor, None);
    }

    // --- E5: accepting is undoable via the composer undo stack. ---
    #[test]
    fn completion_accept_is_undoable() {
        // arrange
        let mut app = live_app();
        set_buffer_at(&mut app, "abc", 3);
        app.open_completions(vec!["XYZ".into()], 0, 3);
        app.accept_completion();
        assert_eq!(app.composer.prompt_buffer, "XYZ");

        // act
        app.handle_key(ctrl('z'));

        // assert
        assert_eq!(app.composer.prompt_buffer, "abc");
        assert_eq!(app.composer.prompt_cursor, 3);
    }

    // --- E6: dismissing closes the overlay without touching the buffer. ---
    #[test]
    fn completion_dismiss_leaves_buffer_intact() {
        // arrange
        let mut app = live_app();
        set_buffer(&mut app, "draft text");
        app.open_completions(vec!["one".into(), "two".into()], 0, 0);
        assert!(app.completion_active());

        // act
        app.dismiss_completion();

        // assert
        assert!(!app.completion_active());
        assert_eq!(app.composer.prompt_buffer, "draft text");
        assert_eq!(app.completion_selected_index(), None);
    }

    // --- E7: word-prefix completion filters candidates by the word under the
    //     cursor, sorts them deterministically, and accepts into place. ---
    #[test]
    fn completion_for_word_filters_by_prefix_and_accepts() {
        // arrange
        let mut app = live_app();
        set_buffer(&mut app, "hello wo");
        let candidates = vec![
            "world".to_string(),
            "wonder".to_string(),
            "apple".to_string(),
        ];

        app.open_completions_for_word(&candidates);

        assert!(app.completion_active());
        assert_eq!(app.completion_entries(), vec!["wonder", "world"]);
        assert_eq!(app.completion_selected_text(), Some("wonder".into()));
        assert_eq!(app.completion_replace_range(), Some((6, 8)));

        // act
        app.accept_completion();

        // assert
        assert_eq!(app.composer.prompt_buffer, "hello wonder");
        assert_eq!(app.composer.prompt_cursor, 12);
        assert!(!app.completion_active());
    }

    // --- E-Reject: word completion with no matching candidate stays inactive. ---
    #[test]
    fn completion_for_word_no_match_is_inactive() {
        // arrange
        let mut app = live_app();
        set_buffer(&mut app, "hello xy");
        let candidates = vec!["world".to_string()];

        // act
        app.open_completions_for_word(&candidates);

        // assert
        assert!(!app.completion_active());
        assert_eq!(app.composer.prompt_buffer, "hello xy");
    }

    // --- E-Reject: cycling/accepting while inactive are safe no-ops. ---
    #[test]
    fn completion_ops_are_noops_when_inactive() {
        // arrange
        let mut app = live_app();
        set_buffer(&mut app, "abc");

        // act
        app.cycle_completion_forward();
        app.cycle_completion_backward();
        app.accept_completion();
        app.dismiss_completion();

        // assert
        assert!(!app.completion_active());
        assert_eq!(app.composer.prompt_buffer, "abc");
        assert_eq!(app.completion_selected_index(), None);
    }
}
