use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt::Write;

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
pub(crate) struct AtomBufferPatch {
    start: usize,
    removed: usize,
    inserted: Vec<ComposerAtom>,
    next_atom_id: u64,
}

impl AtomBufferPatch {
    pub(crate) fn apply(&self, buffer: &mut AtomBuffer) {
        buffer.atoms.splice(
            self.start..self.start + self.removed,
            self.inserted.iter().cloned(),
        );
        buffer.next_atom_id = self.next_atom_id;
    }
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
        buffer.atoms = buffer.parse_text(text);
        buffer
    }

    pub fn from_atoms(atoms: Vec<ComposerAtom>) -> Result<Self, AtomBufferError> {
        let mut ids = HashSet::with_capacity(atoms.len());
        let mut duplicate = None;
        let mut next_atom_id = 1;
        // Scan backward so the error names the first conflicting atom in input order.
        for atom in atoms.iter().rev() {
            if !ids.insert(atom.id) {
                duplicate = Some(atom.id);
            }
            next_atom_id = next_atom_id.max(atom.id.get().saturating_add(1));
        }
        if let Some(id) = duplicate {
            return Err(AtomBufferError::DuplicateAtomId(id));
        }
        Ok(Self {
            atoms,
            next_atom_id,
        })
    }

    pub(crate) fn patch_to(&self, target: &Self) -> AtomBufferPatch {
        let start = self
            .atoms
            .iter()
            .zip(&target.atoms)
            .take_while(|(a, b)| a == b)
            .count();
        let suffix = self.atoms[start..]
            .iter()
            .rev()
            .zip(target.atoms[start..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        AtomBufferPatch {
            start,
            removed: self.atoms.len() - start - suffix,
            inserted: target.atoms[start..target.atoms.len() - suffix].to_vec(),
            next_atom_id: target.next_atom_id,
        }
    }

    pub fn atoms(&self) -> &[ComposerAtom] {
        &self.atoms
    }

    pub fn text(&self) -> String {
        let mut text = String::new();
        for atom in &self.atoms {
            match &atom.kind {
                AtomKind::Text(cluster) => text.push_str(cluster.as_str()),
                AtomKind::Newline => text.push('\n'),
                AtomKind::FileMention(id) => {
                    let _ = write!(text, "@mention:{}", id.get());
                }
                AtomKind::Attachment(id) => {
                    let _ = write!(text, "[attachment:{}]", id.get());
                }
            }
        }
        text
    }

    pub fn insert_text_at(
        &mut self,
        cursor: AtomCursor,
        text: &str,
    ) -> Result<AtomCursor, AtomBufferError> {
        let insertion_index = self.validate_cursor(cursor)?;
        let inserted = self.parse_text(text);
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
        lines.push(current);
        lines
    }

    fn parse_text(&mut self, text: &str) -> Vec<ComposerAtom> {
        let mut atoms = Vec::new();
        for (line_index, line) in text.split('\n').enumerate() {
            if line_index > 0 {
                atoms.push(ComposerAtom::newline(self.allocate_id()));
            }
            atoms.extend(
                split_graphemes(line)
                    .map(|cluster| ComposerAtom::text(self.allocate_id(), cluster)),
            );
        }
        atoms
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
