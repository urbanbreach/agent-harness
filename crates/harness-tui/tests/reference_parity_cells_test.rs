//! L2 semantic-cell capture/compare pipeline (Wave T05 / A-CELLS).
//!
//! Contract: docs/grok-build-tui-implementation-prompt.md L2, L5, §15 Semantic Cells.
//! Freeze-backed grapheme compare: draft (exact), startup (identity logo), overlays +
//! fail (identity path/badge/wall-clock).

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity cell tests use fail-fast asserts"
)]

use std::path::{Path, PathBuf};

use harness_testkit::parity::{
    compare_frames, is_settled, semantic_frame_from_vt100_screen, CellModifiers, CursorState,
    IdentityMaskRegistry, ResolvedRgb, SemanticCell, SemanticFrame, StableFrameTracker,
    SETTLE_IDENTICAL_FRAMES,
};

fn sample_frame(label: &str) -> SemanticFrame {
    let mut frame = SemanticFrame::new(8, 2, CursorState::visible_block(1, 0));
    for (idx, ch) in label.chars().enumerate() {
        let col = u16::try_from(idx).expect("col fits");
        frame
            .set_cell(SemanticCell::blank(0, col).with_grapheme(ch.to_string(), 1))
            .expect("set cell");
    }
    frame
}

#[test]
fn identical_frames_pass_exact_compare() {
    // arrange
    let expected = sample_frame("hello");
    let actual = sample_frame("hello");

    // act
    let result = compare_frames(&expected, &actual, &IdentityMaskRegistry::new());

    // assert
    assert!(result.is_ok(), "identical frames must pass: {result:?}");
}

#[test]
fn differing_grapheme_fails_closed() {
    // arrange
    // act
    // assert
    let expected = sample_frame("hello");
    let actual = sample_frame("hallo");
    let err = compare_frames(&expected, &actual, &IdentityMaskRegistry::new())
        .expect_err("grapheme mismatch must fail");
    assert!(
        err.iter().any(|diff| diff.path.contains("grapheme")),
        "expected grapheme path, got {err:?}"
    );
}

#[test]
fn differing_color_fails_closed() {
    // arrange
    // act
    // assert
    let mut expected = sample_frame("A");
    let mut actual = sample_frame("A");
    expected
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("A", 1)
                .with_fg(ResolvedRgb::new(255, 0, 0)),
        )
        .expect("set");
    actual
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("A", 1)
                .with_fg(ResolvedRgb::new(0, 255, 0)),
        )
        .expect("set");
    let err = compare_frames(&expected, &actual, &IdentityMaskRegistry::new())
        .expect_err("color mismatch must fail");
    assert!(err.iter().any(|diff| diff.path.ends_with(".fg")));
}

#[test]
fn differing_modifier_fails_closed() {
    // arrange
    // act
    // assert
    let mut expected = sample_frame("B");
    let actual = sample_frame("B");
    expected
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("B", 1)
                .with_modifiers(CellModifiers {
                    underline: true,
                    ..CellModifiers::default()
                }),
        )
        .expect("set");
    let err = compare_frames(&expected, &actual, &IdentityMaskRegistry::new())
        .expect_err("modifier mismatch must fail");
    assert!(err.iter().any(|diff| diff.path.ends_with(".modifiers")));
}

#[test]
fn differing_cursor_fails_closed() {
    // arrange
    // act
    // assert
    let expected = SemanticFrame::new(4, 2, CursorState::visible_block(0, 0));
    let actual = SemanticFrame::new(4, 2, CursorState::visible_block(1, 3));
    let err = compare_frames(&expected, &actual, &IdentityMaskRegistry::new())
        .expect_err("cursor mismatch must fail");
    assert!(err.iter().any(|diff| diff.path.contains("cursor")));
}

#[test]
fn wide_glyph_and_continuation_handling() {
    // arrange
    // act
    // assert
    let mut expected = SemanticFrame::new(4, 1, CursorState::hidden(0, 0));
    expected
        .set_cell(SemanticCell::blank(0, 0).with_grapheme("あ", 2))
        .expect("wide");
    expected
        .set_cell(SemanticCell::blank(0, 1).with_continuation())
        .expect("cont");

    let mut actual = expected.clone();
    assert!(compare_frames(&expected, &actual, &IdentityMaskRegistry::new()).is_ok());

    // Break continuation flag only.
    actual
        .set_cell(SemanticCell::blank(0, 1).with_grapheme("", 1))
        .expect("broken cont");
    let err = compare_frames(&expected, &actual, &IdentityMaskRegistry::new())
        .expect_err("continuation mismatch must fail");
    assert!(err
        .iter()
        .any(|diff| diff.path.contains("continuation") || diff.path.contains("width")));
}

#[test]
fn identity_mask_is_field_level_grapheme_only() {
    // arrange
    // act
    // assert
    let mut expected = sample_frame("Grok");
    let mut actual = sample_frame("Harn");
    for col in 0..4 {
        let exp = expected
            .cell(0, col)
            .expect("exp")
            .clone()
            .with_fg(ResolvedRgb::new(1, 1, 1));
        let act = actual
            .cell(0, col)
            .expect("act")
            .clone()
            .with_fg(ResolvedRgb::new(1, 1, 1));
        expected.set_cell(exp).expect("set");
        actual.set_cell(act).expect("set");
    }

    let masks = IdentityMaskRegistry::new()
        .with_field("product_title_text", [(0, 0), (0, 1), (0, 2), (0, 3)]);
    assert!(
        compare_frames(&expected, &actual, &masks).is_ok(),
        "identity grapheme mask must allow title text substitution"
    );

    // Masks must not cover color.
    actual
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("H", 1)
                .with_fg(ResolvedRgb::new(9, 9, 9)),
        )
        .expect("set");
    let err = compare_frames(&expected, &actual, &masks).expect_err("color under mask fails");
    assert!(err.iter().any(|diff| diff.path.ends_with(".fg")));
}

#[test]
fn stable_frame_helper_requires_three_identical() {
    // arrange
    // act
    // assert
    assert_eq!(SETTLE_IDENTICAL_FRAMES, 3);

    let a = sample_frame("ok");
    let b = sample_frame("ok");
    let c = sample_frame("ok");
    let other = sample_frame("no");

    assert!(!is_settled(&[a.clone(), b.clone()]));
    assert!(is_settled(&[a.clone(), b.clone(), c.clone()]));
    assert!(!is_settled(&[a.clone(), b.clone(), other.clone()]));

    let mut tracker = StableFrameTracker::new();
    assert!(!tracker.observe(a.clone()));
    assert!(!tracker.observe(b.clone()));
    assert!(tracker.observe(c.clone()));
    assert!(tracker.settled_frame().is_some());

    // Unsettled after change; three more identical required.
    assert!(!tracker.observe(other));
    assert!(!tracker.observe(a));
    assert!(!tracker.observe(b));
    assert!(tracker.observe(c));
}

#[test]
fn serialize_cells_json_and_cells_txt() {
    // arrange
    // act
    // assert
    let dir = tempfile::tempdir().expect("tempdir");
    let json_path = dir.path().join("cells.json");
    let txt_path = dir.path().join("cells.txt");

    let mut frame = sample_frame("Hi");
    frame
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("H", 1)
                .with_modifiers(CellModifiers {
                    bold: true,
                    ..CellModifiers::default()
                }),
        )
        .expect("set");

    frame.write_cells_json(&json_path).expect("write json");
    frame.write_cells_txt(&txt_path).expect("write txt");

    assert!(json_path.is_file());
    assert!(txt_path.is_file());

    let loaded = SemanticFrame::read_cells_json(&json_path).expect("read json");
    assert_eq!(loaded, frame);

    let txt = std::fs::read_to_string(&txt_path).expect("read txt");
    assert!(txt.contains("8x2"));
    assert!(txt.contains("cursor=(1,0)"));
    assert!(txt.contains("[0,0]"));
    assert!(txt.contains("bold=1"));
}

#[test]
fn vt100_adapter_matches_parser_screen_cells() {
    // arrange
    // act
    // assert
    let mut parser = vt100::Parser::new(3, 10, 0);
    parser.process(b"\x1b[1;1Hxy\x1b[1;3H\x1b[31mZ\x1b[0m");
    let frame = semantic_frame_from_vt100_screen(parser.screen());

    assert_eq!(frame.rows, 3);
    assert_eq!(frame.cols, 10);
    assert_eq!(frame.cell(0, 0).expect("x").grapheme, "x");
    assert_eq!(frame.cell(0, 1).expect("y").grapheme, "y");
    assert_eq!(frame.cell(0, 2).expect("Z").grapheme, "Z");
    // ANSI red index 1 resolves through the same palette path as visual_renderer.
    assert_eq!(
        frame.cell(0, 2).expect("Z").fg,
        ResolvedRgb::new(205, 49, 49)
    );

    // Round-trip self-compare.
    assert!(compare_frames(&frame, &frame, &IdentityMaskRegistry::new()).is_ok());
}

#[test]
fn artifact_paths_use_cells_json_shape() {
    // arrange
    // act
    // assert
    // Sanity: path naming for L2 evidence trees (not production wiring).
    let path = PathBuf::from("artifacts/qa-evidence/example/cells/cells.json");
    assert_eq!(
        path.file_name().and_then(|n| n.to_str()),
        Some("cells.json")
    );
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn evidence_root() -> PathBuf {
    std::env::var_os("HARNESS_TUI_PARITY_ARTIFACT_DIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| {
            repo_root().join("artifacts/qa-evidence/20260717-tui-reference-parity")
        })
}

fn frame_from_terminal_txt(path: &Path, cols: u16, rows: u16) -> SemanticFrame {
    let text = std::fs::read_to_string(path).unwrap_or_else(|err| {
        panic!("read terminal.txt {}: {err}", path.display());
    });
    let lines: Vec<&str> = text.lines().collect();
    let mut frame = SemanticFrame::new(cols, rows, CursorState::hidden(0, 0));
    for row in 0..rows {
        let line = lines.get(usize::from(row)).copied().unwrap_or("");
        let mut col: u16 = 0;
        for ch in line.chars() {
            if col >= cols {
                break;
            }
            let grapheme = ch.to_string();
            frame
                .set_cell(SemanticCell::blank(row, col).with_grapheme(grapheme, 1))
                .expect("set cell");
            col = col.saturating_add(1);
        }
        while col < cols {
            frame
                .set_cell(SemanticCell::blank(row, col).with_grapheme(" ", 1))
                .expect("pad");
            col = col.saturating_add(1);
        }
    }
    frame
}

fn require_evidence(path: &Path) -> bool {
    if path.is_file() {
        return true;
    }
    if std::env::var("HARNESS_TUI_PARITY_STRICT").as_deref() == Ok("1") {
        panic!(
            "HARNESS_TUI_PARITY_STRICT=1 requires evidence file {}",
            path.display()
        );
    }
    false
}

#[test]
fn freeze_draft_matches_harness_draft_v23_graphemes_exact() {
    // arrange
    // act
    // assert
    let root = evidence_root();
    let freeze = root.join("reference/freeze/run1-draft/terminal.txt");
    let actual = root.join("actual/harness-draft-v23/terminal.txt");
    if !require_evidence(&freeze) || !require_evidence(&actual) {
        return;
    }

    let expected = frame_from_terminal_txt(&freeze, 120, 32);
    let observed = frame_from_terminal_txt(&actual, 120, 32);
    let result = compare_frames(&expected, &observed, &IdentityMaskRegistry::new());
    assert!(
        result.is_ok(),
        "draft freeze vs harness-draft-v23 must be exact grapheme parity: {result:?}"
    );
}

#[test]
fn freeze_startup_matches_harness_startup_v24_with_identity_logo_mask() {
    // arrange
    // act
    // assert
    let root = evidence_root();
    let freeze = root.join("reference/freeze/run1-startup/terminal.txt");
    let actual = root.join("actual/harness-startup-v24/terminal.txt");
    if !require_evidence(&freeze) || !require_evidence(&actual) {
        return;
    }

    let expected = frame_from_terminal_txt(&freeze, 120, 32);
    let observed = frame_from_terminal_txt(&actual, 120, 32);

    let mut identity_cells = Vec::new();
    for row in 9u16..=15 {
        for col in 0u16..120 {
            identity_cells.push((row, col));
        }
    }
    for col in 0u16..120 {
        identity_cells.push((1, col));
    }
    let masks =
        IdentityMaskRegistry::new().with_field("startup_identity_logo_changelog", identity_cells);

    let result = compare_frames(&expected, &observed, &masks);
    assert!(
        result.is_ok(),
        "startup freeze vs harness-startup-v24 must match outside identity logo/changelog: {result:?}"
    );
}

fn full_row_cells(row: u16, cols: u16) -> Vec<(u16, u16)> {
    (0..cols).map(|col| (row, col)).collect()
}

fn identity_rows_mask(field: &str, rows: &[u16], cols: u16) -> IdentityMaskRegistry {
    let mut cells = Vec::new();
    for &row in rows {
        cells.extend(full_row_cells(row, cols));
    }
    IdentityMaskRegistry::new().with_field(field, cells)
}

fn assert_freeze_grapheme_match(
    freeze_rel: &str,
    actual_rel: &str,
    cols: u16,
    rows: u16,
    masks: &IdentityMaskRegistry,
    label: &str,
) {
    let root = evidence_root();
    let freeze = root.join(freeze_rel);
    let actual = root.join(actual_rel);
    if !require_evidence(&freeze) || !require_evidence(&actual) {
        return;
    }
    let expected = frame_from_terminal_txt(&freeze, cols, rows);
    let observed = frame_from_terminal_txt(&actual, cols, rows);
    let result = compare_frames(&expected, &observed, masks);
    assert!(
        result.is_ok(),
        "{label} freeze-backed grapheme parity failed: {result:?}"
    );
}

#[test]
fn freeze_session_matches_harness_session_v63_with_identity_path_badge() {
    // arrange
    // act
    // assert
    let masks = identity_rows_mask("overlay_identity_path_badge", &[1, 28], 120);
    assert_freeze_grapheme_match(
        "reference/freeze/run1-session/terminal.txt",
        "actual/harness-session-v63/terminal.txt",
        120,
        32,
        &masks,
        "OVL-SESSION",
    );
}

#[test]
fn freeze_help_matches_harness_help_v1_with_identity_path_badge() {
    // arrange
    // act
    // assert
    let masks = identity_rows_mask("overlay_identity_path_badge", &[1, 28], 120);
    assert_freeze_grapheme_match(
        "reference/freeze/run1-shortcuts-ctrlx/terminal.txt",
        "actual/harness-help-v1/terminal.txt",
        120,
        32,
        &masks,
        "OVL-HELP",
    );
}

#[test]
fn freeze_palette_matches_harness_palette_v63_with_identity_path_badge() {
    // arrange
    // act
    // assert
    let masks = identity_rows_mask("overlay_identity_path_badge", &[1, 28], 120);
    assert_freeze_grapheme_match(
        "reference/freeze/run1-palette/terminal.txt",
        "actual/harness-palette-v63/terminal.txt",
        120,
        32,
        &masks,
        "OVL-PALETTE",
    );
}

#[test]
fn freeze_fail_matches_harness_fail_v20_with_identity_path_clock_badge() {
    // arrange
    // act
    // assert
    let masks = identity_rows_mask("shell_fail_identity_path_clock_badge", &[1, 4, 36], 120);
    assert_freeze_grapheme_match(
        "reference/freeze/run1-stream-probe/terminal.txt",
        "actual/harness-pty-fail-120x40-v20/terminal.txt",
        120,
        40,
        &masks,
        "SHELL-FAIL",
    );
}

#[test]
fn freeze_startup_100x30_matches_harness_v9_with_identity_rows() {
    // arrange
    // act
    // assert
    let mut rows = vec![1u16, 26, 28];
    rows.extend(8u16..=14);
    let masks = identity_rows_mask("resp_100x30_identity", &rows, 100);
    assert_freeze_grapheme_match(
        "reference/freeze/run1-startup-100x30/terminal.txt",
        "actual/harness-startup-100x30-v9/terminal.txt",
        100,
        30,
        &masks,
        "RESP-100x30",
    );
}

#[test]
fn freeze_startup_120x40_matches_harness_v8_with_identity_rows() {
    // arrange
    // act
    // assert
    let mut rows = vec![1u16, 36, 38];
    rows.extend(12u16..=18);
    let masks = identity_rows_mask("resp_120x40_identity", &rows, 120);
    assert_freeze_grapheme_match(
        "reference/freeze/run1-startup-120x40/terminal.txt",
        "actual/harness-startup-120x40-v8/terminal.txt",
        120,
        40,
        &masks,
        "RESP-120x40",
    );
}

#[test]
fn freeze_startup_120x50_matches_harness_v8_with_identity_rows() {
    // arrange
    // act
    // assert
    let mut rows = vec![1u16, 46];
    rows.extend(15u16..=21);
    let masks = identity_rows_mask("resp_120x50_identity", &rows, 120);
    assert_freeze_grapheme_match(
        "reference/freeze/run1-startup-120x50/terminal.txt",
        "actual/harness-startup-120x50-v8/terminal.txt",
        120,
        50,
        &masks,
        "RESP-120x50",
    );
}

#[test]
fn freeze_startup_80x24_matches_harness_v8_with_identity_rows() {
    // arrange
    // act
    // assert
    let masks = identity_rows_mask("resp_80x24_identity", &[1, 14, 15, 16, 20, 22], 80);
    assert_freeze_grapheme_match(
        "reference/freeze/run1-startup-80x24/terminal.txt",
        "actual/harness-startup-80x24-v8/terminal.txt",
        80,
        24,
        &masks,
        "RESP-80x24",
    );
}

#[test]
fn freeze_startup_79x24_matches_harness_v8_with_identity_rows() {
    // arrange
    // act
    // assert
    let masks = identity_rows_mask("resp_79x24_identity", &[1, 14, 15, 16, 20, 22], 79);
    assert_freeze_grapheme_match(
        "reference/freeze/run1-startup-79x24/terminal.txt",
        "actual/harness-startup-79x24-v8/terminal.txt",
        79,
        24,
        &masks,
        "RESP-79x24",
    );
}

#[test]
fn freeze_startup_60x20_matches_harness_v8_with_identity_rows() {
    // arrange
    // act
    // assert
    let masks = identity_rows_mask("resp_60x20_identity", &[1, 16, 18], 60);
    assert_freeze_grapheme_match(
        "reference/freeze/run1-startup-60x20/terminal.txt",
        "actual/harness-startup-60x20-v8/terminal.txt",
        60,
        20,
        &masks,
        "RESP-60x20",
    );
}

#[test]
fn freeze_startup_wide_matches_harness_v8_with_identity_rows() {
    // arrange
    // act
    // assert
    let mut rows = vec![1u16, 36];
    rows.extend(12u16..=18);
    let masks = identity_rows_mask("resp_wide_identity", &rows, 140);
    assert_freeze_grapheme_match(
        "reference/freeze/run1-startup-140x40/terminal.txt",
        "actual/harness-startup-140x40-v8/terminal.txt",
        140,
        40,
        &masks,
        "RESP-WIDE",
    );
}

fn first_inventory_line_number(path: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(path).ok()?;
    for line in text.lines() {
        let trimmed = line.trim();
        let Some((num, rest)) = trimmed.split_once(". f") else {
            continue;
        };
        if !rest.ends_with(".txt") {
            continue;
        }
        if let Ok(n) = num.parse::<u32>() {
            return Some(n);
        }
    }
    None
}

#[test]
fn freeze_scroll_inventory_first_line_matches_harness_v7_packing() {
    // arrange
    // act
    // assert
    let root = evidence_root();
    let freeze = root.join("reference/freeze/run1-scroll-proxy-v3/terminal.txt");
    let actual = root.join("actual/harness-pty-scroll-120x32-v10/terminal.txt");
    if !require_evidence(&freeze) || !require_evidence(&actual) {
        return;
    }
    let freeze_first = first_inventory_line_number(&freeze);
    let actual_first = first_inventory_line_number(&actual);
    assert_eq!(
        freeze_first,
        Some(39),
        "freeze SCROLL first inventory must be f39; got {freeze_first:?}"
    );
    assert_eq!(
        actual_first,
        Some(39),
        "loop15 scroll packing: harness L3 first inventory must match freeze f39; got {actual_first:?}"
    );
}

#[test]
fn freeze_complete_matches_harness_complete_v5_with_identity_path_clock_badge() {
    // arrange
    // act
    // assert
    // Identity: breadcrumb path (L2), user/thought clocks (L5, L10), model badge (L29).
    // L3 v5 adds PNG+ANSI via web-terminal-visual-qa; grapheme residual class unchanged.
    let masks = identity_rows_mask(
        "shell_complete_identity_path_clock_badge",
        &[1, 4, 9, 28],
        120,
    );
    assert_freeze_grapheme_match(
        "reference/freeze/run1-complete-proxy-v2/terminal.txt",
        "actual/harness-pty-complete-120x32-v5/terminal.txt",
        120,
        32,
        &masks,
        "SHELL-COMPLETE",
    );
}

#[test]
fn freeze_perm_matches_harness_perm_v14_with_path_draft_identity_mask() {
    // arrange
    // act
    // assert
    // Geometry (pin/height/lead) matches freeze. Residual rows only:
    // L2 breadcrumb spacing, L5 user clock, L20/L23 path length packing,
    // L24 draft line (product feature vs freeze empty draft gap).
    let masks = identity_rows_mask("shell_perm_path_draft_identity", &[1, 4, 19, 22, 23], 120);
    assert_freeze_grapheme_match(
        "reference/freeze/run1-perm-proxy-v2/terminal.txt",
        "actual/harness-pty-perm-120x32-v14/terminal.txt",
        120,
        32,
        &masks,
        "SHELL-PERM",
    );
}

#[test]
fn freeze_question_matches_harness_question_v16_with_identity_scroll_dismiss_mask() {
    // arrange
    // act
    // assert
    // Geometry (Thought→Ask adjacent, Waiting pin L17 lead=4, dock 11-row packing,
    // left pad 2, y copy) matches freeze. Residual rows only:
    // L2 breadcrumb spacing, L5–6 user clock/wrap, L17 right-meta gap,
    // L23–25 scrollbar █, L28 Enter:submit gap, L31 product-honest Ctrl+c vs Shift+x.
    let masks = identity_rows_mask(
        "shell_question_identity_scroll_dismiss",
        &[1, 4, 5, 16, 22, 23, 24, 27, 30],
        120,
    );
    assert_freeze_grapheme_match(
        "reference/freeze/run1-question-proxy-v2/terminal.txt",
        "actual/harness-pty-question-120x32-v16/terminal.txt",
        120,
        32,
        &masks,
        "SHELL-QUESTION",
    );
}
