mod deletion;
mod history;
mod movement;
mod paste;
mod selection;
mod undo;

pub use deletion::DeleteKind;
pub use history::PromptHistory;
pub use selection::{MousePoint, Selection, SelectionError, VisualSelection};
pub use undo::{EditGroup, EditorSnapshot, UndoStack};

use std::fmt::{Display, Formatter};

use crate::composer_atoms::{AtomBuffer, AtomBufferError, AtomCursor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditingError {
    Buffer(AtomBufferError),
    Selection(SelectionError),
}

impl Display for EditingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Buffer(error) => Display::fmt(error, formatter),
            Self::Selection(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for EditingError {}

impl From<AtomBufferError> for EditingError {
    fn from(error: AtomBufferError) -> Self {
        Self::Buffer(error)
    }
}

impl From<SelectionError> for EditingError {
    fn from(error: SelectionError) -> Self {
        Self::Selection(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorState {
    pub buffer: AtomBuffer,
    pub cursor: AtomCursor,
    pub selection: Option<Selection>,
    pub history: PromptHistory,
    pub undo_depth: usize,
    pub redo_depth: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerEditor {
    buffer: AtomBuffer,
    cursor: AtomCursor,
    selection: Option<Selection>,
    undo: UndoStack,
    history: PromptHistory,
    mouse_anchor: Option<AtomCursor>,
}

impl ComposerEditor {
    pub fn new() -> Self {
        Self::from_buffer(AtomBuffer::new())
    }

    pub fn from_text(text: &str) -> Self {
        Self::from_buffer(AtomBuffer::from_text(text))
    }

    pub fn from_buffer(buffer: AtomBuffer) -> Self {
        let cursor = AtomCursor::before(buffer.atoms().len());
        Self {
            buffer,
            cursor,
            selection: None,
            undo: UndoStack::default(),
            history: PromptHistory::new(Vec::new()),
            mouse_anchor: None,
        }
    }

    pub fn buffer(&self) -> &AtomBuffer {
        &self.buffer
    }

    pub fn text(&self) -> String {
        self.buffer.text()
    }

    pub const fn cursor(&self) -> AtomCursor {
        self.cursor
    }

    pub fn selection(&self) -> Option<Selection> {
        self.selection.filter(|selection| !selection.is_empty())
    }

    pub fn state(&self) -> EditorState {
        EditorState {
            buffer: self.buffer.clone(),
            cursor: self.cursor,
            selection: self.selection(),
            history: self.history.clone(),
            undo_depth: self.undo.undo_depth(),
            redo_depth: self.undo.redo_depth(),
        }
    }

    fn snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            buffer: self.buffer.clone(),
            cursor: self.cursor,
            selection: self.selection(),
            history: self.history.clone(),
        }
    }

    fn restore(&mut self, snapshot: EditorSnapshot) {
        self.buffer = snapshot.buffer;
        self.cursor = snapshot.cursor;
        self.selection = snapshot.selection;
        self.history = snapshot.history;
        self.mouse_anchor = None;
    }

    fn record(&mut self, before: EditorSnapshot, group: EditGroup) {
        self.undo.record(before, self.snapshot(), group);
    }
}

impl Default for ComposerEditor {
    fn default() -> Self {
        Self::new()
    }
}
