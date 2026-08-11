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
    pub(super) fn parity_text(&self) -> String {
        self.slice.editor().text()
    }

    pub(super) fn parity_cursor(&self) -> usize {
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

    pub(super) fn sync_legacy_from_parity(&mut self) {
        self.prompt_buffer = self.parity_text();
        self.prompt_cursor = self.parity_cursor();
        self.selection_anchor = None;
    }

    pub(super) fn parity_editing_ready(&self) -> bool {
        self.parity_text() == self.prompt_buffer
            && self.parity_cursor() == self.prompt_cursor
            && self.selection_anchor.is_none()
    }

    fn sync_parity_from_legacy(&mut self) {
        if self.parity_text() != self.prompt_buffer {
            let _ = self.slice.replace_text(&self.prompt_buffer);
        }
    }

    pub(super) fn replace_parity_text(&mut self, text: &str) -> Result<(), ComposerSliceError> {
        self.sync_parity_from_legacy();
        self.slice.replace_text(text)?;
        self.sync_legacy_from_parity();
        Ok(())
    }

    pub(super) fn parity_insert_text(&mut self, text: &str) -> Result<(), ComposerSliceError> {
        self.sync_parity_from_legacy();
        self.slice.insert_text(text)?;
        self.sync_legacy_from_parity();
        Ok(())
    }

    pub(super) fn parity_paste(&mut self, text: &str) -> Result<(), ComposerSliceError> {
        self.sync_parity_from_legacy();
        self.slice.paste(text)?;
        self.sync_legacy_from_parity();
        Ok(())
    }

    pub(super) fn parity_backspace(&mut self) -> Result<(), ComposerSliceError> {
        self.sync_parity_from_legacy();
        self.slice.backspace()?;
        self.sync_legacy_from_parity();
        Ok(())
    }

    pub(super) fn parity_delete(
        &mut self,
        kind: crate::composer_editing::DeleteKind,
    ) -> Result<(), ComposerSliceError> {
        self.sync_parity_from_legacy();
        self.slice.delete(kind)?;
        self.sync_legacy_from_parity();
        Ok(())
    }

    pub(super) fn parity_move_left(&mut self) -> Result<(), ComposerSliceError> {
        self.sync_parity_from_legacy();
        self.slice.move_left()?;
        self.sync_legacy_from_parity();
        Ok(())
    }

    pub(super) fn parity_move_right(&mut self) -> Result<(), ComposerSliceError> {
        self.sync_parity_from_legacy();
        self.slice.move_right()?;
        self.sync_legacy_from_parity();
        Ok(())
    }

    pub(super) fn parity_undo(&mut self) -> Result<bool, ComposerSliceError> {
        let changed = self.slice.undo()?;
        if changed {
            self.sync_legacy_from_parity();
        }
        Ok(changed)
    }

    pub(super) fn parity_redo(&mut self) -> Result<bool, ComposerSliceError> {
        let changed = self.slice.redo()?;
        if changed {
            self.sync_legacy_from_parity();
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
