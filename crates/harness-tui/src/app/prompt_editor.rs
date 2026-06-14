use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use super::file_mentions::prompt_char_to_byte;
use super::{Action, AppState, ComposerState, FileMentionTag};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ComposerMode {
    #[default]
    Prompt,
    Shell,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app) struct ComposerSnapshot {
    text: String,
    cursor: usize,
    selection_anchor: Option<usize>,
    file_tags: Vec<FileMentionTag>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PromptStashEntry {
    text: String,
    cursor: usize,
    file_tags: Vec<FileMentionTag>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueuedPromptEntry {
    text: String,
    request_id: Option<String>,
    task_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::app) struct PendingQueuedPromptCancellation {
    text: String,
    request_id: Option<String>,
}

impl PromptStashEntry {
    pub(in crate::app) fn persisted(text: String, cursor: usize) -> Self {
        Self {
            text,
            cursor,
            file_tags: Vec::new(),
        }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(in crate::app) fn cursor(&self) -> usize {
        self.cursor
    }
}

impl QueuedPromptEntry {
    pub(in crate::app) fn new(text: String) -> Self {
        Self {
            text,
            request_id: None,
            task_id: None,
        }
    }

    pub(in crate::app) fn scheduled(text: String, request_id: String, task_id: String) -> Self {
        Self {
            text,
            request_id: Some(request_id),
            task_id: Some(task_id),
        }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(in crate::app) fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    pub(in crate::app) fn task_id(&self) -> Option<&str> {
        self.task_id.as_deref()
    }

    pub(in crate::app) fn attach_request_id(&mut self, request_id: String) {
        self.request_id = Some(request_id);
    }

    pub(in crate::app) fn attach_task_id(&mut self, task_id: String) {
        self.task_id = Some(task_id);
    }
}

impl PendingQueuedPromptCancellation {
    pub(in crate::app) fn from_entry(entry: QueuedPromptEntry) -> Self {
        Self {
            text: entry.text,
            request_id: entry.request_id,
        }
    }

    pub(in crate::app) fn matches_unclaimed_text(&self, text: &str) -> bool {
        self.request_id.is_none() && self.text == text
    }

    pub(in crate::app) fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    pub(in crate::app) fn attach_request_id(&mut self, request_id: String) {
        self.request_id = Some(request_id);
    }
}

impl ComposerState {
    pub fn mode(&self) -> ComposerMode {
        self.mode
    }

    pub fn selection_range(&self) -> Option<Range<usize>> {
        let anchor = self.selection_anchor?;
        (anchor != self.prompt_cursor).then(|| {
            let start = anchor.min(self.prompt_cursor);
            let end = anchor.max(self.prompt_cursor);
            start..end
        })
    }

    pub fn stash_len(&self) -> usize {
        self.prompt_stash.len()
    }

    pub fn stash_dialog_visible(&self) -> bool {
        self.stash_dialog_visible
    }

    pub(crate) fn prompt_stash_entries(&self) -> &[PromptStashEntry] {
        &self.prompt_stash
    }

    pub(crate) fn prompt_stash_selected(&self) -> usize {
        self.prompt_stash_selected
    }

    pub fn queued_prompt_count(&self) -> usize {
        self.queued_prompt_count
    }

    pub fn queued_prompt_dialog_visible(&self) -> bool {
        self.queued_prompt_dialog_visible
    }

    pub(crate) fn queued_prompt_entries(&self) -> &[QueuedPromptEntry] {
        &self.queued_prompts
    }

    pub(crate) fn queued_prompt_selected(&self) -> usize {
        self.queued_prompt_selected
    }
}

impl AppState {
    fn composer_snapshot(&self) -> ComposerSnapshot {
        ComposerSnapshot {
            text: self.composer.prompt_buffer.clone(),
            cursor: self.composer.prompt_cursor,
            selection_anchor: self.composer.selection_anchor,
            file_tags: self.file_mention_tags.clone(),
        }
    }

    pub(in crate::app) fn record_prompt_edit_snapshot(&mut self) {
        self.composer.undo_stack.push(self.composer_snapshot());
        self.composer.redo_stack.clear();
    }

    pub(in crate::app) fn restore_composer_snapshot(&mut self, snapshot: ComposerSnapshot) {
        self.composer.prompt_buffer = snapshot.text;
        self.composer.prompt_cursor = snapshot
            .cursor
            .min(self.composer.prompt_buffer.chars().count());
        self.composer.selection_anchor = snapshot.selection_anchor;
        self.file_mention_tags = snapshot.file_tags;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn undo_prompt_edit(&mut self) {
        let Some(snapshot) = self.composer.undo_stack.pop() else {
            return;
        };
        self.composer.redo_stack.push(self.composer_snapshot());
        self.restore_composer_snapshot(snapshot);
    }

    pub(in crate::app) fn redo_prompt_edit(&mut self) {
        let Some(snapshot) = self.composer.redo_stack.pop() else {
            return;
        };
        self.composer.undo_stack.push(self.composer_snapshot());
        self.restore_composer_snapshot(snapshot);
    }

    pub(in crate::app) fn clear_prompt_selection(&mut self) {
        self.composer.selection_anchor = None;
    }

    pub(in crate::app) fn move_prompt_cursor_to(&mut self, cursor: usize, selecting: bool) {
        let next = cursor.min(self.prompt_char_count());
        if selecting {
            if self.composer.selection_anchor.is_none() {
                self.composer.selection_anchor = Some(self.composer.prompt_cursor);
            }
        } else {
            self.clear_prompt_selection();
        }
        self.composer.prompt_cursor = next;
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn selected_prompt_range(&self) -> Option<Range<usize>> {
        self.composer.selection_range()
    }

    pub(in crate::app) fn delete_prompt_range_without_snapshot(&mut self, range: Range<usize>) {
        if range.start >= range.end {
            return;
        }
        let start_byte = prompt_char_to_byte(&self.composer.prompt_buffer, range.start);
        let end_byte = prompt_char_to_byte(&self.composer.prompt_buffer, range.end);
        self.adjust_file_mention_tags_for_delete(range.start, range.end);
        self.composer
            .prompt_buffer
            .replace_range(start_byte..end_byte, "");
        self.composer.prompt_cursor = range.start.min(self.prompt_char_count());
        self.clear_prompt_selection();
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn delete_prompt_selection_without_snapshot(&mut self) -> bool {
        let Some(range) = self.selected_prompt_range() else {
            return false;
        };
        self.delete_prompt_range_without_snapshot(range);
        true
    }

    pub(in crate::app) fn select_all_prompt(&mut self) {
        self.composer.selection_anchor = Some(0);
        self.composer.prompt_cursor = self.prompt_char_count();
    }

    pub(in crate::app) fn line_start_before_prompt_cursor(&self) -> usize {
        self.composer
            .prompt_buffer
            .chars()
            .take(self.composer.prompt_cursor)
            .enumerate()
            .filter_map(|(index, ch)| (ch == '\n').then_some(index + 1))
            .last()
            .unwrap_or(0)
    }

    pub(in crate::app) fn line_end_after_prompt_cursor(&self) -> usize {
        let cursor = self.composer.prompt_cursor;
        self.composer
            .prompt_buffer
            .chars()
            .enumerate()
            .skip(cursor)
            .find_map(|(index, ch)| (ch == '\n').then_some(index))
            .unwrap_or_else(|| self.prompt_char_count())
    }

    pub(in crate::app) fn previous_grapheme_boundary(&self) -> usize {
        grapheme_boundaries(&self.composer.prompt_buffer)
            .into_iter()
            .take_while(|boundary| *boundary < self.composer.prompt_cursor)
            .last()
            .unwrap_or(0)
    }

    pub(in crate::app) fn next_grapheme_boundary(&self) -> usize {
        grapheme_boundaries(&self.composer.prompt_buffer)
            .into_iter()
            .find(|boundary| *boundary > self.composer.prompt_cursor)
            .unwrap_or_else(|| self.prompt_char_count())
    }

    pub(in crate::app) fn previous_word_boundary(&self) -> usize {
        let graphemes = grapheme_ranges(&self.composer.prompt_buffer);
        let mut index = graphemes
            .iter()
            .rposition(|g| g.start < self.composer.prompt_cursor)
            .map(|pos| pos + 1)
            .unwrap_or(0);
        while index > 0 && !is_word_grapheme(&graphemes[index - 1].text) {
            index -= 1;
        }
        while index > 0 && is_word_grapheme(&graphemes[index - 1].text) {
            index -= 1;
        }
        graphemes
            .get(index)
            .map(|g| g.start)
            .unwrap_or(0)
            .min(self.composer.prompt_cursor)
    }

    pub(in crate::app) fn next_word_boundary(&self) -> usize {
        let graphemes = grapheme_ranges(&self.composer.prompt_buffer);
        let mut index = graphemes
            .iter()
            .position(|g| g.end > self.composer.prompt_cursor)
            .unwrap_or(graphemes.len());
        while index < graphemes.len() && !is_word_grapheme(&graphemes[index].text) {
            index += 1;
        }
        while index < graphemes.len() && is_word_grapheme(&graphemes[index].text) {
            index += 1;
        }
        graphemes
            .get(index.saturating_sub(1))
            .map(|g| g.end)
            .unwrap_or_else(|| self.prompt_char_count())
    }

    pub(in crate::app) fn set_composer_mode(&mut self, mode: ComposerMode) {
        self.composer.mode = mode;
        self.clear_prompt_selection();
    }

    pub(in crate::app) fn save_prompt_to_stash(&mut self) {
        if self.composer.prompt_buffer.is_empty() {
            return;
        }
        self.composer.prompt_stash.push(PromptStashEntry {
            text: self.composer.prompt_buffer.clone(),
            cursor: self.composer.prompt_cursor,
            file_tags: self.file_mention_tags.clone(),
        });
        self.composer.prompt_stash_selected = self.composer.prompt_stash.len().saturating_sub(1);
        self.clear_prompt_input();
        self.composer.stash_dialog_visible = false;
        self.save_prompt_stash();
    }

    pub(in crate::app) fn pop_prompt_stash(&mut self) {
        let Some(entry) = self.composer.prompt_stash.pop() else {
            return;
        };
        self.composer.prompt_buffer = entry.text;
        self.composer.prompt_cursor = entry.cursor.min(self.prompt_char_count());
        self.file_mention_tags = entry.file_tags;
        self.composer.prompt_stash_selected = self
            .composer
            .prompt_stash_selected
            .min(self.composer.prompt_stash.len().saturating_sub(1));
        self.composer.stash_dialog_visible = false;
        self.clear_prompt_selection();
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
        self.save_prompt_stash();
    }

    pub(in crate::app) fn show_prompt_stash(&mut self) {
        self.close_palette();
        self.clear_slash_menu();
        self.clear_file_mention_menu();
        self.composer.stash_dialog_visible = true;
        self.composer.queued_prompt_dialog_visible = false;
    }

    pub(in crate::app) fn close_prompt_stash(&mut self) {
        self.composer.stash_dialog_visible = false;
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    pub(in crate::app) fn move_prompt_stash_selection(&mut self, delta: isize) {
        self.composer.prompt_stash_selected = move_prompt_management_selection(
            self.composer.prompt_stash_selected,
            delta,
            self.composer.prompt_stash.len(),
        );
    }

    pub(in crate::app) fn pop_selected_prompt_stash(&mut self) {
        if self.composer.prompt_stash.is_empty() {
            self.close_prompt_stash();
            return;
        }
        let selected = self
            .composer
            .prompt_stash_selected
            .min(self.composer.prompt_stash.len().saturating_sub(1));
        let entry = self.composer.prompt_stash.remove(selected);
        self.composer.prompt_buffer = entry.text;
        self.composer.prompt_cursor = entry.cursor.min(self.prompt_char_count());
        self.file_mention_tags = entry.file_tags;
        self.composer.prompt_stash_selected =
            selected.min(self.composer.prompt_stash.len().saturating_sub(1));
        self.close_prompt_stash();
        self.clear_prompt_selection();
        self.save_prompt_stash();
    }

    pub(in crate::app) fn delete_selected_prompt_stash(&mut self) {
        if self.composer.queued_prompt_dialog_visible {
            self.delete_selected_queued_prompt_preview();
            return;
        }
        if self.composer.prompt_stash.is_empty() {
            self.composer.stash_dialog_visible = false;
            return;
        }
        let selected = self
            .composer
            .prompt_stash_selected
            .min(self.composer.prompt_stash.len().saturating_sub(1));
        self.composer.prompt_stash.remove(selected);
        self.composer.prompt_stash_selected =
            selected.min(self.composer.prompt_stash.len().saturating_sub(1));
        self.save_prompt_stash();
    }

    pub(crate) fn prompt_stash_visual_row_count(&self) -> usize {
        self.composer.prompt_stash.len().max(1)
    }

    pub(in crate::app) fn execute_prompt_editor_action(&mut self, action: Action) -> bool {
        match action {
            Action::InputSelectLeft => {
                self.move_prompt_cursor_to(self.previous_grapheme_boundary(), true)
            }
            Action::InputSelectRight => {
                self.move_prompt_cursor_to(self.next_grapheme_boundary(), true)
            }
            Action::InputSelectUp => self.move_prompt_cursor_line(true, true),
            Action::InputSelectDown => self.move_prompt_cursor_line(false, true),
            Action::InputLineStart => {
                self.move_prompt_cursor_to(self.line_start_before_prompt_cursor(), false)
            }
            Action::InputLineEnd => {
                self.move_prompt_cursor_to(self.line_end_after_prompt_cursor(), false)
            }
            Action::InputSelectLineStart => {
                self.move_prompt_cursor_to(self.line_start_before_prompt_cursor(), true)
            }
            Action::InputSelectLineEnd => {
                self.move_prompt_cursor_to(self.line_end_after_prompt_cursor(), true)
            }
            Action::InputWordBackward => {
                self.move_prompt_cursor_to(self.previous_word_boundary(), false)
            }
            Action::InputWordForward => {
                self.move_prompt_cursor_to(self.next_word_boundary(), false)
            }
            Action::InputSelectWordBackward => {
                self.move_prompt_cursor_to(self.previous_word_boundary(), true)
            }
            Action::InputSelectWordForward => {
                self.move_prompt_cursor_to(self.next_word_boundary(), true)
            }
            Action::InputDeleteWordBackward => {
                let start = self.previous_word_boundary();
                self.delete_prompt_range_with_snapshot(start..self.composer.prompt_cursor);
            }
            Action::InputDeleteWordForward => {
                let end = self.next_word_boundary();
                self.delete_prompt_range_with_snapshot(self.composer.prompt_cursor..end);
            }
            Action::InputDeleteLine => {
                let start =
                    line_start_at(&self.composer.prompt_buffer, self.composer.prompt_cursor);
                let mut end =
                    line_end_at(&self.composer.prompt_buffer, self.composer.prompt_cursor);
                if end < self.prompt_char_count() {
                    end += 1;
                }
                self.delete_prompt_range_with_snapshot(start..end);
            }
            Action::InputDeleteToLineStart => {
                let end = self
                    .selected_prompt_range()
                    .map(|range| range.end)
                    .unwrap_or(self.composer.prompt_cursor);
                let start = line_start_at(&self.composer.prompt_buffer, end);
                self.delete_prompt_range_with_snapshot(start..end);
            }
            Action::InputDeleteToLineEnd => {
                let start = self
                    .selected_prompt_range()
                    .map(|range| range.start)
                    .unwrap_or(self.composer.prompt_cursor);
                let end = line_end_at(&self.composer.prompt_buffer, start);
                self.delete_prompt_range_with_snapshot(start..end);
            }
            Action::InputSelectAll => self.select_all_prompt(),
            Action::InputUndo => self.undo_prompt_edit(),
            Action::InputRedo => self.redo_prompt_edit(),
            Action::PromptStash => self.save_prompt_to_stash(),
            Action::PromptStashPop => self.pop_prompt_stash(),
            Action::PromptStashList => self.show_prompt_stash(),
            Action::PromptStashDeleteSelected => self.delete_selected_prompt_stash(),
            Action::QueuedPrompts => self.show_queued_prompts(),
            _ => return false,
        }
        true
    }

    fn move_prompt_cursor_line(&mut self, up: bool, selecting: bool) {
        if selecting && self.composer.selection_anchor.is_none() {
            self.composer.selection_anchor = Some(self.composer.prompt_cursor);
        }
        let moved = if up {
            self.move_prompt_cursor_up()
        } else {
            self.move_prompt_cursor_down()
        };
        if moved && !selecting {
            self.clear_prompt_selection();
        }
        if moved {
            self.sync_file_mention_overlay();
        }
    }

    fn delete_prompt_range_with_snapshot(&mut self, range: Range<usize>) {
        if range.start >= range.end {
            self.clear_prompt_selection();
            return;
        }
        self.record_prompt_edit_snapshot();
        self.delete_prompt_range_without_snapshot(range);
    }
}

pub(in crate::app) fn is_prompt_editor_action(action: Action) -> bool {
    matches!(
        action,
        Action::InputSelectLeft
            | Action::InputSelectRight
            | Action::InputSelectUp
            | Action::InputSelectDown
            | Action::InputLineStart
            | Action::InputLineEnd
            | Action::InputSelectLineStart
            | Action::InputSelectLineEnd
            | Action::InputWordBackward
            | Action::InputWordForward
            | Action::InputSelectWordBackward
            | Action::InputSelectWordForward
            | Action::InputDeleteWordBackward
            | Action::InputDeleteWordForward
            | Action::InputDeleteLine
            | Action::InputDeleteToLineStart
            | Action::InputDeleteToLineEnd
            | Action::InputSelectAll
            | Action::InputUndo
            | Action::InputRedo
            | Action::PromptStash
            | Action::PromptStashPop
            | Action::PromptStashList
            | Action::PromptStashDeleteSelected
            | Action::QueuedPrompts
    )
}

#[derive(Debug)]
struct GraphemeRange {
    start: usize,
    end: usize,
    text: String,
}

fn grapheme_boundaries(value: &str) -> Vec<usize> {
    let mut boundaries = value
        .grapheme_indices(true)
        .map(|(byte, _)| value[..byte].chars().count())
        .collect::<Vec<_>>();
    boundaries.push(value.chars().count());
    boundaries.sort_unstable();
    boundaries.dedup();
    boundaries
}

fn grapheme_ranges(value: &str) -> Vec<GraphemeRange> {
    value
        .grapheme_indices(true)
        .map(|(byte, grapheme)| {
            let start = value[..byte].chars().count();
            GraphemeRange {
                start,
                end: start + grapheme.chars().count(),
                text: grapheme.to_string(),
            }
        })
        .collect()
}

fn is_word_grapheme(value: &str) -> bool {
    value.chars().any(|ch| ch.is_alphanumeric() || ch == '_')
}

fn line_start_at(value: &str, cursor: usize) -> usize {
    value
        .chars()
        .take(cursor)
        .enumerate()
        .filter_map(|(index, ch)| (ch == '\n').then_some(index + 1))
        .last()
        .unwrap_or(0)
}

fn line_end_at(value: &str, cursor: usize) -> usize {
    value
        .chars()
        .enumerate()
        .skip(cursor)
        .find_map(|(index, ch)| (ch == '\n').then_some(index))
        .unwrap_or_else(|| value.chars().count())
}

pub(in crate::app) fn move_prompt_management_selection(
    selected: usize,
    delta: isize,
    len: usize,
) -> usize {
    if len == 0 {
        return 0;
    }

    if delta == -1 {
        return if selected == 0 {
            len.saturating_sub(1)
        } else {
            selected - 1
        };
    }

    if delta == 1 {
        return (selected + 1) % len;
    }

    let current = selected.min(len.saturating_sub(1)) as isize;
    let next = (current + delta).clamp(0, len.saturating_sub(1) as isize);
    usize::try_from(next).unwrap_or(0)
}
