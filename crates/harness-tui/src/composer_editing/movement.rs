use crate::composer_atoms::{AtomBuffer, AtomCursor, AtomKind};

use super::{ComposerEditor, Selection};

pub fn move_left(_buffer: &AtomBuffer, cursor: AtomCursor) -> AtomCursor {
    AtomCursor::before(cursor.insertion_index().saturating_sub(1))
}

pub fn move_right(buffer: &AtomBuffer, cursor: AtomCursor) -> AtomCursor {
    AtomCursor::before(
        cursor
            .insertion_index()
            .saturating_add(1)
            .min(buffer.atoms().len()),
    )
}

pub fn move_word_left(buffer: &AtomBuffer, cursor: AtomCursor) -> AtomCursor {
    let atoms = buffer.atoms();
    let mut index = cursor.insertion_index().min(atoms.len());
    while index > 0 && !is_word_atom(&atoms[index - 1].kind) {
        index -= 1;
    }
    while index > 0 && is_word_atom(&atoms[index - 1].kind) {
        index -= 1;
    }
    AtomCursor::before(index)
}

pub fn move_word_right(buffer: &AtomBuffer, cursor: AtomCursor) -> AtomCursor {
    let atoms = buffer.atoms();
    let mut index = cursor.insertion_index().min(atoms.len());
    while index < atoms.len() && is_word_atom(&atoms[index].kind) {
        index += 1;
    }
    while index < atoms.len() && !is_word_atom(&atoms[index].kind) {
        index += 1;
    }
    AtomCursor::before(index)
}

pub fn move_line_start(buffer: &AtomBuffer, cursor: AtomCursor) -> AtomCursor {
    let atoms = buffer.atoms();
    let mut index = cursor.insertion_index().min(atoms.len());
    while index > 0 && !matches!(atoms[index - 1].kind, AtomKind::Newline) {
        index -= 1;
    }
    AtomCursor::before(index)
}

pub fn move_line_end(buffer: &AtomBuffer, cursor: AtomCursor) -> AtomCursor {
    let atoms = buffer.atoms();
    let mut index = cursor.insertion_index().min(atoms.len());
    while index < atoms.len() && !matches!(atoms[index].kind, AtomKind::Newline) {
        index += 1;
    }
    AtomCursor::before(index)
}

pub const fn move_buffer_start() -> AtomCursor {
    AtomCursor::start()
}

pub fn move_buffer_end(buffer: &AtomBuffer) -> AtomCursor {
    AtomCursor::before(buffer.atoms().len())
}

pub fn is_word_atom(kind: &AtomKind) -> bool {
    match kind {
        AtomKind::Text(cluster) => cluster.as_str().chars().all(char::is_alphanumeric),
        AtomKind::Newline | AtomKind::FileMention(_) | AtomKind::Attachment(_) => false,
    }
}

impl ComposerEditor {
    pub fn move_left(&mut self) {
        self.cursor = move_left(&self.buffer, self.cursor);
        self.selection = None;
    }

    pub fn move_right(&mut self) {
        self.cursor = move_right(&self.buffer, self.cursor);
        self.selection = None;
    }

    pub fn move_word_left(&mut self) {
        self.cursor = move_word_left(&self.buffer, self.cursor);
        self.selection = None;
    }

    pub fn move_word_right(&mut self) {
        self.cursor = move_word_right(&self.buffer, self.cursor);
        self.selection = None;
    }

    pub fn move_line_start(&mut self) {
        self.cursor = move_line_start(&self.buffer, self.cursor);
        self.selection = None;
    }

    pub fn move_line_end(&mut self) {
        self.cursor = move_line_end(&self.buffer, self.cursor);
        self.selection = None;
    }

    pub const fn move_buffer_start(&mut self) {
        self.cursor = move_buffer_start();
        self.selection = None;
    }

    pub fn move_buffer_end(&mut self) {
        self.cursor = move_buffer_end(&self.buffer);
        self.selection = None;
    }

    pub fn select_word_left(&mut self) {
        self.extend_selection(move_word_left(&self.buffer, self.cursor));
    }

    pub fn select_word_right(&mut self) {
        self.extend_selection(move_word_right(&self.buffer, self.cursor));
    }

    pub(super) fn extend_selection(&mut self, active: AtomCursor) {
        let anchor = self.selection.map_or(self.cursor, Selection::anchor);
        self.selection = Some(Selection::new(anchor, active));
        self.cursor = active;
    }
}
