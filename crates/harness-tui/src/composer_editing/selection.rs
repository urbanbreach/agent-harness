use std::fmt::{Display, Formatter};

use crate::composer_atoms::{AtomBuffer, AtomCursor};

use super::{ComposerEditor, EditingError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    anchor: AtomCursor,
    active: AtomCursor,
}

impl Selection {
    pub const fn new(anchor: AtomCursor, active: AtomCursor) -> Self {
        Self { anchor, active }
    }

    pub const fn anchor(self) -> AtomCursor {
        self.anchor
    }

    pub const fn active(self) -> AtomCursor {
        self.active
    }

    pub fn start(self) -> AtomCursor {
        let anchor = self.anchor.insertion_index();
        let active = self.active.insertion_index();
        AtomCursor::before(anchor.min(active))
    }

    pub fn end(self) -> AtomCursor {
        let anchor = self.anchor.insertion_index();
        let active = self.active.insertion_index();
        AtomCursor::before(anchor.max(active))
    }

    pub fn is_empty(self) -> bool {
        self.anchor.insertion_index() == self.active.insertion_index()
    }
}

pub type VisualSelection = Selection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MousePoint {
    pub row: usize,
    pub column: u16,
}

impl MousePoint {
    pub const fn new(row: usize, column: u16) -> Self {
        Self { row, column }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionError {
    RowOutOfBounds { row: usize },
}

impl Display for SelectionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RowOutOfBounds { row } => write!(formatter, "visual row {row} is out of bounds"),
        }
    }
}

impl std::error::Error for SelectionError {}

pub fn cursor_at_point(
    buffer: &AtomBuffer,
    point: MousePoint,
    width: u16,
) -> Result<AtomCursor, SelectionError> {
    let lines = buffer.wrap(width.max(1));
    let line = lines
        .get(point.row)
        .ok_or(SelectionError::RowOutOfBounds { row: point.row })?;
    let mut used = 0u16;
    for atom_id in &line.atom_ids {
        let Some((index, atom)) = buffer
            .atoms()
            .iter()
            .enumerate()
            .find(|(_, atom)| atom.id == *atom_id)
        else {
            continue;
        };
        if point.column <= used {
            return Ok(AtomCursor::before(index));
        }
        let next = used.saturating_add(atom.display_width);
        if atom.display_width > 0 && point.column < next {
            return Ok(AtomCursor::before(index));
        }
        used = next;
    }
    line.atom_ids
        .last()
        .and_then(|last| buffer.atoms().iter().position(|atom| atom.id == *last))
        .map(|index| AtomCursor::before(index.saturating_add(1)))
        .or_else(|| Some(AtomCursor::before(0)))
        .ok_or(SelectionError::RowOutOfBounds { row: point.row })
}

impl ComposerEditor {
    pub fn select_char_left(&mut self) {
        self.extend_selection(super::movement::move_left(&self.buffer, self.cursor));
    }

    pub fn select_char_right(&mut self) {
        self.extend_selection(super::movement::move_right(&self.buffer, self.cursor));
    }

    pub fn select_line(&mut self) {
        let start = super::movement::move_line_start(&self.buffer, self.cursor);
        let end = super::movement::move_line_end(&self.buffer, self.cursor);
        self.selection = Some(Selection::new(start, end));
        self.cursor = end;
    }

    pub fn select_all(&mut self) {
        let end = super::movement::move_buffer_end(&self.buffer);
        self.selection = Some(Selection::new(AtomCursor::start(), end));
        self.cursor = end;
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    pub fn begin_mouse_selection(
        &mut self,
        point: MousePoint,
        width: u16,
    ) -> Result<(), EditingError> {
        let cursor = cursor_at_point(&self.buffer, point, width)?;
        self.mouse_anchor = Some(cursor);
        self.selection = Some(Selection::new(cursor, cursor));
        self.cursor = cursor;
        Ok(())
    }

    pub fn update_mouse_selection(
        &mut self,
        point: MousePoint,
        width: u16,
    ) -> Result<(), EditingError> {
        let cursor = cursor_at_point(&self.buffer, point, width)?;
        let anchor = self.mouse_anchor.unwrap_or(self.cursor);
        self.selection = Some(Selection::new(anchor, cursor));
        self.cursor = cursor;
        Ok(())
    }
}
