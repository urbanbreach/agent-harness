//! Unit/integration coverage for harness-testkit parity cell pipeline.
//!
//! Complements crates/harness-tui/tests/reference_parity_cells_test.rs.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "parity cell tests use fail-fast asserts"
)]

use harness_testkit::parity::{
    compare_frames, is_settled, semantic_frame_from_vt100_screen, CellModifiers, CursorState,
    IdentityMaskRegistry, ResolvedRgb, SemanticCell, SemanticFrame, StableFrameTracker,
    SEMANTIC_FRAME_SCHEMA_VERSION,
};

#[test]
fn blank_frame_validates_and_roundtrips_json() {
    let frame = SemanticFrame::new(4, 2, CursorState::visible_block(1, 0));
    frame.validate().expect("valid");
    let json = frame.to_json_value().expect("json");
    assert_eq!(json["schema_version"], SEMANTIC_FRAME_SCHEMA_VERSION);
    assert_eq!(json["cells"].as_array().expect("cells").len(), 8);
    let roundtrip = SemanticFrame::from_json_value(&json).expect("roundtrip");
    assert_eq!(roundtrip, frame);
}

#[test]
fn compare_identical_and_mismatches() {
    let mut a = SemanticFrame::new(3, 1, CursorState::visible_block(0, 0));
    let mut b = a.clone();
    a.set_cell(SemanticCell::blank(0, 0).with_grapheme("x", 1))
        .expect("set");
    b.set_cell(SemanticCell::blank(0, 0).with_grapheme("x", 1))
        .expect("set");
    assert!(compare_frames(&a, &b, &IdentityMaskRegistry::new()).is_ok());

    b.set_cell(SemanticCell::blank(0, 0).with_grapheme("y", 1))
        .expect("set");
    let err = compare_frames(&a, &b, &IdentityMaskRegistry::new()).expect_err("grapheme");
    assert!(err.iter().any(|d| d.path.contains("grapheme")));
}

#[test]
fn settle_helper_needs_three_identical_frames() {
    let f = SemanticFrame::new(2, 1, CursorState::hidden(0, 0));
    let other = {
        let mut frame = f.clone();
        frame
            .set_cell(SemanticCell::blank(0, 0).with_grapheme("!", 1))
            .expect("set");
        frame
    };
    assert!(!is_settled(&[f.clone(), f.clone()]));
    assert!(is_settled(&[f.clone(), f.clone(), f.clone()]));
    assert!(!is_settled(&[f.clone(), f.clone(), other]));

    let mut tracker = StableFrameTracker::new();
    assert!(!tracker.observe(f.clone()));
    assert!(!tracker.observe(f.clone()));
    assert!(tracker.observe(f));
}

#[test]
fn wide_glyph_continuation_and_identity_mask() {
    let mut expected = SemanticFrame::new(3, 1, CursorState::hidden(0, 0));
    expected
        .set_cell(SemanticCell::blank(0, 0).with_grapheme("宽", 2))
        .expect("wide");
    expected
        .set_cell(SemanticCell::blank(0, 1).with_continuation())
        .expect("cont");
    let actual = expected.clone();
    assert!(compare_frames(&expected, &actual, &IdentityMaskRegistry::new()).is_ok());

    let mut titled = SemanticFrame::new(4, 1, CursorState::hidden(0, 0));
    let mut other = titled.clone();
    for (col, ch) in ['A', 'B', 'C', 'D'].into_iter().enumerate() {
        let col = u16::try_from(col).expect("col");
        titled
            .set_cell(
                SemanticCell::blank(0, col)
                    .with_grapheme(ch.to_string(), 1)
                    .with_fg(ResolvedRgb::new(1, 1, 1)),
            )
            .expect("set");
    }
    for (col, ch) in ['W', 'X', 'Y', 'Z'].into_iter().enumerate() {
        let col = u16::try_from(col).expect("col");
        other
            .set_cell(
                SemanticCell::blank(0, col)
                    .with_grapheme(ch.to_string(), 1)
                    .with_fg(ResolvedRgb::new(1, 1, 1)),
            )
            .expect("set");
    }
    let masks =
        IdentityMaskRegistry::new().with_field("product_title_text", [(0, 0), (0, 1), (0, 2), (0, 3)]);
    assert!(compare_frames(&titled, &other, &masks).is_ok());
}

#[test]
fn vt100_adapter_and_modifier_diff() {
    let mut parser = vt100::Parser::new(2, 6, 0);
    parser.process(b"\x1b[1;1HHi");
    let frame = semantic_frame_from_vt100_screen(parser.screen());
    assert_eq!(frame.cell(0, 0).expect("H").grapheme, "H");
    assert_eq!(frame.cell(0, 1).expect("i").grapheme, "i");

    let mut a = SemanticFrame::new(2, 1, CursorState::hidden(0, 0));
    let b = a.clone();
    a.set_cell(
        SemanticCell::blank(0, 0)
            .with_grapheme("m", 1)
            .with_modifiers(CellModifiers {
                bold: true,
                ..CellModifiers::default()
            }),
    )
    .expect("set");
    let err = compare_frames(&a, &b, &IdentityMaskRegistry::new()).expect_err("mod");
    assert!(err.iter().any(|d| d.path.ends_with(".modifiers")));
}
