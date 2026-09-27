use super::*;
use unicode_segmentation::UnicodeSegmentation;

impl AppState {
    pub(in crate::app) fn prompt_grapheme_boundary(&self, forward: bool) -> usize {
        let mut boundaries =
            self.composer
                .prompt_buffer
                .graphemes(true)
                .scan(0, |end, grapheme| {
                    *end += grapheme.chars().count();
                    Some(*end)
                });
        if forward {
            boundaries
                .find(|end| *end > self.composer.prompt_cursor)
                .unwrap_or(self.prompt_char_count())
        } else {
            boundaries
                .take_while(|end| *end < self.composer.prompt_cursor)
                .last()
                .unwrap_or(0)
        }
    }

    fn composer_word_boundary(&self, forward: bool) -> usize {
        let (left, right) = self
            .composer
            .prompt_buffer
            .split_at(self.prompt_cursor_byte_index());
        if forward {
            self.prompt_char_count()
                - right
                    .chars()
                    .skip_while(|ch| ch.is_alphanumeric())
                    .skip_while(|ch| !ch.is_alphanumeric())
                    .count()
        } else {
            left.chars()
                .rev()
                .skip_while(|ch| !ch.is_alphanumeric())
                .skip_while(|ch| ch.is_alphanumeric())
                .count()
        }
    }

    fn composer_line_boundary(&self, forward: bool) -> usize {
        let (left, right) = self
            .composer
            .prompt_buffer
            .split_at(self.prompt_cursor_byte_index());
        if forward {
            self.composer.prompt_cursor + right.chars().take_while(|ch| *ch != '\n').count()
        } else {
            self.composer.prompt_cursor - left.chars().rev().take_while(|ch| *ch != '\n').count()
        }
    }

    fn move_composer_cursor(&mut self, cursor: usize, selecting: bool) {
        self.composer.selection_anchor = if selecting {
            Some(
                self.composer
                    .selection_anchor
                    .unwrap_or(self.composer.prompt_cursor),
            )
        } else {
            None
        };
        self.composer.prompt_cursor = cursor;
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn delete_prompt_range(&mut self, start: usize, end: usize) {
        let count = self.prompt_char_count();
        let start = start.min(count);
        let end = end.min(count);
        if start >= end {
            return;
        }
        self.reset_clear_prompt_confirmation();
        self.adjust_file_mention_tags_for_delete(start, end);
        let start_byte = self.prompt_char_byte_index(start);
        let end_byte = self.prompt_char_byte_index(end);
        self.composer
            .prompt_buffer
            .replace_range(start_byte..end_byte, "");
        self.composer.prompt_cursor -=
            (end - start).min(self.composer.prompt_cursor.saturating_sub(start));
        self.composer.selection_anchor = None;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn composer_select_char_left(&mut self) {
        if self.composer.prompt_cursor > 0 {
            self.move_composer_cursor(self.prompt_grapheme_boundary(false), true);
        }
    }

    pub(in crate::app) fn composer_select_char_right(&mut self) {
        if self.composer.prompt_cursor < self.prompt_char_count() {
            self.move_composer_cursor(self.prompt_grapheme_boundary(true), true);
        }
    }

    pub(in crate::app) fn composer_select_word_left(&mut self) {
        self.move_composer_cursor(self.composer_word_boundary(false), true);
    }

    pub(in crate::app) fn composer_select_word_right(&mut self) {
        self.move_composer_cursor(self.composer_word_boundary(true), true);
    }

    pub(in crate::app) fn composer_select_line(&mut self) {
        self.composer.selection_anchor = Some(self.composer_line_boundary(false));
        self.move_composer_cursor(self.composer_line_boundary(true), true);
    }

    pub(in crate::app) fn composer_select_all(&mut self) {
        self.composer.selection_anchor = Some(0);
        self.move_composer_cursor(self.prompt_char_count(), true);
    }

    pub(in crate::app) fn composer_move_word_left(&mut self) {
        self.move_composer_cursor(self.composer_word_boundary(false), false);
    }

    pub(in crate::app) fn composer_move_word_right(&mut self) {
        self.move_composer_cursor(self.composer_word_boundary(true), false);
    }

    pub(in crate::app) fn composer_move_line_start(&mut self) {
        self.move_composer_cursor(self.composer_line_boundary(false), false);
    }

    pub(in crate::app) fn composer_move_line_end(&mut self) {
        self.move_composer_cursor(self.composer_line_boundary(true), false);
    }

    pub(in crate::app) fn composer_move_buffer_start(&mut self) {
        self.move_composer_cursor(0, false);
    }

    pub(in crate::app) fn composer_move_buffer_end(&mut self) {
        self.move_composer_cursor(self.prompt_char_count(), false);
    }

    fn delete_recorded_range(&mut self, start: usize, end: usize) {
        if start < end {
            self.composer.push_undo();
            self.delete_prompt_range(start, end);
        }
    }

    pub(in crate::app) fn composer_delete_word_forward(&mut self) {
        self.delete_recorded_range(
            self.composer.prompt_cursor,
            self.composer_word_boundary(true),
        );
    }

    pub(in crate::app) fn composer_delete_word_backward(&mut self) {
        self.delete_recorded_range(
            self.composer_word_boundary(false),
            self.composer.prompt_cursor,
        );
    }

    pub(in crate::app) fn composer_delete_line(&mut self) {
        let end = self.composer_line_boundary(true);
        self.delete_recorded_range(
            self.composer_line_boundary(false),
            end + usize::from(end < self.prompt_char_count()),
        );
    }

    pub(in crate::app) fn composer_kill_to_line_start(&mut self) {
        self.delete_recorded_range(
            self.composer_line_boundary(false),
            self.composer.prompt_cursor,
        );
    }

    pub(in crate::app) fn composer_kill_to_line_end(&mut self) {
        self.delete_recorded_range(
            self.composer.prompt_cursor,
            self.composer_line_boundary(true),
        );
    }

    fn restore_composer_edit(&mut self, redo: bool) {
        self.reset_clear_prompt_confirmation();
        let restored = self.composer.editor_matches_prompt_fields()
            && if redo {
                matches!(self.composer.editor_redo(), Ok(true))
            } else {
                matches!(self.composer.editor_undo(), Ok(true))
            };
        if !restored {
            let snapshot = if redo {
                self.composer.redo_stack.pop()
            } else {
                self.composer.undo_stack.pop()
            };
            let Some(snapshot) = snapshot else {
                return;
            };
            let current = self.composer.snapshot();
            if redo {
                self.composer.undo_stack.push(current);
            } else {
                self.composer.redo_stack.push(current);
            }
            self.composer.restore(snapshot);
        }
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn composer_undo(&mut self) {
        self.restore_composer_edit(false);
    }

    pub(in crate::app) fn composer_redo(&mut self) {
        self.restore_composer_edit(true);
    }
}
