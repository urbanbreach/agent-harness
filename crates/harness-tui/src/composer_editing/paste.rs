use crate::composer_atoms::{AtomBuffer, AtomBufferError, AtomCursor, AttachmentId, ComposerAtom};

use super::{ComposerEditor, EditGroup, EditingError};

pub fn normalize_paste(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

pub fn insert_text(
    buffer: &mut AtomBuffer,
    cursor: AtomCursor,
    text: &str,
) -> Result<AtomCursor, AtomBufferError> {
    buffer.insert_text_at(cursor, &normalize_paste(text))
}

pub fn insert_attachment(
    buffer: &mut AtomBuffer,
    cursor: AtomCursor,
    attachment: AttachmentId,
) -> Result<AtomCursor, AtomBufferError> {
    let insertion_index = buffer.insert_text_at(cursor, "")?.insertion_index();
    let next_id = buffer
        .atoms()
        .iter()
        .map(|atom| atom.id.get())
        .max()
        .unwrap_or(0)
        .saturating_add(1);
    let mut atoms = buffer.atoms().to_vec();
    atoms.insert(
        insertion_index,
        ComposerAtom::attachment(next_id, attachment),
    );
    *buffer = AtomBuffer::from_atoms(atoms).map_err(|error| match error {
        AtomBufferError::DuplicateAtomId(id) => AtomBufferError::DuplicateAtomId(id),
        AtomBufferError::CursorOutOfBounds(cursor) => AtomBufferError::CursorOutOfBounds(cursor),
        AtomBufferError::ReversedRange => AtomBufferError::ReversedRange,
    })?;
    Ok(AtomCursor::before(insertion_index + 1))
}

impl ComposerEditor {
    pub fn insert_text(&mut self, text: &str) -> Result<(), EditingError> {
        self.apply_insert(text, EditGroup::Paste)
    }

    pub fn paste(&mut self, text: &str) -> Result<(), EditingError> {
        self.apply_insert(text, EditGroup::Paste)
    }

    pub fn insert_attachment(&mut self, attachment: AttachmentId) -> Result<(), EditingError> {
        let before = self.snapshot();
        self.delete_selection()?;
        self.cursor = insert_attachment(&mut self.buffer, self.cursor, attachment)?;
        self.selection = None;
        self.record(before, EditGroup::AttachmentInsertion);
        Ok(())
    }

    fn apply_insert(&mut self, text: &str, group: EditGroup) -> Result<(), EditingError> {
        let before = self.snapshot();
        self.delete_selection()?;
        self.cursor = insert_text(&mut self.buffer, self.cursor, text)?;
        self.selection = None;
        self.record(before, group);
        Ok(())
    }
}
