//! Exact semantic-frame comparison (fail-closed, no SSIM).
//!
//! Optional identity masks are field-level and only exempt grapheme text at
//! explicitly registered cell coordinates. Geometry, color, modifiers, and
//! cursor remain mandatory.

use std::collections::BTreeSet;
use std::fmt;

use super::cells::{CursorState, SemanticCell, SemanticFrame};

/// Number of consecutive identical frames required for settle (contract §15).
pub const SETTLE_IDENTICAL_FRAMES: usize = 3;

/// Identity-field mask: only grapheme may differ at listed cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdentityFieldMask {
    pub field_id: String,
    pub cells: BTreeSet<(u16, u16)>,
}

impl IdentityFieldMask {
    pub fn new(field_id: impl Into<String>, cells: impl IntoIterator<Item = (u16, u16)>) -> Self {
        Self {
            field_id: field_id.into(),
            cells: cells.into_iter().collect(),
        }
    }

    pub fn contains(&self, row: u16, col: u16) -> bool {
        self.cells.contains(&(row, col))
    }
}

/// Explicit mask registry (default: empty / no exemptions).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdentityMaskRegistry {
    fields: Vec<IdentityFieldMask>,
}

impl IdentityMaskRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, mask: IdentityFieldMask) {
        self.fields.push(mask);
    }

    pub fn with_field(
        mut self,
        field_id: impl Into<String>,
        cells: impl IntoIterator<Item = (u16, u16)>,
    ) -> Self {
        self.register(IdentityFieldMask::new(field_id, cells));
        self
    }

    pub fn grapheme_mask_field(&self, row: u16, col: u16) -> Option<&str> {
        self.fields
            .iter()
            .find(|field| field.contains(row, col))
            .map(|field| field.field_id.as_str())
    }
}

/// One unapproved difference (fail-closed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellDiff {
    pub path: String,
    pub expected: String,
    pub observed: String,
}

impl CellDiff {
    /// Construct a new cell difference with the given path, expected, and
    /// observed values.
    pub fn new(
        path: impl Into<String>,
        expected: impl Into<String>,
        observed: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            expected: expected.into(),
            observed: observed.into(),
        }
    }
}

impl fmt::Display for CellDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: expected {}, observed {}",
            self.path, self.expected, self.observed
        )
    }
}

pub type CompareResult = Result<(), Vec<CellDiff>>;

/// Exact compare; identity masks exempt grapheme only.
pub fn compare_frames(
    expected: &SemanticFrame,
    actual: &SemanticFrame,
    masks: &IdentityMaskRegistry,
) -> CompareResult {
    let mut diffs = Vec::new();

    if expected.cols != actual.cols || expected.rows != actual.rows {
        diffs.push(CellDiff::new(
            "dimensions",
            format!("{}x{}", expected.cols, expected.rows),
            format!("{}x{}", actual.cols, actual.rows),
        ));
        return Err(diffs);
    }

    if expected.alternate_screen != actual.alternate_screen {
        diffs.push(CellDiff::new(
            "alternate_screen",
            expected.alternate_screen.to_string(),
            actual.alternate_screen.to_string(),
        ));
    }

    compare_cursor(&expected.cursor, &actual.cursor, &mut diffs);

    let cell_count = expected.cells.len().min(actual.cells.len());
    if expected.cells.len() != actual.cells.len() {
        diffs.push(CellDiff::new(
            "cells.len",
            expected.cells.len().to_string(),
            actual.cells.len().to_string(),
        ));
    }

    for idx in 0..cell_count {
        compare_cell(&expected.cells[idx], &actual.cells[idx], masks, &mut diffs);
    }

    if diffs.is_empty() {
        Ok(())
    } else {
        Err(diffs)
    }
}

fn compare_cursor(expected: &CursorState, actual: &CursorState, diffs: &mut Vec<CellDiff>) {
    if expected.row != actual.row || expected.col != actual.col {
        diffs.push(CellDiff::new(
            "cursor.position",
            format!("({},{})", expected.row, expected.col),
            format!("({},{})", actual.row, actual.col),
        ));
    }
    if expected.visible != actual.visible {
        diffs.push(CellDiff::new(
            "cursor.visible",
            expected.visible.to_string(),
            actual.visible.to_string(),
        ));
    }
    if expected.shape != actual.shape {
        diffs.push(CellDiff::new(
            "cursor.shape",
            format!("{:?}", expected.shape),
            format!("{:?}", actual.shape),
        ));
    }
}

fn compare_cell(
    expected: &SemanticCell,
    actual: &SemanticCell,
    masks: &IdentityMaskRegistry,
    diffs: &mut Vec<CellDiff>,
) {
    let base = format!("cell[{},{}]", expected.row, expected.col);

    if expected.width != actual.width {
        diffs.push(CellDiff::new(
            format!("{base}.width"),
            expected.width.to_string(),
            actual.width.to_string(),
        ));
    }
    if expected.continuation != actual.continuation {
        diffs.push(CellDiff::new(
            format!("{base}.continuation"),
            expected.continuation.to_string(),
            actual.continuation.to_string(),
        ));
    }
    if expected.fg != actual.fg {
        diffs.push(CellDiff::new(
            format!("{base}.fg"),
            expected.fg.to_hex(),
            actual.fg.to_hex(),
        ));
    }
    if expected.bg != actual.bg {
        diffs.push(CellDiff::new(
            format!("{base}.bg"),
            expected.bg.to_hex(),
            actual.bg.to_hex(),
        ));
    }
    if expected.modifiers != actual.modifiers {
        diffs.push(CellDiff::new(
            format!("{base}.modifiers"),
            format!("{:?}", expected.modifiers),
            format!("{:?}", actual.modifiers),
        ));
    }
    if expected.hyperlink != actual.hyperlink {
        diffs.push(CellDiff::new(
            format!("{base}.hyperlink"),
            format!("{:?}", expected.hyperlink),
            format!("{:?}", actual.hyperlink),
        ));
    }

    if expected.grapheme != actual.grapheme
        && masks
            .grapheme_mask_field(expected.row, expected.col)
            .is_none()
    {
        diffs.push(CellDiff::new(
            format!("{base}.grapheme"),
            format!("{:?}", expected.grapheme),
            format!("{:?}", actual.grapheme),
        ));
    }
}

pub fn is_settled(frames: &[SemanticFrame]) -> bool {
    if frames.len() < SETTLE_IDENTICAL_FRAMES {
        return false;
    }
    let start = frames.len() - SETTLE_IDENTICAL_FRAMES;
    let first = &frames[start];
    frames[start + 1..].iter().all(|frame| frame == first)
}

/// Tracks consecutive identical frames until settle.
#[derive(Clone, Debug, Default)]
pub struct StableFrameTracker {
    recent: Vec<SemanticFrame>,
    required: usize,
}

impl StableFrameTracker {
    pub fn new() -> Self {
        Self {
            recent: Vec::new(),
            required: SETTLE_IDENTICAL_FRAMES,
        }
    }

    pub fn observe(&mut self, frame: SemanticFrame) -> bool {
        if let Some(last) = self.recent.last() {
            if last != &frame {
                self.recent.clear();
            }
        }
        self.recent.push(frame);
        if self.recent.len() > self.required {
            let drain = self.recent.len() - self.required;
            self.recent.drain(0..drain);
        }
        self.is_settled()
    }

    pub fn is_settled(&self) -> bool {
        self.recent.len() >= self.required && self.recent.windows(2).all(|pair| pair[0] == pair[1])
    }

    pub fn settled_frame(&self) -> Option<&SemanticFrame> {
        if self.is_settled() {
            self.recent.last()
        } else {
            None
        }
    }

    pub fn consecutive_identical(&self) -> usize {
        self.recent.len()
    }
}
