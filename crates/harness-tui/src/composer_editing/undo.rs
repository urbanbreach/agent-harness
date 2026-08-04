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
    before: EditorSnapshot,
    after: EditorSnapshot,
    group: EditGroup,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UndoStack {
    undo: Vec<UndoEntry>,
    redo: Vec<UndoEntry>,
}

impl UndoStack {
    pub fn record(&mut self, before: EditorSnapshot, after: EditorSnapshot, group: EditGroup) {
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
        let entry = self.undo.pop()?;
        self.redo.push(UndoEntry {
            before: current.clone(),
            after: entry.after,
            group: entry.group,
        });
        Some(entry.before)
    }

    pub fn redo(&mut self, current: &EditorSnapshot) -> Option<EditorSnapshot> {
        let entry = self.redo.pop()?;
        self.undo.push(UndoEntry {
            before: current.clone(),
            after: entry.after.clone(),
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
        let Some(snapshot) = self.undo.undo(&current) else {
            return false;
        };
        self.restore(snapshot);
        true
    }

    pub fn redo(&mut self) -> bool {
        let current = self.snapshot();
        let Some(snapshot) = self.undo.redo(&current) else {
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
