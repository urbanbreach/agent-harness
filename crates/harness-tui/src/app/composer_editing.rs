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
        let Some(snapshot) = self.composer.undo_stack.pop() else {
            return;
        };
        self.composer.redo_stack.push(self.composer.snapshot());
        self.composer.restore(snapshot);
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn composer_redo(&mut self) {
        let Some(snapshot) = self.composer.redo_stack.pop() else {
            return;
        };
        self.composer.undo_stack.push(self.composer.snapshot());
        self.composer.restore(snapshot);
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }
}
