use crate::composer_atoms::{AtomBuffer, AtomCursor, AtomKind};

use super::movement;
use super::{ComposerEditor, EditGroup, EditingError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteKind {
    CharacterBackward,
    CharacterForward,
    WordBackward,
    WordForward,
    Line,
}

pub fn delete_range(
    buffer: &mut AtomBuffer,
    cursor: AtomCursor,
    selection: Option<(AtomCursor, AtomCursor)>,
    kind: DeleteKind,
) -> Result<AtomCursor, crate::composer_atoms::AtomBufferError> {
    let (start, end) = selection
        .map(|(left, right)| {
            (
                left.insertion_index().min(right.insertion_index()),
                left.insertion_index().max(right.insertion_index()),
            )
        })
        .or_else(|| deletion_indices(buffer, cursor, kind))
        .unwrap_or((cursor.insertion_index(), cursor.insertion_index()));
    buffer.delete_range(AtomCursor::before(start), AtomCursor::before(end))
}

pub fn deletion_indices(
    buffer: &AtomBuffer,
    cursor: AtomCursor,
    kind: DeleteKind,
) -> Option<(usize, usize)> {
    let index = cursor.insertion_index().min(buffer.atoms().len());
    match kind {
        DeleteKind::CharacterBackward if index > 0 => Some((index - 1, index)),
        DeleteKind::CharacterForward if index < buffer.atoms().len() => Some((index, index + 1)),
        DeleteKind::WordBackward => {
            let start =
                movement::move_word_left(buffer, AtomCursor::before(index)).insertion_index();
            (start < index).then_some((start, index))
        }
        DeleteKind::WordForward => {
            let end =
                movement::move_word_right(buffer, AtomCursor::before(index)).insertion_index();
            (index < end).then_some((index, end))
        }
        DeleteKind::Line => {
            let start =
                movement::move_line_start(buffer, AtomCursor::before(index)).insertion_index();
            let mut end =
                movement::move_line_end(buffer, AtomCursor::before(index)).insertion_index();
            if matches!(
                buffer.atoms().get(end).map(|atom| &atom.kind),
                Some(AtomKind::Newline)
            ) {
                end += 1;
            }
            (start < end).then_some((start, end))
        }
        DeleteKind::CharacterBackward | DeleteKind::CharacterForward => None,
    }
}

impl ComposerEditor {
    pub fn backspace(&mut self) -> Result<(), EditingError> {
        self.delete(DeleteKind::CharacterBackward)
    }

    pub fn delete(&mut self, kind: DeleteKind) -> Result<(), EditingError> {
        let before = self.snapshot();
        let selected = self
            .selection
            .map(|selection| (selection.start(), selection.end()));
        self.cursor = delete_range(&mut self.buffer, self.cursor, selected, kind)?;
        self.selection = None;
        let group = match kind {
            DeleteKind::CharacterBackward | DeleteKind::CharacterForward => {
                EditGroup::CharacterDelete
            }
            DeleteKind::WordBackward | DeleteKind::WordForward => EditGroup::WordDelete,
            DeleteKind::Line => EditGroup::LineDelete,
        };
        self.record(before, group);
        Ok(())
    }

    pub(super) fn delete_selection(&mut self) -> Result<(), EditingError> {
        let Some(selection) = self.selection.filter(|selection| !selection.is_empty()) else {
            return Ok(());
        };
        self.cursor = self
            .buffer
            .delete_range(selection.start(), selection.end())?;
        self.selection = None;
        Ok(())
    }
}
