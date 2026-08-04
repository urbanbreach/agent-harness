use serde::{Deserialize, Serialize};

use super::atom::{AtomId, AtomKind, ComposerAtom};
use super::cursor::{AtomBoundary, AtomCursor};
use super::grapheme::split_graphemes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AtomBufferError {
    DuplicateAtomId(AtomId),
    CursorOutOfBounds(AtomCursor),
    ReversedRange,
}

impl std::fmt::Display for AtomBufferError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateAtomId(id) => write!(formatter, "duplicate atom id {}", id.get()),
            Self::CursorOutOfBounds(cursor) => {
                write!(formatter, "cursor {cursor} is outside the atom buffer")
            }
            Self::ReversedRange => formatter.write_str("atom range is reversed"),
        }
    }
}

impl std::error::Error for AtomBufferError {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtomBuffer {
    pub atoms: Vec<ComposerAtom>,
    next_atom_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedLine {
    pub atom_ids: Vec<AtomId>,
    pub display_width: u16,
}

impl AtomBuffer {
    pub fn new() -> Self {
        Self {
            atoms: Vec::new(),
            next_atom_id: 1,
        }
    }

    pub fn from_text(text: &str) -> Self {
        let mut buffer = Self::new();
        let mut pending = String::new();
        for character in text.chars() {
            if character == '\n' {
                buffer.append_text(&pending);
                pending.clear();
                let id = buffer.allocate_id();
                buffer.atoms.push(ComposerAtom::newline(id));
            } else {
                pending.push(character);
            }
        }
        buffer.append_text(&pending);
        buffer
    }

    pub fn from_atoms(atoms: Vec<ComposerAtom>) -> Result<Self, AtomBufferError> {
        let next_atom_id = atoms.iter().try_fold(1, |next, atom| {
            if atoms
                .iter()
                .filter(|candidate| candidate.id == atom.id)
                .count()
                > 1
            {
                Err(AtomBufferError::DuplicateAtomId(atom.id))
            } else {
                Ok(next.max(atom.id.get().saturating_add(1)))
            }
        })?;
        Ok(Self {
            atoms,
            next_atom_id,
        })
    }

    pub fn atoms(&self) -> &[ComposerAtom] {
        &self.atoms
    }

    pub fn text(&self) -> String {
        self.atoms
            .iter()
            .map(|atom| match &atom.kind {
                AtomKind::Text(cluster) => cluster.as_str().to_owned(),
                AtomKind::Newline => "\n".to_owned(),
                AtomKind::FileMention(id) => format!("@mention:{}", id.get()),
                AtomKind::Attachment(id) => format!("[attachment:{}]", id.get()),
            })
            .collect()
    }

    pub fn insert_text_at(
        &mut self,
        cursor: AtomCursor,
        text: &str,
    ) -> Result<AtomCursor, AtomBufferError> {
        let insertion_index = self.validate_cursor(cursor)?;
        let mut inserted = Vec::new();
        let mut pending = String::new();
        for character in text.chars() {
            if character == '\n' {
                inserted.extend(
                    split_graphemes(&pending)
                        .into_iter()
                        .map(|cluster| ComposerAtom::text(self.allocate_id(), cluster)),
                );
                pending.clear();
                inserted.push(ComposerAtom::newline(self.allocate_id()));
            } else {
                pending.push(character);
            }
        }
        inserted.extend(
            split_graphemes(&pending)
                .into_iter()
                .map(|cluster| ComposerAtom::text(self.allocate_id(), cluster)),
        );
        let inserted_len = inserted.len();
        self.atoms
            .splice(insertion_index..insertion_index, inserted);
        Ok(AtomCursor::before(insertion_index + inserted_len))
    }

    pub fn delete_range(
        &mut self,
        start: AtomCursor,
        end: AtomCursor,
    ) -> Result<AtomCursor, AtomBufferError> {
        let start_index = self.validate_cursor(start)?;
        let end_index = self.validate_cursor(end)?;
        if start_index > end_index {
            return Err(AtomBufferError::ReversedRange);
        }
        self.atoms.drain(start_index..end_index);
        Ok(AtomCursor::before(start_index.min(self.atoms.len())))
    }

    pub fn wrap(&self, width: u16) -> Vec<WrappedLine> {
        let mut lines = Vec::new();
        let mut current = WrappedLine {
            atom_ids: Vec::new(),
            display_width: 0,
        };
        for atom in &self.atoms {
            if matches!(atom.kind, AtomKind::Newline) {
                current.atom_ids.push(atom.id);
                lines.push(current);
                current = WrappedLine {
                    atom_ids: Vec::new(),
                    display_width: 0,
                };
            } else if current.display_width > 0
                && current.display_width.saturating_add(atom.display_width) > width
            {
                lines.push(current);
                current = WrappedLine {
                    atom_ids: vec![atom.id],
                    display_width: atom.display_width,
                };
            } else {
                current.atom_ids.push(atom.id);
                current.display_width = current.display_width.saturating_add(atom.display_width);
            }
        }
        if !current.atom_ids.is_empty() || lines.is_empty() {
            lines.push(current);
        }
        lines
    }

    fn append_text(&mut self, text: &str) {
        for cluster in split_graphemes(text) {
            let id = self.allocate_id();
            self.atoms.push(ComposerAtom::text(id, cluster));
        }
    }

    fn allocate_id(&mut self) -> u64 {
        let id = self.next_atom_id;
        self.next_atom_id = self.next_atom_id.saturating_add(1);
        id
    }

    fn validate_cursor(&self, cursor: AtomCursor) -> Result<usize, AtomBufferError> {
        let insertion_index = cursor.insertion_index();
        let valid = match cursor.boundary {
            AtomBoundary::Before => cursor.atom_index <= self.atoms.len(),
            AtomBoundary::After => cursor.atom_index < self.atoms.len(),
        };
        if valid && insertion_index <= self.atoms.len() {
            Ok(insertion_index)
        } else {
            Err(AtomBufferError::CursorOutOfBounds(cursor))
        }
    }
}

impl Default for AtomBuffer {
    fn default() -> Self {
        Self::new()
    }
}
