use std::path::PathBuf;

use super::prompt_history::PromptHistoryDraft;

const UNDO_STACK_MAX: usize = 100;

#[derive(Debug, Clone)]
pub(super) struct ComposerSnapshot {
    pub(super) text: String,
    pub(super) cursor: usize,
    pub(super) selection_anchor: Option<usize>,
}

#[derive(Default)]
pub struct ComposerState {
    pub prompt_buffer: String,
    pub prompt_cursor: usize,
    pub selection_anchor: Option<usize>,
    pub prompt_history: Vec<String>,
    pub prompt_history_index: Option<usize>,
    pub(super) prompt_history_path: Option<PathBuf>,
    pub(super) prompt_history_draft: Option<PromptHistoryDraft>,
    pub(super) undo_stack: Vec<ComposerSnapshot>,
    pub(super) redo_stack: Vec<ComposerSnapshot>,
    pub shell_mode: bool,
    pub multiline_mode: bool,
}

impl ComposerState {
    pub(super) fn snapshot(&self) -> ComposerSnapshot {
        ComposerSnapshot {
            text: self.prompt_buffer.clone(),
            cursor: self.prompt_cursor,
            selection_anchor: self.selection_anchor,
        }
    }

    pub(super) fn push_undo(&mut self) {
        if self.undo_stack.len() >= UNDO_STACK_MAX {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(self.snapshot());
        self.redo_stack.clear();
    }

    pub(super) fn restore(&mut self, snapshot: ComposerSnapshot) {
        self.prompt_buffer = snapshot.text;
        self.prompt_cursor = snapshot.cursor;
        self.selection_anchor = snapshot.selection_anchor;
    }
}
