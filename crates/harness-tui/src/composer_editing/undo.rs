use std::sync::Arc;

use crate::composer_atoms::{AtomBuffer, AtomBufferPatch, AtomCursor};

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
struct SnapshotPatch {
    buffer: Option<AtomBufferPatch>,
    cursor: AtomCursor,
    selection: Option<Selection>,
    history: Option<PromptHistory>,
}

impl SnapshotPatch {
    fn between(from: &EditorSnapshot, to: &EditorSnapshot) -> Option<Self> {
        let buffer = (from.buffer != to.buffer).then(|| from.buffer.patch_to(&to.buffer));
        let history = (from.history != to.history).then(|| to.history.clone());
        if buffer.is_none()
            && history.is_none()
            && from.cursor == to.cursor
            && from.selection == to.selection
        {
            return None;
        }
        Some(Self {
            buffer,
            cursor: to.cursor,
            selection: to.selection,
            history,
        })
    }

    fn restore(patch: Option<&Self>, mut snapshot: Arc<EditorSnapshot>) -> Arc<EditorSnapshot> {
        let Some(patch) = patch else {
            return snapshot;
        };
        let state = Arc::make_mut(&mut snapshot);
        if let Some(buffer) = &patch.buffer {
            buffer.apply(&mut state.buffer);
        }
        state.cursor = patch.cursor;
        state.selection = patch.selection;
        if let Some(history) = &patch.history {
            state.history.clone_from(history);
        }
        snapshot
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UndoEntry {
    before: Option<SnapshotPatch>,
    previous: Option<SnapshotPatch>,
    group: EditGroup,
}

// One complete tip anchors the patches. Bridges preserve changes made between
// recorded edits, including cursor, selection, history and atom allocator state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Journal {
    entries: Vec<Arc<UndoEntry>>,
    tip: Option<Arc<EditorSnapshot>>,
}

impl Journal {
    fn push(&mut self, before: Arc<EditorSnapshot>, after: &EditorSnapshot, group: EditGroup) {
        let entry = UndoEntry {
            before: SnapshotPatch::between(after, &before),
            previous: self
                .tip
                .as_ref()
                .and_then(|tip| SnapshotPatch::between(&before, tip)),
            group,
        };
        let advance = SnapshotPatch::between(&before, after);
        self.tip = None;
        self.tip = Some(SnapshotPatch::restore(advance.as_ref(), before));
        self.entries.push(Arc::new(entry));
    }

    fn pop(&mut self) -> Option<(Arc<EditorSnapshot>, Arc<EditorSnapshot>, EditGroup)> {
        let entry = self.entries.pop()?;
        let after = self.tip.take()?;
        let before = SnapshotPatch::restore(entry.before.as_ref(), Arc::clone(&after));
        if !self.entries.is_empty() {
            self.tip = Some(SnapshotPatch::restore(
                entry.previous.as_ref(),
                Arc::clone(&before),
            ));
        }
        Some((before, after, entry.group))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UndoStack {
    undo: Journal,
    redo: Journal,
}

impl UndoStack {
    pub fn record(&mut self, before: EditorSnapshot, after: EditorSnapshot, group: EditGroup) {
        self.record_shared(Arc::new(before), &after, group);
    }

    pub(super) fn latest(&self) -> Option<&Arc<EditorSnapshot>> {
        self.undo.tip.as_ref()
    }

    pub(super) fn record_shared(
        &mut self,
        mut before: Arc<EditorSnapshot>,
        after: &EditorSnapshot,
        group: EditGroup,
    ) {
        if before.as_ref() == after {
            return;
        }
        if group == EditGroup::CharacterDelete
            && self
                .undo
                .entries
                .last()
                .is_some_and(|entry| entry.group == group)
            && self.undo.tip.as_ref() == Some(&before)
            && let Some((start, _, _)) = self.undo.pop()
        {
            before = start;
        }
        self.undo.push(before, after, group);
        self.redo = Journal::default();
    }

    pub fn undo(&mut self, current: &EditorSnapshot) -> Option<EditorSnapshot> {
        self.undo.tip.as_ref()?;
        self.undo_shared(Arc::new(current.clone()))
            .map(Arc::unwrap_or_clone)
    }

    fn undo_shared(&mut self, current: Arc<EditorSnapshot>) -> Option<Arc<EditorSnapshot>> {
        let (before, after, group) = self.undo.pop()?;
        self.redo.push(current, &after, group);
        Some(before)
    }

    pub fn redo(&mut self, current: &EditorSnapshot) -> Option<EditorSnapshot> {
        self.redo.tip.as_ref()?;
        self.redo_shared(Arc::new(current.clone()))
            .map(Arc::unwrap_or_clone)
    }

    fn redo_shared(&mut self, current: Arc<EditorSnapshot>) -> Option<Arc<EditorSnapshot>> {
        let (_, after, group) = self.redo.pop()?;
        self.undo.push(current, &after, group);
        Some(after)
    }

    pub fn undo_depth(&self) -> usize {
        self.undo.entries.len()
    }

    pub fn redo_depth(&self) -> usize {
        self.redo.entries.len()
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
