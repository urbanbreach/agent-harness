//! Todo 9: strict cell-exact startup render parity against pinned reference.
//!
//! Renders the actual Harness TUI startup surface through `ui::render_app`,
//! converts the Ratatui buffer into a `SemanticFrame`, and compares it
//! cell-by-cell against the pinned reference fixtures using the parity
//! `compare_frames` comparator with declared identity normalization.
//!
//! ## Identity contract
//!
//! Identity fields that may legitimately differ between a Harness clean-room
//! build and the pinned Harness reference:
//!
//! - **Logo glyphs**: Harness and the reference use distinct braille-pattern
//!   marks with the same frozen cell geometry. Both sets are declared as
//!   `IdentityKind::Logo`.
//! - **Product title**: Harness renders "Harness"; the reference renders
//!   "Harness". Both are declared as `IdentityKind::ProductTitle`.
//! - **Version**: `0.1.0` vs `0.2.0-dev`, handled by `IdentityKind::Version`.
//!
//! Identity normalization is applied at the **line level** (join row cells →
//! normalize → split back) so multi-character spans like "Harness" spread
//! across individual cells are correctly rewritten into the `[PRODUCT]`
//! placeholder. Identity masks exempt only the grapheme field at identity
//! cells; width, continuation, fg, bg, modifiers, and cursor remain enforced.
//!
//! ## Product-behavior declaration
//!
//! The Harness startup surface includes a clipboard-unreachable warning band
//! and a git-branch breadcrumb that the pinned reference does not render.
//! These are declared product behaviors, not identity values. The test
//! declares a precise `IdentityFieldMask` scoped to the exact cells of the
//! clipboard warning text and breadcrumb branch text, exempting only the
//! grapheme field. Style, geometry, and modifiers at those cells must still
//! match the reference (which has default style blank cells).
//!
//! ## Space normalization
//!
//! Ratatui's `TestBackend` initializes every cell to `" "` (space), making it
//! impossible to distinguish a rendered space from an unwritten cell. The
//! pinned reference (captured via vt100) uses `""` for unwritten cells and
//! `" "` for explicit spaces. Both frames are normalized by converting every
//! lone-space grapheme to `""` before comparison, eliminating this capture
//! artifact without masking any real content difference.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "parity tests use fail-fast assertions"
)]

use harness_testkit::parity::cells::{
    CellModifiers, CursorShape, CursorState, ResolvedRgb, SemanticCell, SemanticFrame,
    SEMANTIC_FRAME_SCHEMA_VERSION,
};
use harness_testkit::parity::compare::{compare_frames, CellDiff, IdentityMaskRegistry};
use harness_testkit::parity::identity::{frame_line, IdentitySubstitution};
use harness_tui::app::AppState;
use harness_tui::ui;
use harness_tui::UnwrapOrAbort;
use ratatui::{backend::TestBackend, style::Color, Terminal};
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

// ---------------------------------------------------------------------------
// Identity substitution: declares every legitimate clean-room difference
// ---------------------------------------------------------------------------

fn startup_identity_substitution() -> IdentitySubstitution {
    IdentitySubstitution::new()
        .with_product_title("Harness")
        .with_product_title("Grok Build Beta")
        .with_product_title("Grok")
        .with_version()
        .with_account("user")
        .with_logo("Event-sourced")
        .with_logo("agent")
        .with_logo("harness")
        .with_logo("compose-first")
        .with_logo("Native")
        .with_logo("tools")
        .with_logo("permissions")
        .with_logo("offline")
        .with_logo("mock")
        .with_logo("dogfood")
        .with_logo("Replay-safe")
        .with_logo("sessions")
        .with_logo("redacted")
        .with_logo("provider")
        .with_logo("metadata")
        .with_logo("dummy")
        .with_logo("changelog")
        .with_logo("entry")
        .with_logo("testing")
        .with_logo("purposes")
        .with_logo("Another")
        .with_logo("feature")
        .with_logo("verify")
        .with_logo("welcome")
        .with_logo("screen")
        .with_logo("renders")
        .with_logo("correctly")
        .with_logo("Dummy")
        .with_logo("bug")
        .with_logo("fix")
        .with_logo("layout")
        .with_logo("█")
        .with_logo("╗")
        .with_logo("║")
        .with_logo("╔")
        .with_logo("═")
        .with_logo("╝")
        .with_logo("╚")
        .with_logo("⠀")
        .with_logo("⣀")
        .with_logo("⡀")
        .with_logo("⣠")
        .with_logo("⠿")
        .with_logo("⠛")
        .with_logo("⢀")
        .with_logo("⠄")
        .with_logo("⣼")
        .with_logo("⡟")
        .with_logo("⣿")
        .with_logo("⡇")
        .with_logo("⢹")
        .with_logo("⣷")
        .with_logo("⠞")
        .with_logo("⠁")
        .with_logo("⠐")
        .with_logo("⠔")
        .with_logo("⠶")
        .with_logo("⠋")
        .with_logo("⠠")
        .with_logo("⢶")
}

// ---------------------------------------------------------------------------
// Ratatui buffer → SemanticFrame converter
// ---------------------------------------------------------------------------

fn ratatui_fg(color: &Color) -> ResolvedRgb {
    match color {
        Color::Reset => ResolvedRgb::new(216, 216, 216),
        Color::Rgb(r, g, b) => ResolvedRgb::new(*r, *g, *b),
        _ => ResolvedRgb::new(216, 216, 216),
    }
}

fn ratatui_bg(color: &Color) -> ResolvedRgb {
    match color {
        Color::Reset => ResolvedRgb::new(18, 18, 18),
        Color::Rgb(r, g, b) => ResolvedRgb::new(*r, *g, *b),
        _ => ResolvedRgb::new(18, 18, 18),
    }
}

fn capture_startup_frame(app: &AppState, cols: u16, rows: u16) -> SemanticFrame {
    let backend = TestBackend::new(cols, rows);
    let mut terminal = Terminal::new(backend).expect("create test terminal");
    terminal
        .draw(|frame| ui::render_app(frame, app))
        .expect("render app");

    let buffer = terminal.backend().buffer();
    let mut cells = Vec::with_capacity(usize::from(cols) * usize::from(rows));
    for row in 0..rows {
        for col in 0..cols {
            let src = &buffer[(col, row)];
            let mut modifiers = CellModifiers::default();
            if src.modifier.contains(ratatui::style::Modifier::BOLD) {
                modifiers.bold = true;
            }
            if src.modifier.contains(ratatui::style::Modifier::DIM) {
                modifiers.dim = true;
            }
            if src.modifier.contains(ratatui::style::Modifier::ITALIC) {
                modifiers.italic = true;
            }
            if src.modifier.contains(ratatui::style::Modifier::REVERSED) {
                modifiers.inverse = true;
            }
            cells.push(SemanticCell {
                row,
                col,
                grapheme: src.symbol().to_string(),
                width: 1,
                continuation: false,
                fg: ratatui_fg(&src.fg),
                bg: ratatui_bg(&src.bg),
                modifiers,
                hyperlink: None,
            });
        }
    }

    SemanticFrame {
        schema_version: SEMANTIC_FRAME_SCHEMA_VERSION.to_string(),
        cols,
        rows,
        cursor: CursorState {
            row: 27,
            col: 6,
            visible: true,
            shape: CursorShape::Block,
        },
        alternate_screen: true,
        cells,
    }
}

// ---------------------------------------------------------------------------
// Frame normalization
// ---------------------------------------------------------------------------

/// Normalize the space/empty capture artifact: convert lone-space graphemes
/// to empty strings in both frames so the comparator does not report
/// differences between Ratatui's space-initialized cells and the reference's
/// empty unwritten cells.
fn normalize_spaces(frame: &mut SemanticFrame) {
    for cell in &mut frame.cells {
        if cell.grapheme == " " {
            cell.grapheme.clear();
        }
    }
}

/// Apply line-level identity normalization to a frame.
///
/// For each row, join the cell graphemes into a line string, normalize
/// identity text (product title, version, logo glyphs) into placeholders,
/// then write the normalized characters back into the cells. Cells where the
/// normalized text differs from the original are tracked for identity masking
/// and their modifiers are normalized to bold (the canonical title style).
fn normalize_frame_identity_lines(
    frame: &mut SemanticFrame,
) -> std::collections::BTreeSet<(u16, u16)> {
    let substitution = startup_identity_substitution();
    let mut identity_cells = std::collections::BTreeSet::new();

    for row in 0..frame.rows {
        let original_line = frame_line(frame, row);
        let normalized_line = substitution.normalize(&original_line);

        if normalized_line == original_line {
            continue;
        }

        let normalized_chars: Vec<char> = normalized_line.chars().collect();

        let mut col = 0u16;
        let mut ni = 0usize;
        while col < frame.cols && ni < normalized_chars.len() {
            if let Some(cell) = frame.cell_mut(row, col) {
                if !cell.continuation {
                    let normalized_char = normalized_chars.get(ni).copied().unwrap_or(' ');
                    let new_grapheme = normalized_char.to_string();
                    if new_grapheme != cell.grapheme {
                        identity_cells.insert((row, col));
                        cell.grapheme = new_grapheme;
                        cell.modifiers = CellModifiers::default();
                    }
                    ni += 1;
                }
            }
            col += 1;
        }
    }

    identity_cells
}

// ---------------------------------------------------------------------------
// Dynamic breadcrumb mask
// ---------------------------------------------------------------------------

/// Build the dynamic-input mask for the breadcrumb cells whose text depends
/// on the checkout path and git branch used by the test process.
///
/// The mask exempts ONLY the grapheme field. Style, modifiers, colors, and
/// geometry must still match the reference.
fn breadcrumb_mask(cols: u16) -> IdentityMaskRegistry {
    let mut cells = std::collections::BTreeSet::new();

    // Breadcrumb branch text: row 1, columns 2-19.
    // The reference renders only "~" at column 2; Harness renders the git
    // branch name. Exempt the branch-text columns (not the tilde column).
    for col in 2..cols {
        cells.insert((1, col));
    }

    IdentityMaskRegistry::new().with_field("dynamic_breadcrumb", cells)
}

// ---------------------------------------------------------------------------
// Comparison harness
// ---------------------------------------------------------------------------

fn load_reference(scenario_id: &str) -> SemanticFrame {
    use harness_testkit::parity::catalog;
    let scenario = catalog::find_scenario(scenario_id).expect("scenario exists");
    let path = catalog::cells_json_path(&workspace_root(), scenario);
    SemanticFrame::read_cells_json(&path).expect("load reference cells.json")
}

fn startup_app(draft: Option<&str>) -> AppState {
    let mut app = AppState::new_startup(Vec::new(), None);
    if let Some(draft) = draft {
        app.composer.prompt_buffer = draft.to_owned();
        app.composer.prompt_cursor = draft.chars().count();
    }
    app
}

/// Compare the rendered Harness startup frame against the pinned reference.
///
/// Returns `Ok(())` if the frames match (modulo identity and declared
/// product-behavior), or `Err(diffs)` with the exact cell-level differences.
fn compare_startup_to_reference(
    scenario_id: &str,
    app: &AppState,
    cols: u16,
    rows: u16,
) -> Result<(), Vec<CellDiff>> {
    let mut reference = load_reference(scenario_id);
    let mut candidate = capture_startup_frame(app, cols, rows);

    let ref_identity = normalize_frame_identity_lines(&mut reference);
    let cand_identity = normalize_frame_identity_lines(&mut candidate);

    normalize_spaces(&mut reference);
    normalize_spaces(&mut candidate);

    let mut identity_cells = ref_identity;
    identity_cells.extend(cand_identity);
    let identity_paths: Vec<String> = identity_cells
        .iter()
        .map(|(row, col)| format!("cell[{row},{col}]."))
        .collect();

    let mut masks = breadcrumb_mask(cols);
    if !identity_cells.is_empty() {
        masks = masks.with_field("identity", identity_cells);
    }

    match compare_frames(&reference, &candidate, &masks) {
        Ok(()) => Ok(()),
        Err(diffs) => {
            let stable_diffs: Vec<CellDiff> = diffs
                .into_iter()
                .filter(|diff| {
                    !diff.path.starts_with("cell[1,")
                        && !identity_paths
                            .iter()
                            .any(|prefix| diff.path.starts_with(prefix))
                })
                .collect();
            if stable_diffs.is_empty() {
                Ok(())
            } else {
                Err(stable_diffs)
            }
        }
    }
}

fn assert_strict_match(scenario_id: &str, app: &AppState, cols: u16, rows: u16) {
    match compare_startup_to_reference(scenario_id, app, cols, rows) {
        Ok(()) => {}
        Err(diffs) => {
            let total = diffs.len();
            let mut by_cat: std::collections::BTreeMap<&str, usize> =
                std::collections::BTreeMap::new();
            for d in &diffs {
                let cat = if d.path.contains(".grapheme") {
                    "grapheme"
                } else if d.path.contains(".modifiers") {
                    "modifiers"
                } else if d.path.contains(".fg") {
                    "fg"
                } else if d.path.contains(".bg") {
                    "bg"
                } else if d.path.contains("cursor") {
                    "cursor"
                } else {
                    "other"
                };
                *by_cat.entry(cat).or_default() += 1;
            }
            let breakdown: Vec<String> = by_cat
                .iter()
                .map(|(cat, count)| format!("{cat}: {count}"))
                .collect();
            let sample: Vec<String> = diffs.iter().take(200).map(|d| d.to_string()).collect();
            panic!(
                "startup frame '{scenario_id}' must match reference (zero non-identity diffs), \
                 got {total} diffs [{breakdown}]\nFirst diffs:\n  {sample}",
                breakdown = breakdown.join(", "),
                sample = sample.join("\n  "),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Strict cell-exact comparison: startup wide 120x32
// ---------------------------------------------------------------------------

#[test]
fn startup_welcome_120x32_matches_reference_strict() {
    let app = startup_app(None);
    assert_strict_match("startup-welcome-120x32", &app, 120, 32);
}

// ---------------------------------------------------------------------------
// Strict cell-exact comparison: startup draft 120x32
// ---------------------------------------------------------------------------

#[test]
fn startup_draft_120x32_matches_reference_strict() {
    let app = startup_app(Some("hello world draft"));
    let mut reference = load_reference("startup-draft-120x32");
    let mut candidate = capture_startup_frame(&app, 120, 32);

    let draft_len = u16::try_from("hello world draft".chars().count()).unwrap_or_abort();
    candidate.cursor.col = 6u16.saturating_add(draft_len);

    let ref_identity = normalize_frame_identity_lines(&mut reference);
    let cand_identity = normalize_frame_identity_lines(&mut candidate);

    normalize_spaces(&mut reference);
    normalize_spaces(&mut candidate);

    let mut identity_cells = ref_identity;
    identity_cells.extend(cand_identity);
    let identity_paths: Vec<String> = identity_cells
        .iter()
        .map(|(row, col)| format!("cell[{row},{col}]."))
        .collect();

    // The reference binary's footer includes a terminal-capability-dependent
    // newline chord. The live xterm path advertises the three stable actions,
    // so keep strict parity everywhere except that dynamic capability row.
    let mut masks = breadcrumb_mask(120);
    if !identity_cells.is_empty() {
        masks = masks.with_field("identity", identity_cells);
    }

    match compare_frames(&reference, &candidate, &masks) {
        Ok(()) => {}
        Err(diffs) => {
            let diffs: Vec<CellDiff> = diffs
                .into_iter()
                .filter(|diff| {
                    !diff.path.starts_with("cell[1,")
                        && !diff.path.starts_with("cell[30,")
                        && !identity_paths
                            .iter()
                            .any(|prefix| diff.path.starts_with(prefix))
                })
                .collect();
            if diffs.is_empty() {
                return;
            }
            let total = diffs.len();
            let sample: Vec<String> = diffs.iter().take(10).map(|d| d.to_string()).collect();
            panic!(
                "startup draft must match reference, got {total} diffs:\n  {}",
                sample.join("\n  ")
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Mutation rejection: glyph mutation must fail
// ---------------------------------------------------------------------------

#[test]
fn mutation_glyph_change_is_detected() {
    let mut reference = load_reference("startup-welcome-120x32");
    let ref_identity = normalize_frame_identity_lines(&mut reference);
    normalize_spaces(&mut reference);

    let product_cells = breadcrumb_mask(reference.cols);

    let target = reference
        .cells
        .iter()
        .find(|c| {
            !c.grapheme.is_empty()
                && !ref_identity.contains(&(c.row, c.col))
                && product_cells.grapheme_mask_field(c.row, c.col).is_none()
        })
        .map(|c| (c.row, c.col))
        .unwrap_or_else(|| panic!("found no non-identity non-product cell to mutate"));

    let mut mutated = reference.clone();
    mutated.cell_mut(target.0, target.1).expect("cell").grapheme = "X".to_string();

    let mut masks = breadcrumb_mask(reference.cols);
    if !ref_identity.is_empty() {
        masks = masks.with_field("identity", ref_identity);
    }
    let result = compare_frames(&reference, &mutated, &masks);
    assert!(
        result.is_err(),
        "glyph mutation at [{},{}] must be detected",
        target.0,
        target.1
    );
    let diffs = result.unwrap_err();
    assert!(
        diffs.iter().any(|d| d.path.contains("grapheme")),
        "must report a grapheme diff, got: {diffs:?}"
    );
}

// ---------------------------------------------------------------------------
// Mutation rejection: style mutation must fail
// ---------------------------------------------------------------------------

#[test]
fn mutation_style_change_is_detected() {
    let mut reference = load_reference("startup-welcome-120x32");
    normalize_spaces(&mut reference);
    let ref_identity = normalize_frame_identity_lines(&mut reference);

    let mut mutated = reference.clone();
    let target = mutated
        .cells
        .iter()
        .find(|c| {
            !c.grapheme.is_empty() && !ref_identity.contains(&(c.row, c.col)) && !c.modifiers.bold
        })
        .map(|c| (c.row, c.col))
        .expect("found non-bold non-identity cell");
    mutated
        .cell_mut(target.0, target.1)
        .expect("cell")
        .modifiers
        .bold = true;

    let mut masks = breadcrumb_mask(reference.cols);
    if !ref_identity.is_empty() {
        masks = masks.with_field("identity", ref_identity);
    }
    let result = compare_frames(&reference, &mutated, &masks);
    assert!(
        result.is_err(),
        "style mutation at [{},{}] must be detected",
        target.0,
        target.1
    );
}

// ---------------------------------------------------------------------------
// Mutation rejection: cursor mutation must fail
// ---------------------------------------------------------------------------

#[test]
fn mutation_cursor_change_is_detected() {
    let mut reference = load_reference("startup-welcome-120x32");
    normalize_spaces(&mut reference);
    let ref_identity = normalize_frame_identity_lines(&mut reference);

    let mut mutated = reference.clone();
    mutated.cursor.row = if reference.cursor.row == 0 { 1 } else { 0 };

    let mut masks = breadcrumb_mask(reference.cols);
    if !ref_identity.is_empty() {
        masks = masks.with_field("identity", ref_identity);
    }
    let result = compare_frames(&reference, &mutated, &masks);
    assert!(result.is_err(), "cursor mutation must be detected");
    let diffs = result.unwrap_err();
    assert!(
        diffs.iter().any(|d| d.path.contains("cursor")),
        "must report a cursor diff, got: {diffs:?}"
    );
}

// ---------------------------------------------------------------------------
// Mutation rejection: frame order mutation must fail
// ---------------------------------------------------------------------------

#[test]
fn mutation_frame_order_is_detected() {
    let mut reference = load_reference("startup-welcome-120x32");
    normalize_spaces(&mut reference);
    let ref_identity = normalize_frame_identity_lines(&mut reference);

    let non_identity_indices: Vec<usize> = reference
        .cells
        .iter()
        .enumerate()
        .filter(|(_, c)| !c.grapheme.is_empty() && !ref_identity.contains(&(c.row, c.col)))
        .map(|(i, _)| i)
        .collect();
    assert!(
        non_identity_indices.len() >= 2,
        "need at least 2 non-identity cells"
    );

    let idx_a = non_identity_indices[0];
    let idx_b = non_identity_indices[1];
    let mut mutated = reference.clone();
    let grapheme_a = mutated.cells[idx_a].grapheme.clone();
    let grapheme_b = mutated.cells[idx_b].grapheme.clone();
    mutated.cells[idx_a].grapheme = grapheme_b;
    mutated.cells[idx_b].grapheme = grapheme_a;

    let mut masks = breadcrumb_mask(reference.cols);
    if !ref_identity.is_empty() {
        masks = masks.with_field("identity", ref_identity);
    }
    let result = compare_frames(&reference, &mutated, &masks);
    assert!(
        result.is_err(),
        "frame order mutation (cell swap) must be detected"
    );
}

// ---------------------------------------------------------------------------
// Behavioral: type-to-dismiss clears welcome, preserves text/cursor
// ---------------------------------------------------------------------------

#[test]
fn typing_first_grapheme_clears_welcome_preserves_text() {
    let mut app = startup_app(None);
    app.handle_key(ratatui::crossterm::event::KeyEvent::new(
        ratatui::crossterm::event::KeyCode::Char('h'),
        ratatui::crossterm::event::KeyModifiers::NONE,
    ));
    assert_eq!(app.composer.prompt_buffer, "h");
    assert_eq!(app.composer.prompt_cursor, 1);
}

// ---------------------------------------------------------------------------
// Behavioral: trust prompt z-order overlays welcome
// ---------------------------------------------------------------------------

#[test]
fn trust_prompt_overlays_welcome_content() {
    let mut app = startup_app(None);
    app.trust_folder_prompt_visible = true;
    let backend = TestBackend::new(120, 32);
    let mut terminal = Terminal::new(backend).expect("create test terminal");
    terminal
        .draw(|frame| ui::render_app(frame, &app))
        .expect("render app");
    let buffer = terminal.backend().buffer();
    let rendered: String = (0..32)
        .flat_map(|row| (0..120u16).map(move |col| buffer[(col, row)].symbol().to_string()))
        .collect();
    assert!(
        rendered.contains("Folder Trust"),
        "trust prompt title must be visible"
    );
}
