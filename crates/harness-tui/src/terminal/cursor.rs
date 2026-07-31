//! Cursor control leaf types: position, visibility, and shape.
//!
//! Pure value objects mirroring the deterministic cursor rows the terminal
//! lifecycle must own: a 0-based grid position, a visibility toggle, and a
//! cursor shape. No terminal I/O — these project intent only.

/// Visual cursor shape (DECSCUSR family). Visibility is tracked separately on
/// [`CursorState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    /// Terminal default shape.
    #[default]
    Default,
    /// Block cursor.
    Block,
    /// Vertical bar (beam) cursor.
    Line,
    /// Underline cursor.
    Underline,
}

/// A 0-based cursor cell coordinate, matching the terminal
/// `MoveTo(column, row)` convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CursorPosition {
    /// 0-based column (x).
    pub column: u16,
    /// 0-based row (y).
    pub row: u16,
}

impl CursorPosition {
    /// A cursor position from a 0-based column and row.
    pub const fn new(column: u16, row: u16) -> Self {
        Self { column, row }
    }

    /// Clamp to the half-open grid `[0, columns) x [0, rows)`. A zero dimension
    /// collapses its axis to 0.
    pub fn clamped(self, columns: u16, rows: u16) -> Self {
        Self {
            column: self.column.min(columns.saturating_sub(1)),
            row: self.row.min(rows.saturating_sub(1)),
        }
    }
}

/// Full cursor projection: position + visibility + shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorState {
    /// Current cell position.
    pub position: CursorPosition,
    /// Whether the cursor is shown.
    pub visible: bool,
    /// Current shape.
    pub shape: CursorShape,
}

impl CursorState {
    /// A default cursor: home position, visible, terminal default shape.
    pub const fn new() -> Self {
        Self {
            position: CursorPosition::new(0, 0),
            visible: true,
            shape: CursorShape::Default,
        }
    }

    /// Move to an exact position (no clamping).
    pub const fn move_to(self, position: CursorPosition) -> Self {
        Self { position, ..self }
    }

    /// Move to a position clamped to a `(columns x rows)` grid.
    pub fn move_to_clamped(self, position: CursorPosition, columns: u16, rows: u16) -> Self {
        let position = position.clamped(columns, rows);
        Self { position, ..self }
    }

    /// Show the cursor.
    pub const fn show(self) -> Self {
        Self {
            visible: true,
            ..self
        }
    }

    /// Hide the cursor.
    pub const fn hide(self) -> Self {
        Self {
            visible: false,
            ..self
        }
    }

    /// Whether the cursor is shown.
    pub const fn is_visible(self) -> bool {
        self.visible
    }

    /// Set the cursor shape.
    pub const fn with_shape(self, shape: CursorShape) -> Self {
        Self { shape, ..self }
    }
}

impl Default for CursorState {
    fn default() -> Self {
        Self::new()
    }
}
