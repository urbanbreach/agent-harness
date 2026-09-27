use std::sync::Arc;

use crate::composer_atoms::{AtomBuffer, AtomCursor};

use super::history::PromptHistory;
use super::selection::Selection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorSnapshot {
    pub buffer: AtomBuffer,
    pub cursor: AtomCursor,
    pub selection: Option<Selection>,
    pub history: PromptHistory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditGroup {
    CharacterDelete,
    WordDelete,
    LineDelete,
    AttachmentInsertion,
    Paste,
    History,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UndoEntry {
    before: Arc<EditorSnapshot>,
    after: Arc<EditorSnapshot>,
    group: EditGroup,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UndoStack {
    undo: Vec<UndoEntry>,
    redo: Vec<UndoEntry>,
}

impl UndoStack {
    pub fn record(&mut self, before: EditorSnapshot, after: EditorSnapshot, group: EditGroup) {
        self.record_shared(Arc::new(before), Arc::new(after), group);
    }

    pub(super) fn latest(&self) -> Option<&Arc<EditorSnapshot>> {
        self.undo.last().map(|entry| &entry.after)
    }

    pub(super) fn record_shared(
        &mut self,
        before: Arc<EditorSnapshot>,
        after: Arc<EditorSnapshot>,
        group: EditGroup,
    ) {
        if before == after {
            return;
        }
        if group == EditGroup::CharacterDelete
            && self
                .undo
                .last()
                .is_some_and(|entry| entry.group == group && entry.after == before)
        {
            if let Some(entry) = self.undo.last_mut() {
                entry.after = after;
            }
        } else {
            self.undo.push(UndoEntry {
                before,
                after,
                group,
            });
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, current: &EditorSnapshot) -> Option<EditorSnapshot> {
        self.undo.last()?;
        self.undo_shared(Arc::new(current.clone()))
            .map(Arc::unwrap_or_clone)
    }

    fn undo_shared(&mut self, current: Arc<EditorSnapshot>) -> Option<Arc<EditorSnapshot>> {
        let entry = self.undo.pop()?;
        self.redo.push(UndoEntry {
            before: current,
            after: entry.after,
            group: entry.group,
        });
        Some(entry.before)
    }

    pub fn redo(&mut self, current: &EditorSnapshot) -> Option<EditorSnapshot> {
        self.redo.last()?;
        self.redo_shared(Arc::new(current.clone()))
            .map(Arc::unwrap_or_clone)
    }

    fn redo_shared(&mut self, current: Arc<EditorSnapshot>) -> Option<Arc<EditorSnapshot>> {
        let entry = self.redo.pop()?;
        self.undo.push(UndoEntry {
            before: current,
            after: Arc::clone(&entry.after),
            group: entry.group,
        });
        Some(entry.after)
    }

    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }
}

impl super::ComposerEditor {
    pub fn undo(&mut self) -> bool {
        let current = self.snapshot();
        let Some(snapshot) = self.undo.undo_shared(current) else {
            return false;
        };
        self.restore(snapshot);
        true
    }

    pub fn redo(&mut self) -> bool {
        let current = self.snapshot();
        let Some(snapshot) = self.undo.redo_shared(current) else {
            return false;
        };
        self.restore(snapshot);
        true
    }

    pub fn undo_depth(&self) -> usize {
        self.undo.undo_depth()
    }

    pub fn redo_depth(&self) -> usize {
        self.undo.redo_depth()
    }
}
