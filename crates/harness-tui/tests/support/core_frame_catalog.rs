//! Test support: Core-frame catalog helpers for the harness-tui parity test.
//!
//! This module wraps the `harness_testkit::parity::catalog` module for use in
//! the `core_frame_semantic_parity_test` integration test. It provides:
//!
//! - Workspace root resolution
//! - Reference fixture loading (cells.json, terminal-ansi.bin, receipt.json)
//! - Candidate frame building from ANSI bytes (offline, no PTY/browser)
//! - Identity-normalized comparison against pinned reference fixtures
//! - Adversarial mutation helpers for failure-path testing

use std::path::{Path, PathBuf};

use harness_testkit::parity::catalog::{
    self, compare_candidate_strict, compare_candidate_to_reference, ComparisonOutcome, CoreScenario,
};
use harness_testkit::parity::cells::{CursorState, SemanticCell, SemanticFrame};
use harness_testkit::parity::compare::{CellDiff, IdentityMaskRegistry, StableFrameTracker};
use harness_testkit::parity::frame_io::ParityCellError;
use harness_testkit::parity::identity::IdentitySubstitution;

/// Resolve the workspace root from the test's CARGO_MANIFEST_DIR.
///
/// `CARGO_MANIFEST_DIR` for harness-tui is `crates/harness-tui`; the
/// workspace root is two levels up.
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Load the pinned reference SemanticFrame for a scenario.
pub fn load_reference(scenario: &CoreScenario) -> Result<SemanticFrame, ParityCellError> {
    catalog::load_reference_frame(&workspace_root(), scenario)
}

/// Load the raw ANSI bytes from a scenario's terminal-ansi.bin.
pub fn load_reference_ansi(scenario: &CoreScenario) -> Result<Vec<u8>, ParityCellError> {
    catalog::load_reference_ansi(&workspace_root(), scenario)
}

/// Build a candidate SemanticFrame from raw ANSI bytes (offline, no PTY).
pub fn frame_from_ansi(
    ansi: &[u8],
    cols: u16,
    rows: u16,
) -> Result<SemanticFrame, ParityCellError> {
    catalog::frame_from_ansi(ansi, cols, rows)
}

/// Build a candidate from the reference fixture's own ANSI bytes.
/// Round-trips the pinned ANSI through the vt100 adapter.
pub fn candidate_from_reference_ansi(
    scenario: &CoreScenario,
) -> Result<SemanticFrame, ParityCellError> {
    catalog::candidate_from_reference_ansi(&workspace_root(), scenario)
}

/// Compare a candidate against the pinned reference using identity normalization.
pub fn compare_with_identity(
    scenario: &CoreScenario,
    candidate: &SemanticFrame,
) -> Result<ComparisonOutcome, ParityCellError> {
    compare_candidate_to_reference(&workspace_root(), scenario, candidate)
}

/// Compare a candidate against the pinned reference using strict exact
/// comparison (no identity normalization).
pub fn compare_strict(
    scenario: &CoreScenario,
    candidate: &SemanticFrame,
) -> Result<ComparisonOutcome, ParityCellError> {
    compare_candidate_strict(&workspace_root(), scenario, candidate)
}

/// Check whether a scenario has a complete reference fixture.
pub fn has_reference(scenario: &CoreScenario) -> bool {
    catalog::has_reference_frame(&workspace_root(), scenario)
}

/// Load and validate the receipt for a scenario.
pub fn load_receipt(scenario: &CoreScenario) -> Result<catalog::FixtureReceipt, ParityCellError> {
    catalog::load_receipt(&workspace_root(), scenario)
}

// ---------------------------------------------------------------------------
// Adversarial mutation helpers
// ---------------------------------------------------------------------------

/// Mutate a single glyph at the given coordinates.
/// Returns a new frame with the changed cell.
pub fn mutate_glyph(
    frame: &SemanticFrame,
    row: u16,
    col: u16,
    new_grapheme: &str,
) -> SemanticFrame {
    let mut mutated = frame.clone();
    if let Some(cell) = mutated.cell_mut(row, col) {
        cell.grapheme = new_grapheme.to_owned();
    }
    mutated
}

/// Mutate the foreground color of a single cell.
pub fn mutate_fg(frame: &SemanticFrame, row: u16, col: u16) -> SemanticFrame {
    let mut mutated = frame.clone();
    if let Some(cell) = mutated.cell_mut(row, col) {
        cell.fg = harness_testkit::parity::ResolvedRgb::new(255, 0, 255);
    }
    mutated
}

/// Mutate the background color of a single cell.
pub fn mutate_bg(frame: &SemanticFrame, row: u16, col: u16) -> SemanticFrame {
    let mut mutated = frame.clone();
    if let Some(cell) = mutated.cell_mut(row, col) {
        cell.bg = harness_testkit::parity::ResolvedRgb::new(0, 255, 255);
    }
    mutated
}

/// Mutate the modifiers (style) of a single cell.
pub fn mutate_style(frame: &SemanticFrame, row: u16, col: u16) -> SemanticFrame {
    let mut mutated = frame.clone();
    if let Some(cell) = mutated.cell_mut(row, col) {
        cell.modifiers.bold = !cell.modifiers.bold;
    }
    mutated
}

/// Mutate the cursor position.
pub fn mutate_cursor(frame: &SemanticFrame, row: u16, col: u16) -> SemanticFrame {
    let mut mutated = frame.clone();
    mutated.cursor = CursorState {
        row,
        col,
        visible: mutated.cursor.visible,
        shape: mutated.cursor.shape,
    };
    mutated
}

/// Mutate the cursor visibility.
pub fn mutate_cursor_visibility(frame: &SemanticFrame) -> SemanticFrame {
    let mut mutated = frame.clone();
    mutated.cursor.visible = !mutated.cursor.visible;
    mutated
}

/// Mutate the cursor shape.
pub fn mutate_cursor_shape(frame: &SemanticFrame) -> SemanticFrame {
    let mut mutated = frame.clone();
    mutated.cursor.shape = match mutated.cursor.shape {
        harness_testkit::parity::CursorShape::Block => harness_testkit::parity::CursorShape::Bar,
        _ => harness_testkit::parity::CursorShape::Block,
    };
    mutated
}

/// Swap the order of two cells in the dense grid (simulates frame order
/// mutation). This corrupts the row-major layout invariant.
pub fn swap_cells(frame: &SemanticFrame, idx_a: usize, idx_b: usize) -> SemanticFrame {
    let mut mutated = frame.clone();
    if idx_a < mutated.cells.len() && idx_b < mutated.cells.len() {
        mutated.cells.swap(idx_a, idx_b);
        // Fix the row/col metadata to match the new position so the frame
        // still validates (the swap is about content, not position labels).
        let (ra, ca) = (mutated.cells[idx_a].row, mutated.cells[idx_a].col);
        let (rb, cb) = (mutated.cells[idx_b].row, mutated.cells[idx_b].col);
        mutated.cells[idx_a].row = rb;
        mutated.cells[idx_a].col = cb;
        mutated.cells[idx_b].row = ra;
        mutated.cells[idx_b].col = ca;
    }
    mutated
}

/// Find the first non-blank cell in a frame, returning (row, col).
/// Useful for targeting mutations at visible content.
pub fn first_non_blank_cell(frame: &SemanticFrame) -> Option<(u16, u16)> {
    frame.cells.iter().find_map(|cell| {
        if !cell.grapheme.is_empty() && !cell.continuation {
            Some((cell.row, cell.col))
        } else {
            None
        }
    })
}

/// Find the first cell containing identity text (product title, version, etc.).
pub fn first_identity_cell(frame: &SemanticFrame) -> Option<(u16, u16)> {
    let sub = catalog::default_identity_substitution();
    frame.cells.iter().find_map(|cell| {
        if !cell.grapheme.is_empty() && !cell.continuation {
            let normalized = sub.normalize(&cell.grapheme);
            if normalized != cell.grapheme {
                Some((cell.row, cell.col))
            } else {
                None
            }
        } else {
            None
        }
    })
}

/// Default identity substitution for tests.
pub fn default_identity() -> IdentitySubstitution {
    catalog::default_identity_substitution()
}

/// Create an empty identity mask registry (strict comparison).
pub fn no_masks() -> IdentityMaskRegistry {
    IdentityMaskRegistry::new()
}

/// Create a StableFrameTracker with the default settle threshold.
pub fn new_tracker() -> StableFrameTracker {
    StableFrameTracker::new()
}
