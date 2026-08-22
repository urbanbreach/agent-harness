use std::path::PathBuf;

use crate::composer_integration::{ComposerSlice, ComposerSliceError};

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
    pub(super) slice: ComposerSlice,
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
    pub(super) fn editor_text(&self) -> String {
        self.slice.editor().text()
    }

    pub(super) fn editor_cursor(&self) -> usize {
        self.slice
            .editor()
            .buffer()
            .atoms()
            .iter()
            .take(self.slice.editor().cursor().insertion_index())
            .map(|atom| match &atom.kind {
                crate::composer_atoms::AtomKind::Text(cluster) => cluster.as_str().chars().count(),
                crate::composer_atoms::AtomKind::Newline => 1,
                crate::composer_atoms::AtomKind::FileMention(_)
                | crate::composer_atoms::AtomKind::Attachment(_) => 1,
            })
            .sum()
    }

    pub(super) fn sync_prompt_fields_from_editor(&mut self) {
        self.prompt_buffer = self.editor_text();
        self.prompt_cursor = self.editor_cursor();
        self.selection_anchor = None;
    }

    pub(super) fn editor_matches_prompt_fields(&self) -> bool {
        self.editor_text() == self.prompt_buffer
            && self.editor_cursor() == self.prompt_cursor
            && self.selection_anchor.is_none()
    }

    fn sync_editor_from_prompt_buffer(&mut self) {
        if self.editor_text() != self.prompt_buffer {
            let _ = self.slice.replace_text(&self.prompt_buffer);
        }
    }

    pub(super) fn replace_editor_text(&mut self, text: &str) -> Result<(), ComposerSliceError> {
        self.sync_editor_from_prompt_buffer();
        self.slice.replace_text(text)?;
        self.sync_prompt_fields_from_editor();
        Ok(())
    }

    pub(super) fn editor_insert_text(&mut self, text: &str) -> Result<(), ComposerSliceError> {
        self.sync_editor_from_prompt_buffer();
        self.slice.insert_text(text)?;
        self.sync_prompt_fields_from_editor();
        Ok(())
    }

    pub(super) fn editor_paste(&mut self, text: &str) -> Result<(), ComposerSliceError> {
        self.sync_editor_from_prompt_buffer();
        self.slice.paste(text)?;
        self.sync_prompt_fields_from_editor();
        Ok(())
    }

    pub(super) fn editor_backspace(&mut self) -> Result<(), ComposerSliceError> {
        self.sync_editor_from_prompt_buffer();
        self.slice.backspace()?;
        self.sync_prompt_fields_from_editor();
        Ok(())
    }

    pub(super) fn editor_delete(
        &mut self,
        kind: crate::composer_editing::DeleteKind,
    ) -> Result<(), ComposerSliceError> {
        self.sync_editor_from_prompt_buffer();
        self.slice.delete(kind)?;
        self.sync_prompt_fields_from_editor();
        Ok(())
    }

    pub(super) fn editor_move_left(&mut self) -> Result<(), ComposerSliceError> {
        self.sync_editor_from_prompt_buffer();
        self.slice.move_left()?;
        self.sync_prompt_fields_from_editor();
        Ok(())
    }

    pub(super) fn editor_move_right(&mut self) -> Result<(), ComposerSliceError> {
        self.sync_editor_from_prompt_buffer();
        self.slice.move_right()?;
        self.sync_prompt_fields_from_editor();
        Ok(())
    }

    pub(super) fn editor_undo(&mut self) -> Result<bool, ComposerSliceError> {
        let changed = self.slice.undo()?;
        if changed {
            self.sync_prompt_fields_from_editor();
        }
        Ok(changed)
    }

    pub(super) fn editor_redo(&mut self) -> Result<bool, ComposerSliceError> {
        let changed = self.slice.redo()?;
        if changed {
            self.sync_prompt_fields_from_editor();
        }
        Ok(changed)
    }

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
