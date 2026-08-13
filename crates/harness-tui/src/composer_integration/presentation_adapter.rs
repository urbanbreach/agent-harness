use crate::composer_atoms::{AtomBuffer, AtomCursor, AtomKind};
use crate::composer_editing::Selection;

use super::{ComposerEditorModel, ComposerPresentationError};

pub(super) struct TextEditorState {
    pub(super) buffer: AtomBuffer,
    pub(super) cursor: AtomCursor,
    pub(super) selection: Option<Selection>,
}

pub(super) fn text_editor_state(
    text: &str,
    cursor_char_index: usize,
    selection_anchor: Option<usize>,
) -> TextEditorState {
    let buffer = AtomBuffer::from_text(text);
    let cursor = cursor_for_char_index(&buffer, cursor_char_index);
    let selection = selection_anchor
        .map(|anchor| Selection::new(cursor_for_char_index(&buffer, anchor), cursor))
        .filter(|selection| !selection.is_empty());
    TextEditorState {
        buffer,
        cursor,
        selection,
    }
}

fn cursor_for_char_index(buffer: &AtomBuffer, char_index: usize) -> AtomCursor {
    let mut consumed = 0usize;
    for (index, atom) in buffer.atoms().iter().enumerate() {
        let atom_chars = match &atom.kind {
            AtomKind::Text(cluster) => cluster.as_str().chars().count(),
            AtomKind::Newline | AtomKind::FileMention(_) | AtomKind::Attachment(_) => 1,
        };
        if consumed.saturating_add(atom_chars) > char_index {
            return AtomCursor::before(index);
        }
        consumed = consumed.saturating_add(atom_chars);
    }
    AtomCursor::before(buffer.atoms().len())
}

impl ComposerEditorModel {
    pub fn legacy_mirror_adapter(
        text: &str,
        cursor_char_index: usize,
        selection_anchor: Option<usize>,
        wrap_width: u16,
        max_viewport_lines: usize,
    ) -> Result<Self, ComposerPresentationError> {
        if wrap_width == 0 {
            return Err(ComposerPresentationError::ZeroWrapWidth);
        }
        if max_viewport_lines == 0 {
            return Err(ComposerPresentationError::ZeroViewportLines);
        }
        let state = text_editor_state(text, cursor_char_index, selection_anchor);
        let wrapped_lines = state.buffer.wrap(wrap_width);
        let viewport_rows = wrapped_lines.len().min(max_viewport_lines).max(1);
        Ok(Self {
            text: text.to_owned(),
            atoms: state.buffer.atoms().to_vec(),
            cursor: state.cursor,
            selection: state.selection,
            wrapped_lines,
            viewport_rows,
        })
    }
}
