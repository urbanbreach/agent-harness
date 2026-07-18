//! Serialize SemanticFrame to cells.json / cells.txt and related errors.

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use serde_json::Value;

use super::cells::{
    CellModifiers, ResolvedRgb, SemanticCell, SemanticFrame, DEFAULT_BG, DEFAULT_FG,
};

/// Capture errors for semantic cell construction and I/O.
#[derive(Debug)]
pub enum ParityCellError {
    OutOfBounds {
        row: u16,
        col: u16,
        rows: u16,
        cols: u16,
    },
    SchemaVersion {
        expected: String,
        observed: String,
    },
    CellCount {
        expected: usize,
        observed: usize,
    },
    CellPosition {
        index: usize,
        expected_row: u16,
        expected_col: u16,
        observed_row: u16,
        observed_col: u16,
    },
    ContinuationWidth {
        row: u16,
        col: u16,
        width: u8,
    },
    Serialize(String),
    Deserialize(String),
    Io(io::Error),
}

impl fmt::Display for ParityCellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfBounds {
                row,
                col,
                rows,
                cols,
            } => write!(f, "cell ({row},{col}) out of bounds for {cols}x{rows}"),
            Self::SchemaVersion { expected, observed } => {
                write!(f, "schema_version expected {expected}, observed {observed}")
            }
            Self::CellCount { expected, observed } => {
                write!(f, "cell count expected {expected}, observed {observed}")
            }
            Self::CellPosition {
                index,
                expected_row,
                expected_col,
                observed_row,
                observed_col,
            } => write!(
                f,
                "cell index {index} expected ({expected_row},{expected_col}), observed ({observed_row},{observed_col})"
            ),
            Self::ContinuationWidth { row, col, width } => {
                write!(
                    f,
                    "continuation cell ({row},{col}) has non-zero width {width}"
                )
            }
            Self::Serialize(msg) => write!(f, "serialize: {msg}"),
            Self::Deserialize(msg) => write!(f, "deserialize: {msg}"),
            Self::Io(err) => write!(f, "io: {err}"),
        }
    }
}

impl std::error::Error for ParityCellError {}

impl SemanticFrame {
    pub fn to_json_value(&self) -> Result<Value, ParityCellError> {
        self.validate()?;
        serde_json::to_value(self).map_err(|err| ParityCellError::Serialize(err.to_string()))
    }

    pub fn write_cells_json(&self, path: &Path) -> Result<(), ParityCellError> {
        let value = self.to_json_value()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(ParityCellError::Io)?;
        }
        let mut file = fs::File::create(path).map_err(ParityCellError::Io)?;
        serde_json::to_writer_pretty(&mut file, &value)
            .map_err(|err| ParityCellError::Serialize(err.to_string()))?;
        file.write_all(b"\n").map_err(ParityCellError::Io)
    }

    pub fn write_cells_txt(&self, path: &Path) -> Result<(), ParityCellError> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(ParityCellError::Io)?;
        }
        let mut file = fs::File::create(path).map_err(ParityCellError::Io)?;
        write!(file, "{}", self.to_cells_txt()).map_err(ParityCellError::Io)
    }

    pub fn to_cells_txt(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "{}x{} cursor=({},{}) visible={} shape={:?} alt_screen={}\n",
            self.cols,
            self.rows,
            self.cursor.row,
            self.cursor.col,
            self.cursor.visible,
            self.cursor.shape,
            self.alternate_screen
        ));
        for cell in &self.cells {
            if cell_is_default_blank(cell) {
                continue;
            }
            let glyph = cell_glyph_label(cell);
            out.push_str(&format!(
                "[{},{}] '{}' w={} cont={} fg={} bg={} bold={} dim={} italic={} underline={} inverse={}\n",
                cell.row,
                cell.col,
                glyph,
                cell.width,
                u8::from(cell.continuation),
                cell.fg.to_hex(),
                cell.bg.to_hex(),
                u8::from(cell.modifiers.bold),
                u8::from(cell.modifiers.dim),
                u8::from(cell.modifiers.italic),
                u8::from(cell.modifiers.underline),
                u8::from(cell.modifiers.inverse),
            ));
        }
        out
    }

    pub fn from_json_value(value: &Value) -> Result<Self, ParityCellError> {
        let frame: Self = serde_json::from_value(value.clone())
            .map_err(|err| ParityCellError::Deserialize(err.to_string()))?;
        frame.validate()?;
        Ok(frame)
    }

    pub fn read_cells_json(path: &Path) -> Result<Self, ParityCellError> {
        let text = fs::read_to_string(path).map_err(ParityCellError::Io)?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|err| ParityCellError::Deserialize(err.to_string()))?;
        Self::from_json_value(&value)
    }
}

fn cell_is_default_blank(cell: &SemanticCell) -> bool {
    cell.grapheme.is_empty()
        && !cell.continuation
        && cell.modifiers == CellModifiers::default()
        && cell.fg == ResolvedRgb::from_array(DEFAULT_FG)
        && cell.bg == ResolvedRgb::from_array(DEFAULT_BG)
}

fn cell_glyph_label(cell: &SemanticCell) -> String {
    if cell.continuation {
        "<cont>".to_owned()
    } else if cell.grapheme.is_empty() {
        "<empty>".to_owned()
    } else {
        cell.grapheme.escape_default().to_string()
    }
}
