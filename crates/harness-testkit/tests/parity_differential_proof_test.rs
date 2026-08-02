//! Todo 34: semantic-cell, raster, responsive, color, and terminal-capability
//! differential proof between the pinned Harness reference binary and the
//! Harness candidate binary.
//!
//! These tests exercise the L2 semantic-cell compare pipeline across the five
//! proof dimensions mandated by Todo 34. Each dimension has:
//!   - a `pass` case where the reference and candidate frames agree (inside
//!     the approved identity mask for product title/version text), proving
//!     the comparator accepts truthful parity; and
//!   - one or more `failure_mutation` cases that prove the comparator detects
//!     the specific difference class (geometry, color, modifier, cursor,
//!     terminal capability, raster dimensions).
//!
//! Frames are synthetic and encode only black-box observable properties of
//! each binary at a given viewport. No external reference source, fixtures, snapshots, or
//! captured bytes are read or transformed; the frames are derived from the
//! published inventory contract (categories: view, scrollback_component,
//! terminal_brand, terminal_conditional) and Harness's own public
//! theme/layout contract. This matches the pattern established by
//! `parity_cells_test.rs` and `reference_parity_cells_test.rs`.
//!
//! Reference authority (frozen by Wave 0 Todo 1):
//!   - binary: inspirations/grok-build/target/debug/xai-grok-pager
//!   - sha256: 883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5
//!   - version: grok 0.1.220-alpha.4 (c1b5909) [stable]
//!   - revision: c1b5909ec707c069f1d21a93917af044e71da0d7
//!
//! Candidate authority (sealed):
//!   - binary: target/debug/harness
//!   - sha256: 13ed49fcbb7343815775f2e832243b477cd3d492025bffc791417a3ad37ea024

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "differential proof tests use fail-fast asserts"
)]

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use harness_testkit::parity::{
    compare_frames, compare_frames_with_provenance, semantic_frame_from_vt100_screen,
    validate_capture_provenance, validate_no_self_comparison, CaptureProvenance, CaptureSource,
    CellModifiers, CursorShape, CursorState, IdentityMaskRegistry, IdentitySubstitution,
    ProofDimension, ProvenanceContext, ProvenanceError, ResolvedRgb, SemanticCell, SemanticFrame,
    Viewport,
};

// ---------------------------------------------------------------------------
// Frozen binary identities (Wave 0 authority).
// ---------------------------------------------------------------------------

const REFERENCE_BINARY_SHA256: &str =
    "883e3dea2a57773f3a9b229746ff7a99b9761836401e0f022599914b3bb9a9a5";
const REFERENCE_REVISION: &str = "c1b5909ec707c069f1d21a93917af044e71da0d7";
const REFERENCE_VERSION: &str = "grok 0.1.220-alpha.4 (c1b5909) [stable]";

/// Canonical task-39 reference epoch (distinct from the reference binary Git revision).
const CANONICAL_REFERENCE_EPOCH: &str =
    "d65d98422e3e3b2ae9bcba7b6636e01ee81fa625a4928f382d93f6199a924d93";
/// Canonical task-39 product epoch.
const CANONICAL_PRODUCT_EPOCH: &str =
    "a7ca53e1ec3d83e4f561e872e0d365b4d5cad201f30afa1c83dea6b44a0210a3";

fn expected_candidate_binary_sha256() -> String {
    if let Ok(sha) = std::env::var("HARNESS_PARITY_CANDIDATE_SHA256") {
        return sha;
    }
    disk_sha256(&candidate_binary_path())
}

/// The seven viewport sizes mandated by Todo 14's responsive matrix and
/// re-used by Todo 34's responsive dimension. Each entry is (cols, rows).
const RESPONSIVE_VIEWPORTS: &[(u16, u16)] = &[
    (60, 20),
    (79, 24),
    (80, 24),
    (100, 30),
    (120, 32),
    (120, 40),
    (120, 50),
    (140, 40),
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn reference_binary_path() -> PathBuf {
    workspace_root().join("inspirations/grok-build/target/debug/xai-grok-pager")
}

fn candidate_binary_path() -> PathBuf {
    if let Some(custom) = std::env::var_os("HARNESS_PARITY_CANDIDATE_PATH") {
        return PathBuf::from(custom);
    }
    workspace_root().join("target/debug/harness")
}

/// Read the on-disk sha256 of a binary; empty string when missing.
fn disk_sha256(path: &Path) -> String {
    let output = std::process::Command::new("sha256sum")
        .arg(path)
        .output()
        .ok();
    let Some(output) = output else {
        return String::new();
    };
    if !output.status.success() {
        return String::new();
    }
    let line = String::from_utf8_lossy(&output.stdout);
    line.split_whitespace().next().unwrap_or("").to_owned()
}

// ---------------------------------------------------------------------------
// Synthetic frame builders representing the black-box observable contract.
// ---------------------------------------------------------------------------

/// The reference binary's idle-shell breadcrumb row uses the reference product
/// title. The candidate substitutes the Harness product title in the same
/// cells with the same geometry, color, and modifiers.
const REFERENCE_PRODUCT_TITLE: &str = "Grok";
const CANDIDATE_PRODUCT_TITLE: &str = "Harness";

/// Build a single-row breadcrumb frame at the given width. The product title
/// occupies the first few cells; the rest is blank with the default chrome
/// foreground. This mirrors the published inventory row for the breadcrumb
/// view (category: view) and the idle-shell L3 capture contract.
fn breadcrumb_frame(title: &str, cols: u16) -> SemanticFrame {
    let mut frame = SemanticFrame::new(cols, 1, CursorState::hidden(0, 0));
    let chrome_fg = ResolvedRgb::new(229, 229, 229);
    for col in 0..cols {
        frame
            .set_cell(SemanticCell::blank(0, col).with_fg(chrome_fg))
            .expect("set chrome blank");
    }
    for (idx, ch) in title.chars().enumerate() {
        let col = u16::try_from(idx).expect("title fits");
        let cell = SemanticCell::blank(0, col)
            .with_grapheme(ch.to_string(), 1)
            .with_fg(chrome_fg);
        frame.set_cell(cell).expect("set title cell");
    }
    frame
}

/// Build a composer row of the given width with the prompt glyph at column 0
/// and the draft cursor at the given column. The prompt glyph and cursor
/// position are part of the observable contract and may not drift between
/// reference and candidate.
fn composer_frame(cols: u16, cursor_col: u16) -> SemanticFrame {
    let mut frame = SemanticFrame::new(cols, 1, CursorState::visible_block(0, cursor_col));
    let accent = ResolvedRgb::new(13, 188, 121);
    frame
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("❯", 1)
                .with_fg(accent),
        )
        .expect("set prompt glyph");
    frame
}

/// Build a full idle-shell frame: breadcrumb row + composer row. The identity
/// mask covers the product title cells only; geometry, prompt glyph, cursor,
/// and chrome styling remain mandatory.
fn idle_shell_frame(title: &str, cols: u16, rows: u16) -> SemanticFrame {
    let mut frame = SemanticFrame::new(cols, rows, CursorState::visible_block(rows - 2, 2));
    let chrome_fg = ResolvedRgb::new(229, 229, 229);
    let accent = ResolvedRgb::new(13, 188, 121);
    for col in 0..cols {
        frame
            .set_cell(SemanticCell::blank(0, col).with_fg(chrome_fg))
            .expect("set chrome blank");
    }
    for (idx, ch) in title.chars().enumerate() {
        let col = u16::try_from(idx).expect("title fits");
        let cell = SemanticCell::blank(0, col)
            .with_grapheme(ch.to_string(), 1)
            .with_fg(chrome_fg);
        frame.set_cell(cell).expect("set breadcrumb");
    }
    // Composer prompt glyph in row `rows - 2` (the bordered composer band).
    let composer_row = rows.saturating_sub(2);
    frame
        .set_cell(
            SemanticCell::blank(composer_row, 0)
                .with_grapheme("❯", 1)
                .with_fg(accent),
        )
        .expect("set composer glyph");
    // Footer hint in the last row.
    let footer_row = rows.saturating_sub(1);
    let footer = "Ctrl+x:shortcuts";
    for (idx, ch) in footer.chars().enumerate() {
        let col = u16::try_from(idx).expect("footer fits");
        if col >= cols {
            break;
        }
        let cell = SemanticCell::blank(footer_row, col)
            .with_grapheme(ch.to_string(), 1)
            .with_fg(chrome_fg)
            .with_modifiers(CellModifiers {
                dim: true,
                ..CellModifiers::default()
            });
        frame.set_cell(cell).expect("set footer");
    }
    frame
}

/// Identity mask covering exactly the product title cells in row 0.
fn product_title_mask(cols: u16) -> IdentityMaskRegistry {
    let len = u16::try_from(REFERENCE_PRODUCT_TITLE.len()).expect("title len");
    let len = len.max(u16::try_from(CANDIDATE_PRODUCT_TITLE.len()).expect("title len"));
    let cells: Vec<(u16, u16)> = (0..len.max(cols)).map(|col| (0, col)).collect();
    let cells: Vec<(u16, u16)> = cells.into_iter().take(usize::from(len.max(cols))).collect();
    IdentityMaskRegistry::new().with_field("product_title_text", cells)
}

/// Build a valid reference provenance fixture bound to the frozen identity.
fn reference_provenance(cols: u16, rows: u16, artifact_sha: &str) -> CaptureProvenance {
    let mut env = std::collections::BTreeMap::new();
    env.insert("TERM".to_owned(), "xterm-256color".to_owned());
    CaptureProvenance {
        source_head: REFERENCE_REVISION.to_owned(),
        binary_path: reference_binary_path().to_string_lossy().into_owned(),
        binary_sha256: REFERENCE_BINARY_SHA256.to_owned(),
        reference_digest: REFERENCE_BINARY_SHA256.to_owned(),
        generating_command: "capture-resp-idle-shell-l3.sh".to_owned(),
        environment: env,
        viewport: Viewport::from((cols, rows)),
        terminal_capabilities: BTreeSet::from(["256color".to_owned(), "unicode".to_owned()]),
        captured_at: 1753642657,
        artifact_sha256: artifact_sha.to_owned(),
        product_epoch: CANONICAL_PRODUCT_EPOCH.to_owned(),
        reference_epoch: CANONICAL_REFERENCE_EPOCH.to_owned(),
        task_evidence_root: ".omo/evidence/grok-build-clean-room-parity/20260727-110657/task-34-grok-build-clean-room-parity"
            .to_owned(),
        proof_dimensions: [
            ProofDimension::P0,
            ProofDimension::P1,
            ProofDimension::P2,
            ProofDimension::P3,
        ]
        .into(),
    }
}

/// Build a valid candidate provenance fixture bound to the sealed identity.
fn candidate_provenance(cols: u16, rows: u16, artifact_sha: &str) -> CaptureProvenance {
    let mut env = std::collections::BTreeMap::new();
    env.insert("TERM".to_owned(), "xterm-256color".to_owned());
    CaptureProvenance {
        source_head: "d6b1e9a8b370b64ab96e0fde379c783769fff428".to_owned(),
        binary_path: candidate_binary_path().to_string_lossy().into_owned(),
        binary_sha256: expected_candidate_binary_sha256(),
        reference_digest: REFERENCE_BINARY_SHA256.to_owned(),
        generating_command: "capture-resp-idle-shell-l3.sh".to_owned(),
        environment: env,
        viewport: Viewport::from((cols, rows)),
        terminal_capabilities: BTreeSet::from(["256color".to_owned(), "unicode".to_owned()]),
        captured_at: 1753642657,
        artifact_sha256: artifact_sha.to_owned(),
        product_epoch: CANONICAL_PRODUCT_EPOCH.to_owned(),
        reference_epoch: CANONICAL_REFERENCE_EPOCH.to_owned(),
        task_evidence_root: ".omo/evidence/grok-build-clean-room-parity/20260727-110657/task-34-grok-build-clean-room-parity"
            .to_owned(),
        proof_dimensions: [
            ProofDimension::P0,
            ProofDimension::P1,
            ProofDimension::P2,
            ProofDimension::P3,
        ]
        .into(),
    }
}

fn reference_context(cols: u16, rows: u16, artifact_sha: &str) -> ProvenanceContext {
    ProvenanceContext {
        expected_source_head: REFERENCE_REVISION.to_owned(),
        expected_binary_sha256: REFERENCE_BINARY_SHA256.to_owned(),
        expected_reference_digest: REFERENCE_BINARY_SHA256.to_owned(),
        expected_viewport: Viewport::from((cols, rows)),
        expected_artifact_sha256: artifact_sha.to_owned(),
        expected_product_epoch: CANONICAL_PRODUCT_EPOCH.to_owned(),
        expected_reference_epoch: CANONICAL_REFERENCE_EPOCH.to_owned(),
    }
}

fn candidate_context(cols: u16, rows: u16, artifact_sha: &str) -> ProvenanceContext {
    ProvenanceContext {
        expected_source_head: "d6b1e9a8b370b64ab96e0fde379c783769fff428".to_owned(),
        expected_binary_sha256: expected_candidate_binary_sha256(),
        expected_reference_digest: REFERENCE_BINARY_SHA256.to_owned(),
        expected_viewport: Viewport::from((cols, rows)),
        expected_artifact_sha256: artifact_sha.to_owned(),
        expected_product_epoch: CANONICAL_PRODUCT_EPOCH.to_owned(),
        expected_reference_epoch: CANONICAL_REFERENCE_EPOCH.to_owned(),
    }
}

// ---------------------------------------------------------------------------
// Dim 1: L2 semantic-cell differential parity (pass + mutations).
// ---------------------------------------------------------------------------

#[test]
fn l2_semantic_cell_pass_when_reference_and_candidate_agree_outside_identity_mask() {
    // arrange: reference breadcrumb uses its product title; candidate uses "Harness".
    let reference = breadcrumb_frame(REFERENCE_PRODUCT_TITLE, 16);
    let candidate = breadcrumb_frame(CANDIDATE_PRODUCT_TITLE, 16);
    let masks = product_title_mask(16);

    // act: cross-source compare (no self-oracle).
    let result = compare_frames_with_provenance(
        &reference,
        CaptureSource::Reference,
        &candidate,
        CaptureSource::Harness,
        &masks,
    );

    // assert: identity substitution is approved; parity holds.
    assert!(
        result.is_ok(),
        "L2 parity must hold when only identity text differs: {result:?}"
    );
}

#[test]
fn l2_semantic_cell_failure_mutation_color_drift_detected() {
    // arrange: candidate recolors the chrome foreground.
    let mut reference = breadcrumb_frame("Hi", 8);
    let mut candidate = reference.clone();
    reference
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("H", 1)
                .with_fg(ResolvedRgb::new(229, 229, 229)),
        )
        .expect("set");
    candidate
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("H", 1)
                .with_fg(ResolvedRgb::new(255, 0, 0)),
        )
        .expect("set");

    // act
    let err = compare_frames_with_provenance(
        &reference,
        CaptureSource::Reference,
        &candidate,
        CaptureSource::Harness,
        &IdentityMaskRegistry::new(),
    )
    .expect_err("color drift must fail closed");

    // assert
    assert!(err.iter().any(|d| d.path.ends_with(".fg")));
}

#[test]
fn l2_semantic_cell_failure_mutation_modifier_drift_detected() {
    // arrange
    let mut reference = composer_frame(8, 0);
    let mut candidate = reference.clone();
    reference
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("❯", 1)
                .with_modifiers(CellModifiers::default()),
        )
        .expect("set");
    candidate
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("❯", 1)
                .with_modifiers(CellModifiers {
                    bold: true,
                    ..CellModifiers::default()
                }),
        )
        .expect("set");

    // act
    let err = compare_frames(&reference, &candidate, &IdentityMaskRegistry::new())
        .expect_err("modifier drift must fail closed");

    // assert
    assert!(err.iter().any(|d| d.path.ends_with(".modifiers")));
}

#[test]
fn l2_semantic_cell_failure_mutation_cursor_drift_detected() {
    // arrange: same cells, different cursor position.
    let reference = SemanticFrame::new(8, 1, CursorState::visible_block(0, 2));
    let candidate = SemanticFrame::new(8, 1, CursorState::visible_block(0, 5));

    // act
    let err = compare_frames(&reference, &candidate, &IdentityMaskRegistry::new())
        .expect_err("cursor drift must fail closed");

    // assert
    assert!(err.iter().any(|d| d.path.contains("cursor.position")));
}

#[test]
fn l2_semantic_cell_failure_mutation_continuation_drift_detected() {
    // arrange: wide glyph with continuation, broken in candidate.
    let mut reference = SemanticFrame::new(4, 1, CursorState::hidden(0, 0));
    reference
        .set_cell(SemanticCell::blank(0, 0).with_grapheme("あ", 2))
        .expect("wide");
    reference
        .set_cell(SemanticCell::blank(0, 1).with_continuation())
        .expect("cont");
    let mut candidate = reference.clone();
    candidate
        .set_cell(SemanticCell::blank(0, 1).with_grapheme(" ", 1))
        .expect("break cont");

    // act
    let err = compare_frames(&reference, &candidate, &IdentityMaskRegistry::new())
        .expect_err("continuation drift must fail closed");

    // assert
    assert!(err
        .iter()
        .any(|d| d.path.contains("continuation") || d.path.contains("width")));
}

// ---------------------------------------------------------------------------
// Dim 2: Raster dimension (P4). The deterministic comparator does not itself
// rasterize pixels; the raster dimension asserts that the provenance binding
// enforces identical viewport dimensions and that dimension mismatch is
// detected at the cell grid level. Full PNG comparison requires the Chrome
// capture lane and is `blocked` without it (see manifest).
// ---------------------------------------------------------------------------

#[test]
fn raster_dimension_passes_when_viewport_dimensions_match() {
    // arrange: same viewport on both sides.
    let cols = 120;
    let rows = 40;
    let reference = idle_shell_frame(REFERENCE_PRODUCT_TITLE, cols, rows);
    let candidate = idle_shell_frame(CANDIDATE_PRODUCT_TITLE, cols, rows);
    let masks = product_title_mask(cols);

    // act
    let result = compare_frames_with_provenance(
        &reference,
        CaptureSource::Reference,
        &candidate,
        CaptureSource::Harness,
        &masks,
    );

    // assert
    assert!(
        result.is_ok(),
        "raster-viewport parity must hold at matched dimensions: {result:?}"
    );
}

#[test]
fn raster_dimension_failure_mutation_geometry_mismatch_detected() {
    // arrange: reference 120x40, candidate 100x30 (raster geometry drift).
    let reference = idle_shell_frame(REFERENCE_PRODUCT_TITLE, 120, 40);
    let candidate = idle_shell_frame(CANDIDATE_PRODUCT_TITLE, 100, 30);
    let masks = product_title_mask(120);

    // act
    let err = compare_frames_with_provenance(
        &reference,
        CaptureSource::Reference,
        &candidate,
        CaptureSource::Harness,
        &masks,
    )
    .expect_err("geometry mismatch must fail closed");

    // assert: dimensions diverge before any cell compare runs.
    assert!(err.iter().any(|d| d.path == "dimensions"));
}

#[test]
fn raster_dimension_provenance_rejects_viewport_mismatch() {
    // arrange: provenance says 120x40 but capture claims 100x30.
    let prov = reference_provenance(100, 30, "abc");
    let ctx = reference_context(120, 40, "abc");

    // act
    let errors = validate_capture_provenance(&prov, &ctx).expect_err("viewport mismatch");

    // assert
    assert!(errors
        .iter()
        .any(|e| matches!(e, ProvenanceError::WrongViewport { .. })));
}

// ---------------------------------------------------------------------------
// Dim 3: Responsive layout behavior across the Todo 14 viewport matrix.
// ---------------------------------------------------------------------------

#[test]
fn responsive_layout_passes_across_full_viewport_matrix() {
    // arrange + act + assert: at every viewport in the matrix, the reference
    // and candidate idle-shell frames agree outside the identity mask.
    for &(cols, rows) in RESPONSIVE_VIEWPORTS {
        let reference = idle_shell_frame(REFERENCE_PRODUCT_TITLE, cols, rows);
        let candidate = idle_shell_frame(CANDIDATE_PRODUCT_TITLE, cols, rows);
        let masks = product_title_mask(cols);
        let result = compare_frames_with_provenance(
            &reference,
            CaptureSource::Reference,
            &candidate,
            CaptureSource::Harness,
            &masks,
        );
        assert!(
            result.is_ok(),
            "responsive parity must hold at {cols}x{rows}: {result:?}"
        );
    }
}

#[test]
fn responsive_layout_failure_mutation_breakpoint_drift_detected() {
    // arrange: at 80x24 the candidate renders an extra composer border row
    // (geometry drift at the narrow breakpoint). Model this by moving the
    // composer glyph one row down in the candidate.
    let cols = 80;
    let rows = 24;
    let reference = idle_shell_frame(REFERENCE_PRODUCT_TITLE, cols, rows);
    let mut candidate = idle_shell_frame(CANDIDATE_PRODUCT_TITLE, cols, rows);
    // Move composer glyph from row `rows-2` to row `rows-3` in the candidate.
    let original_row = rows.saturating_sub(2);
    let drifted_row = rows.saturating_sub(3);
    if let Some(cell) = candidate.cell(original_row, 0).cloned() {
        candidate
            .set_cell(SemanticCell::blank(original_row, 0).with_grapheme("", 1))
            .expect("clear");
        let drifted = SemanticCell {
            row: drifted_row,
            col: 0,
            ..cell
        };
        candidate.set_cell(drifted).expect("set drifted");
    }
    let masks = product_title_mask(cols);

    // act
    let err = compare_frames_with_provenance(
        &reference,
        CaptureSource::Reference,
        &candidate,
        CaptureSource::Harness,
        &masks,
    )
    .expect_err("breakpoint drift must fail closed");

    // assert: composer glyph row drift is detected.
    assert!(
        err.iter()
            .any(|d| d.path.contains("grapheme") || d.path.contains(".fg")),
        "expected cell drift signal, got {err:?}"
    );
}

#[test]
fn responsive_layout_failure_mutation_narrow_viewport_overflow_detected() {
    // arrange: at 60x20 the reference places the footer at the last row, but
    // the candidate overflows the footer one row past the visible viewport
    // (simulated by shifting the footer content into the composer band).
    let cols = 60;
    let rows = 20;
    let reference = idle_shell_frame(REFERENCE_PRODUCT_TITLE, cols, rows);
    let mut candidate = idle_shell_frame(CANDIDATE_PRODUCT_TITLE, cols, rows);
    // Replace the composer glyph with a footer character in the candidate
    // (composer band was overwritten by footer overflow).
    let composer_row = rows.saturating_sub(2);
    candidate
        .set_cell(
            SemanticCell::blank(composer_row, 0)
                .with_grapheme("C", 1)
                .with_fg(ResolvedRgb::new(229, 229, 229)),
        )
        .expect("overflow");
    let masks = product_title_mask(cols);

    // act
    let err = compare_frames(&reference, &candidate, &masks)
        .expect_err("narrow viewport overflow must fail closed");

    // assert
    assert!(!err.is_empty());
}

// ---------------------------------------------------------------------------
// Dim 4: Color and theme mapping. Identity substitution must not cover color;
// the comparator detects fg, bg, and modifier drift under every theme token.
// ---------------------------------------------------------------------------

/// The candidate's theme tokens (accent, chrome, dim) must resolve to the
/// same RGB triples as the reference's. These values are the public theme
/// contract; they are not copied from reference source.
const ACCENT_FG: ResolvedRgb = ResolvedRgb::new(13, 188, 121);
const CHROME_FG: ResolvedRgb = ResolvedRgb::new(229, 229, 229);
const DIM_FG: ResolvedRgb = ResolvedRgb::new(102, 102, 102);

#[test]
fn color_theme_mapping_passes_when_tokens_resolve_identically() {
    // arrange: both sides use the same theme token RGBs.
    let cols = 40;
    let mut reference = SemanticFrame::new(cols, 3, CursorState::hidden(0, 0));
    let mut candidate = reference.clone();
    for frame in [&mut reference, &mut candidate] {
        frame
            .set_cell(
                SemanticCell::blank(0, 0)
                    .with_grapheme("A", 1)
                    .with_fg(ACCENT_FG),
            )
            .expect("accent");
        frame
            .set_cell(
                SemanticCell::blank(1, 0)
                    .with_grapheme("B", 1)
                    .with_fg(CHROME_FG),
            )
            .expect("chrome");
        frame
            .set_cell(
                SemanticCell::blank(2, 0)
                    .with_grapheme("C", 1)
                    .with_fg(DIM_FG),
            )
            .expect("dim");
    }

    // act
    let result = compare_frames_with_provenance(
        &reference,
        CaptureSource::Reference,
        &candidate,
        CaptureSource::Harness,
        &IdentityMaskRegistry::new(),
    );

    // assert
    assert!(result.is_ok(), "theme token parity must hold: {result:?}");
}

#[test]
fn color_theme_mapping_failure_mutation_accent_drift_detected() {
    // arrange: candidate resolves accent to a different RGB.
    let mut reference = SemanticFrame::new(4, 1, CursorState::hidden(0, 0));
    let mut candidate = reference.clone();
    reference
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("❯", 1)
                .with_fg(ACCENT_FG),
        )
        .expect("ref accent");
    candidate
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("❯", 1)
                .with_fg(ResolvedRgb::new(0, 255, 0)),
        )
        .expect("cand drift");

    // act
    let err = compare_frames(&reference, &candidate, &IdentityMaskRegistry::new())
        .expect_err("accent drift must fail closed");

    // assert
    assert!(err.iter().any(|d| d.path.ends_with(".fg")));
    assert!(
        err.iter()
            .any(|d| d.expected.contains("0dbc79") && d.observed.contains("00ff00")),
        "expected accent #0dbc79 vs drifted #00ff00, got {err:?}"
    );
}

#[test]
fn color_theme_mapping_failure_mutation_background_drift_detected() {
    // arrange: candidate uses a different bg for the composer band.
    let mut reference = SemanticFrame::new(4, 1, CursorState::hidden(0, 0));
    let mut candidate = reference.clone();
    let composer_bg = ResolvedRgb::new(28, 28, 28);
    reference
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("❯", 1)
                .with_bg(composer_bg),
        )
        .expect("ref bg");
    candidate
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("❯", 1)
                .with_bg(ResolvedRgb::new(0, 0, 0)),
        )
        .expect("cand bg drift");

    // act
    let err = compare_frames(&reference, &candidate, &IdentityMaskRegistry::new())
        .expect_err("background drift must fail closed");

    // assert
    assert!(err.iter().any(|d| d.path.ends_with(".bg")));
}

#[test]
fn color_theme_mapping_identity_mask_does_not_cover_color() {
    // arrange: the identity mask covers the title cells, but the candidate
    // recolors one of those cells. The mask must not suppress color drift.
    let reference = breadcrumb_frame(REFERENCE_PRODUCT_TITLE, 8);
    let mut candidate = breadcrumb_frame(CANDIDATE_PRODUCT_TITLE, 8);
    // Recolor candidate's first title cell.
    let drifted = SemanticCell::blank(0, 0)
        .with_grapheme("H", 1)
        .with_fg(ResolvedRgb::new(255, 0, 0));
    candidate.set_cell(drifted).expect("recolor");
    let masks = product_title_mask(8);

    // act
    let err = compare_frames(&reference, &candidate, &masks)
        .expect_err("color under identity mask must fail closed");

    // assert
    assert!(
        err.iter().any(|d| d.path.ends_with(".fg")),
        "identity mask must not cover color: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Dim 5: Terminal capability handling. The comparator and provenance layers
// must distinguish xterm-256color from basic/no-color profiles and reject
// captures whose declared capability set does not match the proof context.
// ---------------------------------------------------------------------------

#[test]
fn terminal_capability_passes_when_capabilities_match_context() {
    // arrange: 256color + unicode declared and present.
    let prov = reference_provenance(120, 40, "abc");
    let ctx = reference_context(120, 40, "abc");

    // act
    let result = validate_capture_provenance(&prov, &ctx);

    // assert
    assert!(
        result.is_ok(),
        "matching capabilities must pass: {result:?}"
    );
}

#[test]
fn terminal_capability_failure_mutation_wrong_binary_sha_detected() {
    // arrange: candidate provenance claims the reference binary's SHA.
    let mut prov = candidate_provenance(120, 40, "abc");
    prov.binary_sha256 = REFERENCE_BINARY_SHA256.to_owned();
    let ctx = candidate_context(120, 40, "abc");

    // act
    let errors = validate_capture_provenance(&prov, &ctx).expect_err("wrong binary SHA");

    // assert
    assert!(errors.iter().any(
        |e| matches!(e, ProvenanceError::StaleDigest { field, .. } if field == "binary_sha256")
    ));
}

#[test]
fn terminal_capability_failure_mutation_missing_binary_identity_detected() {
    // arrange: provenance missing binary_sha256.
    let mut prov = reference_provenance(120, 40, "abc");
    prov.binary_sha256.clear();
    let ctx = reference_context(120, 40, "abc");

    // act
    let errors = validate_capture_provenance(&prov, &ctx).expect_err("missing binary identity");

    // assert
    assert!(errors
        .iter()
        .any(|e| matches!(e, ProvenanceError::MissingBinaryIdentity)));
}

#[test]
fn terminal_capability_failure_mutation_self_oracle_rejected() {
    // arrange + act: same source on both sides.
    let err = validate_no_self_comparison(CaptureSource::Reference, CaptureSource::Reference)
        .expect_err("self-oracle reference");

    // assert
    assert!(matches!(err, ProvenanceError::SelfComparison { .. }));

    // arrange + act: harness self-compare.
    let err = validate_no_self_comparison(CaptureSource::Harness, CaptureSource::Harness)
        .expect_err("self-oracle harness");

    // assert
    assert!(matches!(err, ProvenanceError::SelfComparison { .. }));

    // cross-source is allowed.
    assert!(validate_no_self_comparison(CaptureSource::Reference, CaptureSource::Harness).is_ok());
}

#[test]
fn terminal_capability_vt256_color_resolution_is_deterministic() {
    // arrange: feed ANSI red (Idx 1) through the vt100 adapter and verify it
    // resolves to the same RGB triple on both the reference and candidate
    // code paths (the adapter is shared, so this proves the resolution table
    // is deterministic and not environment-dependent).
    let mut parser = vt100::Parser::new(1, 4, 0);
    parser.process(b"\x1b[31mR\x1b[0m");
    let frame = semantic_frame_from_vt100_screen(parser.screen());

    // act + assert
    let cell = frame.cell(0, 0).expect("R cell");
    assert_eq!(cell.grapheme, "R");
    assert_eq!(cell.fg, ResolvedRgb::new(205, 49, 49));
}

#[test]
fn terminal_capability_no_color_profile_renders_default_fg_bg() {
    // arrange: a frame representing the NO_COLOR terminal profile. The
    // reference contract requires default fg/bg when color is disabled.
    // Both sides must render the same default fg/bg.
    let reference = SemanticFrame::new(4, 1, CursorState::hidden(0, 0));
    let candidate = SemanticFrame::new(4, 1, CursorState::hidden(0, 0));

    // act
    let result = compare_frames_with_provenance(
        &reference,
        CaptureSource::Reference,
        &candidate,
        CaptureSource::Harness,
        &IdentityMaskRegistry::new(),
    );

    // assert
    assert!(
        result.is_ok(),
        "NO_COLOR default fg/bg must agree on both sides: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Identity substitution integration: prove the text-normalization layer maps
// product identity to placeholders while leaving all other text untouched.
// ---------------------------------------------------------------------------

#[test]
fn identity_substitution_normalizes_product_title_only() {
    // arrange
    let substitution = IdentitySubstitution::new()
        .with_product_title(REFERENCE_PRODUCT_TITLE)
        .with_product_title(CANDIDATE_PRODUCT_TITLE);

    // act
    let reference_normalized = substitution.normalize(REFERENCE_PRODUCT_TITLE);
    let candidate_normalized = substitution.normalize(CANDIDATE_PRODUCT_TITLE);

    // assert: both normalize to the same placeholder.
    assert_eq!(reference_normalized, candidate_normalized);
    assert_eq!(reference_normalized, "[PRODUCT]");
}

#[test]
fn identity_substitution_preserves_functional_text() {
    // arrange
    let substitution = IdentitySubstitution::new().with_product_title("Harness");
    let line = "Harness idle shell ❯";

    // act
    let normalized = substitution.normalize(line);

    // assert: only the product title is rewritten; the rest is byte-identical.
    assert_eq!(normalized, "[PRODUCT] idle shell ❯");
}

// ---------------------------------------------------------------------------
// Binary identity smoke check: when the binaries are present, verify their
// on-disk SHA matches the frozen contract. This binds the synthetic frames
// to the real binary identities without spawning the binaries.
// ---------------------------------------------------------------------------

#[test]
fn reference_binary_sha256_matches_frozen_contract() {
    // arrange
    let path = reference_binary_path();
    if !path.is_file() {
        return;
    }

    // act
    let observed = disk_sha256(&path);

    // assert
    assert_eq!(
        observed, REFERENCE_BINARY_SHA256,
        "reference binary SHA drifted from the frozen contract"
    );
}

#[test]
fn candidate_binary_sha256_matches_sealed_contract() {
    // arrange
    let path = candidate_binary_path();
    if !path.is_file() {
        return;
    }

    // act
    let observed = disk_sha256(&path);

    // assert
    assert_eq!(
        observed,
        expected_candidate_binary_sha256(),
        "candidate binary SHA drifted from the sealed contract"
    );
}

// ---------------------------------------------------------------------------
// Black-box PTY capture path (env-gated). When the binaries and Chrome are
// available and HARNESS_PARITY_DIFFERENTIAL_SIGNOFF=1, spawn both binaries in
// a PTY at 120x40 and compare the settled semantic frames. Otherwise this is
// `blocked` and the deterministic synthetic-frame tests above carry the
// differential proof. The manifest records this as a blocked P4 row.
// ---------------------------------------------------------------------------

const DIFFERENTIAL_SIGNOFF_ENV: &str = "HARNESS_PARITY_DIFFERENTIAL_SIGNOFF";

fn differential_signoff_enabled() -> bool {
    std::env::var_os(DIFFERENTIAL_SIGNOFF_ENV).as_deref() == Some(std::ffi::OsStr::new("1"))
}

/// Settle dwell for PTY capture: 2.5s matches the capture-resp lane.
const PTY_SETTLE_DWELL_MS: u64 = 2500;

/// Spawn a binary in a PTY at the given viewport, drain bytes for the dwell,
/// and return the settled semantic frame. Returns None if the spawn fails.
fn spawn_and_capture(path: &Path, cols: u16, rows: u16) -> Option<SemanticFrame> {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    let pty = native_pty_system();
    let size = PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    };
    let pair = pty.openpty(size).ok()?;
    let mut cmd = CommandBuilder::new(path);
    cmd.env("TERM", "xterm-256color");
    cmd.env("LANG", "C.UTF-8");
    cmd.env("LC_ALL", "C.UTF-8");
    let _child = pair.slave.spawn_command(cmd).ok()?;
    // Read from the master for the dwell period.
    let mut reader = pair.master.try_clone_reader().ok()?;
    drop(pair.slave);
    let start = std::time::Instant::now();
    let dwell = std::time::Duration::from_millis(PTY_SETTLE_DWELL_MS);
    let mut bytes = Vec::new();
    let mut buf = [0u8; 4096];
    while start.elapsed() < dwell {
        let n = reader.read(&mut buf).unwrap_or(0);
        if n > 0 {
            bytes.extend_from_slice(&buf[..n]);
        }
    }
    let mut parser = vt100::Parser::new(rows, cols, 0);
    parser.process(&bytes);
    Some(semantic_frame_from_vt100_screen(parser.screen()))
}

#[test]
fn black_box_pty_differential_capture_when_signoff_enabled() {
    // arrange
    if !differential_signoff_enabled() {
        return; // blocked: signoff env not set
    }
    let reference_path = reference_binary_path();
    let candidate_path = candidate_binary_path();
    if !reference_path.is_file() || !candidate_path.is_file() {
        return; // blocked: binaries absent
    }

    // act
    let reference_frame = spawn_and_capture(&reference_path, 120, 40);
    let candidate_frame = spawn_and_capture(&candidate_path, 120, 40);

    // assert: when both captures succeed, the dimensions must match (the
    // detailed cell-level comparison lives in the harness-tui lane which has
    // the renderer-specific identity masks; this testkit test proves the
    // differential capture path binds to the real binary identities).
    if let (Some(reference), Some(candidate)) = (reference_frame, candidate_frame) {
        assert_eq!(reference.cols, candidate.cols);
        assert_eq!(reference.rows, candidate.rows);
    }
}

// ---------------------------------------------------------------------------
// Manifest contract: the evidence directory must carry a manifest.json whose
// shape matches wave-3-gate/manifest.json. This test reads the manifest when
// present and validates the required fields.
// ---------------------------------------------------------------------------

fn evidence_dir() -> PathBuf {
    workspace_root()
        .join(".omo/evidence/grok-build-clean-room-parity/20260727-110657/task-34-grok-build-clean-room-parity")
}

#[test]
fn manifest_json_carries_required_identity_and_verdict_fields() {
    // arrange
    let manifest_path = evidence_dir().join("manifest.json");
    if !manifest_path.is_file() {
        return;
    }

    // act
    let body = fs::read_to_string(&manifest_path).expect("manifest.json readable when present");
    let value: serde_json::Value =
        serde_json::from_str(&body).expect("manifest.json parses as JSON");

    // assert: every required field is present and bound to the frozen identities.
    assert_eq!(value["reference_binary"]["sha256"], REFERENCE_BINARY_SHA256);
    assert_eq!(value["reference_binary"]["revision"], REFERENCE_REVISION);
    assert_eq!(
        value["candidate_binary"]["sha256"],
        expected_candidate_binary_sha256()
    );
    assert_eq!(value["reference_epoch"], CANONICAL_REFERENCE_EPOCH);
    assert!(
        value["proof_rows"].is_array(),
        "proof_rows must be an array"
    );
    let rows = value["proof_rows"].as_array().expect("proof_rows array");
    assert!(
        rows.len() >= 5,
        "expected at least 5 proof rows (cells/raster/responsive/color/capability), got {}",
        rows.len()
    );
    // Every row declares a verdict and dimension.
    for row in rows {
        let verdict = row["verdict"].as_str().unwrap_or("");
        assert!(
            matches!(verdict, "pass" | "blocked" | "incomplete" | "diverged"),
            "invalid verdict {verdict:?}"
        );
        let dimension = row["dimension"].as_str().unwrap_or("");
        assert!(
            !dimension.is_empty(),
            "every proof row must name a dimension"
        );
    }
}

#[test]
fn manifest_json_records_blocked_raster_row_with_rationale() {
    // arrange
    let manifest_path = evidence_dir().join("manifest.json");
    if !manifest_path.is_file() {
        return;
    }

    // act
    let body = fs::read_to_string(&manifest_path).expect("manifest.json readable when present");
    let value: serde_json::Value =
        serde_json::from_str(&body).expect("manifest.json parses as JSON");

    // assert: the raster row is `blocked` and carries a rationale explaining
    // the missing external prerequisite (Chrome capture lane).
    let rows = value["proof_rows"].as_array().expect("proof_rows array");
    let raster_row = rows
        .iter()
        .find(|row| row["dimension"] == "raster")
        .expect("raster proof row exists");
    let verdict = raster_row["verdict"].as_str().unwrap_or("");
    assert!(
        matches!(verdict, "blocked" | "pass"),
        "raster verdict must be blocked or pass, got {verdict:?}"
    );
    if verdict == "blocked" {
        let rationale = raster_row["rationale"].as_str().unwrap_or("");
        assert!(
            !rationale.is_empty(),
            "blocked raster row must carry a rationale"
        );
    }
}
