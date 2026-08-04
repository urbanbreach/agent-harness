use crate::composer_atoms::{AtomCursor, AtomKind};
use crate::composer_editing::ComposerEditor;

use super::{CompletionError, CompletionTrigger};

/// Replaces only complete text atoms and returns a new editor snapshot.
pub fn insert_completion(
    editor: &ComposerEditor,
    trigger: &CompletionTrigger,
    replacement: &str,
) -> Result<ComposerEditor, CompletionError> {
    let buffer = editor.buffer();
    let range = trigger.range;
    if range.start > range.end || range.end > buffer.atoms().len() {
        return Err(CompletionError::InvalidRange {
            start: range.start,
            end: range.end,
        });
    }
    for atom in buffer.atoms().iter().skip(range.start).take(range.len()) {
        if !matches!(atom.kind, AtomKind::Text(_)) {
            return Err(CompletionError::ProtectedAtom(atom.id));
        }
    }
    let mut next = buffer.clone();
    next.delete_range(
        AtomCursor::before(range.start),
        AtomCursor::before(range.end),
    )?;
    next.insert_text_at(AtomCursor::before(range.start), replacement)?;
    Ok(ComposerEditor::from_buffer(next))
}
