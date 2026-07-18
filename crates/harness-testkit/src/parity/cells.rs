//! Semantic cell grid types for TUI reference-parity L2 (A-CELLS).
//!
//! Fail-closed exact capture: grapheme, display width, continuation, resolved
//! colors, modifiers, cursor, and dimensions. No similarity scores.

use serde::{Deserialize, Serialize};

use super::frame_io::ParityCellError;

/// Schema version written into `cells.json` artifacts.
pub const SEMANTIC_FRAME_SCHEMA_VERSION: &str = "semantic-frame-v1";

/// Default terminal foreground when the emulator reports `Color::Default`.
/// Matches harness-tui visual_renderer baseline (not a pixel oracle).
pub const DEFAULT_FG: [u8; 3] = [216, 216, 216];

/// Default terminal background when the emulator reports `Color::Default`.
pub const DEFAULT_BG: [u8; 3] = [18, 18, 18];

/// Resolved RGB triple. Always concrete; never an unresolved "default" token.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResolvedRgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl ResolvedRgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const fn from_array(rgb: [u8; 3]) -> Self {
        Self {
            r: rgb[0],
            g: rgb[1],
            b: rgb[2],
        }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

/// Text style modifiers captured per cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CellModifiers {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
}

/// Cursor shape when known.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorShape {
    #[default]
    Block,
    Bar,
    Underline,
    Unknown,
}

/// Cursor position, visibility, and shape for a captured frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CursorState {
    pub row: u16,
    pub col: u16,
    pub visible: bool,
    pub shape: CursorShape,
}

impl CursorState {
    pub const fn hidden(row: u16, col: u16) -> Self {
        Self {
            row,
            col,
            visible: false,
            shape: CursorShape::Block,
        }
    }

    pub const fn visible_block(row: u16, col: u16) -> Self {
        Self {
            row,
            col,
            visible: true,
            shape: CursorShape::Block,
        }
    }
}

/// One terminal cell in the semantic grid.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SemanticCell {
    pub row: u16,
    pub col: u16,
    /// Grapheme cluster or empty string for blank / continuation cells.
    pub grapheme: String,
    /// Display width in columns (0 continuation, 1 narrow, 2 wide).
    pub width: u8,
    /// True when this cell is the trailing half of a wide glyph.
    pub continuation: bool,
    pub fg: ResolvedRgb,
    pub bg: ResolvedRgb,
    pub modifiers: CellModifiers,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hyperlink: Option<String>,
}

impl SemanticCell {
    pub fn blank(row: u16, col: u16) -> Self {
        Self {
            row,
            col,
            grapheme: String::new(),
            width: 1,
            continuation: false,
            fg: ResolvedRgb::from_array(DEFAULT_FG),
            bg: ResolvedRgb::from_array(DEFAULT_BG),
            modifiers: CellModifiers::default(),
            hyperlink: None,
        }
    }

    pub fn with_grapheme(mut self, grapheme: impl Into<String>, width: u8) -> Self {
        self.grapheme = grapheme.into();
        self.width = width;
        self
    }

    pub fn with_continuation(mut self) -> Self {
        self.continuation = true;
        self.width = 0;
        self.grapheme.clear();
        self
    }

    pub fn with_fg(mut self, fg: ResolvedRgb) -> Self {
        self.fg = fg;
        self
    }

    pub fn with_bg(mut self, bg: ResolvedRgb) -> Self {
        self.bg = bg;
        self
    }

    pub fn with_modifiers(mut self, modifiers: CellModifiers) -> Self {
        self.modifiers = modifiers;
        self
    }
}

/// Full-frame semantic capture for L2 comparison.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticFrame {
    pub schema_version: String,
    pub cols: u16,
    pub rows: u16,
    pub cursor: CursorState,
    pub alternate_screen: bool,
    /// Dense row-major cells: `len == cols * rows`.
    pub cells: Vec<SemanticCell>,
}

impl SemanticFrame {
    pub fn new(cols: u16, rows: u16, cursor: CursorState) -> Self {
        let mut cells = Vec::with_capacity(usize::from(cols) * usize::from(rows));
        for row in 0..rows {
            for col in 0..cols {
                cells.push(SemanticCell::blank(row, col));
            }
        }
        Self {
            schema_version: SEMANTIC_FRAME_SCHEMA_VERSION.to_owned(),
            cols,
            rows,
            cursor,
            alternate_screen: false,
            cells,
        }
    }

    pub fn cell_index(&self, row: u16, col: u16) -> Option<usize> {
        if row >= self.rows || col >= self.cols {
            return None;
        }
        Some(usize::from(row) * usize::from(self.cols) + usize::from(col))
    }

    pub fn cell(&self, row: u16, col: u16) -> Option<&SemanticCell> {
        self.cell_index(row, col).map(|idx| &self.cells[idx])
    }

    pub fn cell_mut(&mut self, row: u16, col: u16) -> Option<&mut SemanticCell> {
        self.cell_index(row, col).map(|idx| &mut self.cells[idx])
    }

    pub fn set_cell(&mut self, cell: SemanticCell) -> Result<(), ParityCellError> {
        let idx = self
            .cell_index(cell.row, cell.col)
            .ok_or(ParityCellError::OutOfBounds {
                row: cell.row,
                col: cell.col,
                rows: self.rows,
                cols: self.cols,
            })?;
        self.cells[idx] = cell;
        Ok(())
    }

    /// Validate dense grid invariants before compare/serialize.
    pub fn validate(&self) -> Result<(), ParityCellError> {
        if self.schema_version != SEMANTIC_FRAME_SCHEMA_VERSION {
            return Err(ParityCellError::SchemaVersion {
                expected: SEMANTIC_FRAME_SCHEMA_VERSION.to_owned(),
                observed: self.schema_version.clone(),
            });
        }
        let expected_len = usize::from(self.cols) * usize::from(self.rows);
        if self.cells.len() != expected_len {
            return Err(ParityCellError::CellCount {
                expected: expected_len,
                observed: self.cells.len(),
            });
        }
        for (idx, cell) in self.cells.iter().enumerate() {
            let expected_row =
                u16::try_from(idx / usize::from(self.cols.max(1))).unwrap_or(u16::MAX);
            let expected_col =
                u16::try_from(idx % usize::from(self.cols.max(1))).unwrap_or(u16::MAX);
            if cell.row != expected_row || cell.col != expected_col {
                return Err(ParityCellError::CellPosition {
                    index: idx,
                    expected_row,
                    expected_col,
                    observed_row: cell.row,
                    observed_col: cell.col,
                });
            }
            if cell.continuation && cell.width != 0 {
                return Err(ParityCellError::ContinuationWidth {
                    row: cell.row,
                    col: cell.col,
                    width: cell.width,
                });
            }
        }
        Ok(())
    }
}
