//! Identity comparator mutation tests (Task 25).
//!
//! These synthetic-frame unit tests prove the comparator rejects one border
//! cell mutation, one spacing cell
//! mutation, one color mutation, one cursor position mutation, and one
//! non-identity glyph mutation. Also proves identity-only substitutions pass
//! only when declared bounds and geometry remain exact.
//!
//! Additionally covers the clean-room parity identity substitution rules
//! (Wave 0, Todo 6): logo, product title, version, and truthful
//! provider/account text normalize into generic placeholders while
//! functional content is never over-replaced.
//!
//! They are not reference-vs-Harness rendering evidence. That contract is
//! exercised by the PTY/xterm capture and frozen-evidence suites.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity comparator tests use fail-fast asserts"
)]

use harness_testkit::parity::{
    compare_frames, compare_frames_with_provenance, frame_line, CaptureSource, CellModifiers,
    CursorState, IdentityKind, IdentityMaskRegistry, IdentityReplacement, IdentitySubstitution,
    ResolvedRgb, SemanticCell, SemanticFrame,
};

/// Build a representative idle-shell frame at 120x50 with the measured
/// reference geometry: breadcrumb row 2, composer rows 45-47, footer row 49.
fn reference_idle_frame_120x50() -> SemanticFrame {
    let cols: u16 = 120;
    let rows: u16 = 50;
    let cursor = CursorState {
        row: 46,
        col: 7,
        visible: true,
        shape: harness_testkit::parity::CursorShape::Block,
    };
    let mut frame = SemanticFrame::new(cols, rows, cursor);
    frame.alternate_screen = true;

    let dim = ResolvedRgb::new(102, 102, 102); // index 8
    let white = ResolvedRgb::new(229, 229, 229); // index 7
    let black = ResolvedRgb::new(0, 0, 0); // index 0

    // Breadcrumb row 2 (0-indexed row 1): "  ui-ux-experiments ~/Projects/agent-harness  1.7K / 262K"
    let breadcrumb = "  \u{e0a0} ui-ux-experiments ~/Projects/agent-harness";
    for (i, ch) in breadcrumb.chars().enumerate() {
        let col = u16::try_from(i).expect("col fits");
        let cell = SemanticCell::blank(1, col)
            .with_grapheme(ch.to_string(), 1)
            .with_fg(dim)
            .with_bg(black)
            .with_modifiers(CellModifiers {
                dim: true,
                ..CellModifiers::default()
            });
        frame.set_cell(cell).expect("set breadcrumb");
    }

    // Composer rows 45-47 (0-indexed 44-46)
    // Top border: "  ╭───...───╮"
    frame
        .set_cell(
            SemanticCell::blank(44, 2)
                .with_grapheme("\u{256d}", 1) // ╭
                .with_fg(dim)
                .with_bg(black),
        )
        .expect("set");
    for col in 3..117 {
        frame
            .set_cell(
                SemanticCell::blank(44, col)
                    .with_grapheme("\u{2500}", 1) // ─
                    .with_fg(dim)
                    .with_bg(black),
            )
            .expect("set");
    }
    frame
        .set_cell(
            SemanticCell::blank(44, 117)
                .with_grapheme("\u{256e}", 1) // ╮
                .with_fg(dim)
                .with_bg(black),
        )
        .expect("set");

    // Content row: "  │ ❯  ...  │"
    frame
        .set_cell(
            SemanticCell::blank(45, 2)
                .with_grapheme("\u{2502}", 1) // │
                .with_fg(dim)
                .with_bg(black),
        )
        .expect("set");
    frame
        .set_cell(
            SemanticCell::blank(45, 4)
                .with_grapheme("\u{276f}", 1) // ❯
                .with_fg(white)
                .with_bg(black),
        )
        .expect("set");
    frame
        .set_cell(
            SemanticCell::blank(45, 117)
                .with_grapheme("\u{2502}", 1) // │
                .with_fg(dim)
                .with_bg(black),
        )
        .expect("set");

    // Bottom border: "  ╰───...─── model-badge ─╯"
    frame
        .set_cell(
            SemanticCell::blank(46, 2)
                .with_grapheme("\u{2570}", 1) // ╰
                .with_fg(dim)
                .with_bg(black),
        )
        .expect("set");
    for col in 3..117 {
        frame
            .set_cell(
                SemanticCell::blank(46, col)
                    .with_grapheme("\u{2500}", 1) // ─
                    .with_fg(dim)
                    .with_bg(black),
            )
            .expect("set");
    }
    frame
        .set_cell(
            SemanticCell::blank(46, 117)
                .with_grapheme("\u{256f}", 1) // ╯
                .with_fg(dim)
                .with_bg(black),
        )
        .expect("set");

    // Footer row 49 (0-indexed 48): "  Shift+Tab:mode  │  Ctrl+x:shortcuts"
    let footer = "  Shift+Tab:mode  \u{2502}  Ctrl+x:shortcuts";
    for (i, ch) in footer.chars().enumerate() {
        let col = u16::try_from(i).expect("col fits");
        frame
            .set_cell(
                SemanticCell::blank(48, col)
                    .with_grapheme(ch.to_string(), 1)
                    .with_fg(white)
                    .with_bg(black),
            )
            .expect("set footer");
    }

    frame
}

/// Identity mask for the model badge text in the composer bottom border.
/// This is an identity field: Harness may substitute the model name text
/// while keeping the same cell span and geometry.
fn model_badge_mask() -> IdentityMaskRegistry {
    // Model badge occupies cells in the bottom border row (row 46, 0-indexed)
    // roughly cols 60-116 (right-aligned before ╯).
    let cells: Vec<(u16, u16)> = (60..=116).map(|col| (46, col)).collect();
    IdentityMaskRegistry::new().with_field("model_badge_text", cells)
}

/// Identity mask for breadcrumb path and token usage.
fn breadcrumb_mask() -> IdentityMaskRegistry {
    let cells: Vec<(u16, u16)> = (0..120).map(|col| (1, col)).collect();
    IdentityMaskRegistry::new().with_field("breadcrumb_path_token", cells)
}

// ---------------------------------------------------------------------------
// Mutation rejection tests
// ---------------------------------------------------------------------------

#[test]
fn comparator_rejects_border_cell_mutation() {
    // arrange: reference frame and a copy with one border cell changed
    let expected = reference_idle_frame_120x50();
    let mut actual = expected.clone();

    // Mutate: change the ╭ corner to a different glyph (┌)
    actual
        .set_cell(
            SemanticCell::blank(44, 2)
                .with_grapheme("\u{250c}", 1) // ┌ (sharp corner, wrong)
                .with_fg(ResolvedRgb::new(102, 102, 102))
                .with_bg(ResolvedRgb::new(0, 0, 0)),
        )
        .expect("set");

    // act: compare with identity masks (border is NOT an identity field)
    let masks = model_badge_mask();
    let result = compare_frames(&expected, &actual, &masks);

    // assert: must fail
    let errs = result.expect_err("border mutation must be rejected");
    assert!(
        errs.iter().any(|d| d.path.contains("grapheme")),
        "border cell grapheme diff must be reported: {errs:?}"
    );
}

#[test]
fn comparator_rejects_spacing_cell_mutation() {
    // arrange: reference frame and a copy with a spacing cell changed
    let expected = reference_idle_frame_120x50();
    let mut actual = expected.clone();

    // Mutate: change a blank spacing cell (row 3, col 0) to a visible char
    actual
        .set_cell(
            SemanticCell::blank(2, 0)
                .with_grapheme("X", 1)
                .with_fg(ResolvedRgb::new(229, 229, 229))
                .with_bg(ResolvedRgb::new(0, 0, 0)),
        )
        .expect("set");

    // act
    let masks = model_badge_mask();
    let result = compare_frames(&expected, &actual, &masks);

    // assert
    let errs = result.expect_err("spacing mutation must be rejected");
    assert!(
        errs.iter().any(|d| d.path.contains("grapheme")),
        "spacing cell grapheme diff must be reported: {errs:?}"
    );
}

#[test]
fn comparator_rejects_color_mutation() {
    // arrange: reference frame and a copy with one cell's color changed
    let expected = reference_idle_frame_120x50();
    let mut actual = expected.clone();

    // Mutate: change the ❯ prompt glyph color from white to red
    actual
        .set_cell(
            SemanticCell::blank(45, 4)
                .with_grapheme("\u{276f}", 1) // ❯
                .with_fg(ResolvedRgb::new(241, 76, 76)) // red instead of white
                .with_bg(ResolvedRgb::new(0, 0, 0)),
        )
        .expect("set");

    // act: even with identity masks, color is never exempt
    let masks = model_badge_mask();
    let result = compare_frames(&expected, &actual, &masks);

    // assert
    let errs = result.expect_err("color mutation must be rejected");
    assert!(
        errs.iter().any(|d| d.path.ends_with(".fg")),
        "color diff must be reported: {errs:?}"
    );
}

#[test]
fn comparator_rejects_cursor_position_mutation() {
    // arrange: reference frame and a copy with cursor moved
    let expected = reference_idle_frame_120x50();
    let mut actual = expected.clone();

    // Mutate: move cursor from (46, 7) to (46, 8)
    actual.cursor.col = 8;

    // act
    let masks = model_badge_mask();
    let result = compare_frames(&expected, &actual, &masks);

    // assert
    let errs = result.expect_err("cursor mutation must be rejected");
    assert!(
        errs.iter().any(|d| d.path.contains("cursor")),
        "cursor position diff must be reported: {errs:?}"
    );
}

#[test]
fn comparator_rejects_non_identity_glyph_mutation() {
    // arrange: reference frame and a copy with a non-identity glyph changed
    let expected = reference_idle_frame_120x50();
    let mut actual = expected.clone();

    // Mutate: change the footer separator │ to a different character
    // The footer │ at row 48 is NOT an identity field
    actual
        .set_cell(
            SemanticCell::blank(48, 18)
                .with_grapheme("|", 1) // ASCII pipe instead of box-drawing │
                .with_fg(ResolvedRgb::new(229, 229, 229))
                .with_bg(ResolvedRgb::new(0, 0, 0)),
        )
        .expect("set");

    // act
    let masks = model_badge_mask();
    let result = compare_frames(&expected, &actual, &masks);

    // assert
    let errs = result.expect_err("non-identity glyph mutation must be rejected");
    assert!(
        errs.iter().any(|d| d.path.contains("grapheme")),
        "non-identity glyph diff must be reported: {errs:?}"
    );
}

// ---------------------------------------------------------------------------
// Identity substitution acceptance tests
// ---------------------------------------------------------------------------

#[test]
fn identity_model_badge_substitution_passes_when_geometry_exact() {
    // arrange: reference frame and a copy with model badge text changed
    let expected = reference_idle_frame_120x50();
    let mut actual = expected.clone();

    // Substitute: change model badge text (identity field)
    // The model badge is in the bottom border row (row 46), right-aligned
    for col in 60..=80 {
        actual
            .set_cell(
                SemanticCell::blank(46, col)
                    .with_grapheme("H", 1) // Harness substitution
                    .with_fg(ResolvedRgb::new(102, 102, 102))
                    .with_bg(ResolvedRgb::new(0, 0, 0)),
            )
            .expect("set");
    }

    // act: compare with model badge identity mask
    let masks = model_badge_mask();
    let result = compare_frames(&expected, &actual, &masks);

    // assert: must pass — only grapheme differs under the mask
    assert!(
        result.is_ok(),
        "identity model badge substitution must pass when geometry exact: {result:?}"
    );
}

#[test]
fn identity_breadcrumb_substitution_passes_when_geometry_exact() {
    // arrange
    let expected = reference_idle_frame_120x50();
    let mut actual = expected.clone();

    // Substitute: change breadcrumb path text (identity field)
    for col in 3..40 {
        actual
            .set_cell(
                SemanticCell::blank(1, col)
                    .with_grapheme("X", 1)
                    .with_fg(ResolvedRgb::new(102, 102, 102))
                    .with_bg(ResolvedRgb::new(0, 0, 0))
                    .with_modifiers(CellModifiers {
                        dim: true,
                        ..CellModifiers::default()
                    }),
            )
            .expect("set");
    }

    // act
    let masks = breadcrumb_mask();
    let result = compare_frames(&expected, &actual, &masks);

    // assert
    assert!(
        result.is_ok(),
        "identity breadcrumb substitution must pass when geometry exact: {result:?}"
    );
}

#[test]
fn identity_substitution_fails_when_color_also_changes() {
    // arrange: identity text changed AND color changed under the mask
    let expected = reference_idle_frame_120x50();
    let mut actual = expected.clone();

    // Substitute model badge text AND change its color
    actual
        .set_cell(
            SemanticCell::blank(46, 60)
                .with_grapheme("H", 1)
                .with_fg(ResolvedRgb::new(255, 0, 0)) // wrong color
                .with_bg(ResolvedRgb::new(0, 0, 0)),
        )
        .expect("set");

    // act
    let masks = model_badge_mask();
    let result = compare_frames(&expected, &actual, &masks);

    // assert: must fail — color is never exempt
    let errs = result.expect_err("color under identity mask must fail");
    assert!(
        errs.iter().any(|d| d.path.ends_with(".fg")),
        "color diff under mask must be reported: {errs:?}"
    );
}

#[test]
fn identity_substitution_fails_when_geometry_changes() {
    // arrange: identity text changed AND composer width changed
    let expected = reference_idle_frame_120x50();
    let mut actual = expected.clone();

    // Change model badge text (identity field)
    for col in 60..=80 {
        actual
            .set_cell(
                SemanticCell::blank(46, col)
                    .with_grapheme("H", 1)
                    .with_fg(ResolvedRgb::new(102, 102, 102))
                    .with_bg(ResolvedRgb::new(0, 0, 0)),
            )
            .expect("set");
    }

    // Also change a border cell position (non-identity geometry)
    actual
        .set_cell(
            SemanticCell::blank(44, 117)
                .with_grapheme("\u{2502}", 1) // │ instead of ╮
                .with_fg(ResolvedRgb::new(102, 102, 102))
                .with_bg(ResolvedRgb::new(0, 0, 0)),
        )
        .expect("set");

    // act
    let masks = model_badge_mask();
    let result = compare_frames(&expected, &actual, &masks);

    // assert: must fail — geometry change is not exempt
    let errs = result.expect_err("geometry change with identity must fail");
    assert!(
        errs.iter().any(|d| d.path.contains("grapheme")),
        "border geometry diff must be reported: {errs:?}"
    );
}

// ---------------------------------------------------------------------------
// Provenance-gated comparison tests
// ---------------------------------------------------------------------------

#[test]
fn paired_comparator_rejects_self_oracle() {
    // arrange: both frames from Reference source
    let expected = reference_idle_frame_120x50();
    let actual = expected.clone();
    let masks = IdentityMaskRegistry::new();

    // act
    let result = compare_frames_with_provenance(
        &expected,
        CaptureSource::Reference,
        &actual,
        CaptureSource::Reference,
        &masks,
    );

    // assert: self-comparison must be rejected
    let errs = result.expect_err("self-oracle must be rejected");
    assert!(
        errs.iter()
            .any(|d| d.path.contains("provenance.self_oracle")),
        "self-oracle rejection expected: {errs:?}"
    );
}

#[test]
fn paired_comparator_accepts_cross_source_identical() {
    // arrange: identical frames from different sources
    let expected = reference_idle_frame_120x50();
    let actual = expected.clone();
    let masks = IdentityMaskRegistry::new();

    // act
    let result = compare_frames_with_provenance(
        &expected,
        CaptureSource::Reference,
        &actual,
        CaptureSource::Harness,
        &masks,
    );

    // assert
    assert!(
        result.is_ok(),
        "cross-source identical frames must pass: {result:?}"
    );
}

#[test]
fn paired_comparator_rejects_cross_source_border_mutation() {
    // arrange: reference vs harness with border mutation
    let expected = reference_idle_frame_120x50();
    let mut actual = expected.clone();
    actual
        .set_cell(
            SemanticCell::blank(44, 2)
                .with_grapheme("\u{250c}", 1) // ┌ instead of ╭
                .with_fg(ResolvedRgb::new(102, 102, 102))
                .with_bg(ResolvedRgb::new(0, 0, 0)),
        )
        .expect("set");
    let masks = IdentityMaskRegistry::new();

    // act
    let result = compare_frames_with_provenance(
        &expected,
        CaptureSource::Reference,
        &actual,
        CaptureSource::Harness,
        &masks,
    );

    // assert
    let errs = result.expect_err("cross-source border mutation must be rejected");
    assert!(
        errs.iter().any(|d| d.path.contains("grapheme")),
        "border diff must be reported: {errs:?}"
    );
}

// ---------------------------------------------------------------------------
// Dimension mismatch tests
// ---------------------------------------------------------------------------

#[test]
fn comparator_rejects_dimension_mismatch() {
    // arrange: different viewport dimensions
    let expected = reference_idle_frame_120x50();
    let actual = SemanticFrame::new(100, 40, CursorState::visible_block(36, 7));

    // act
    let result = compare_frames(&expected, &actual, &IdentityMaskRegistry::new());

    // assert
    let errs = result.expect_err("dimension mismatch must be rejected");
    assert!(
        errs.iter().any(|d| d.path == "dimensions"),
        "dimension diff must be reported: {errs:?}"
    );
}

#[test]
fn comparator_rejects_alternate_screen_mismatch() {
    // arrange: same frame but different alternate_screen flag
    let expected = reference_idle_frame_120x50();
    let mut actual = expected.clone();
    actual.alternate_screen = false;

    // act
    let result = compare_frames(&expected, &actual, &IdentityMaskRegistry::new());

    // assert
    let errs = result.expect_err("alternate screen mismatch must be rejected");
    assert!(
        errs.iter().any(|d| d.path == "alternate_screen"),
        "alternate_screen diff must be reported: {errs:?}"
    );
}

#[test]
fn comparator_rejects_cursor_visibility_mutation() {
    // arrange: cursor hidden vs visible
    let expected = reference_idle_frame_120x50();
    let mut actual = expected.clone();
    actual.cursor.visible = false;

    // act
    let result = compare_frames(&expected, &actual, &IdentityMaskRegistry::new());

    // assert
    let errs = result.expect_err("cursor visibility mutation must be rejected");
    assert!(
        errs.iter().any(|d| d.path == "cursor.visible"),
        "cursor.visible diff must be reported: {errs:?}"
    );
}

// ---------------------------------------------------------------------------
// Clean-room identity substitution tests (Wave 0, Todo 6)
//
// Identity substitution is bounded to: the Harness logo, the product title,
// the build version, and truthful provider/account text. Substitution must
// be truthful: only identity spans normalize into generic placeholders and
// functional content is never over-replaced. Geometry and color remain
// enforced by the cell comparator above; these tests cover the text seam.
// ---------------------------------------------------------------------------

/// Registry configured with representative clean-room identity tokens.
fn clean_room_substitution() -> IdentitySubstitution {
    IdentitySubstitution::new()
        .with_logo("\u{2b22}") // ⬢
        .with_product_title("Harness")
        .with_provider("umans-ai-coding-plan")
        .with_account("dev@example.com")
        .with_version()
}

/// Registry normalizing both the reference product's identity and the
/// Harness product's identity into the same placeholders, as a paired
/// comparison would.
fn paired_substitution() -> IdentitySubstitution {
    IdentitySubstitution::new()
        .with_logo("\u{2b22}")
        .with_product_title("Harness")
        .with_product_title("Codex")
        .with_provider("umans-ai-coding-plan")
        .with_provider("openai")
        .with_account("dev@example.com")
        .with_account("user@ref.io")
        .with_version()
}

/// Put `text` into `frame` at (row, col), one narrow cell per grapheme.
fn put_text(frame: &mut SemanticFrame, row: u16, col: u16, text: &str) {
    for (offset, ch) in text.chars().enumerate() {
        let cell_col = col + u16::try_from(offset).expect("col fits");
        frame
            .set_cell(
                SemanticCell::blank(row, cell_col)
                    .with_grapheme(ch.to_string(), 1)
                    .with_fg(ResolvedRgb::new(229, 229, 229))
                    .with_bg(ResolvedRgb::new(0, 0, 0)),
            )
            .expect("set text");
    }
}

#[test]
fn identity_substitution_normalizes_product_title() {
    // arrange
    let sub = clean_room_substitution();

    // act
    let (out, reps) = sub.normalize_detailed("Harness build agent");

    // assert
    assert_eq!(out, "[PRODUCT] build agent");
    assert_eq!(reps.len(), 1);
    assert_eq!(reps[0].kind, IdentityKind::ProductTitle);
    assert_eq!(reps[0].original, "Harness");
}

#[test]
fn identity_substitution_normalizes_logo_glyph_and_sequence() {
    // arrange
    let sub = clean_room_substitution();

    // act: single distinctive logo glyph
    let (out, reps) = sub.normalize_detailed("\u{2b22} status");

    // assert
    assert_eq!(out, "[LOGO] status");
    assert_eq!(reps.len(), 1);
    assert_eq!(reps[0].kind, IdentityKind::Logo);
    assert_eq!(reps[0].original, "\u{2b22}");

    // act: a multi-grapheme logo sequence substitutes as one span
    let pair = IdentitySubstitution::new().with_logo("\u{2b22}\u{2b21}");

    // assert
    assert_eq!(pair.normalize("\u{2b22}\u{2b21} status"), "[LOGO] status");
}

#[test]
fn identity_substitution_normalizes_version_forms() {
    // arrange
    let sub = clean_room_substitution();

    // act + assert: every SemVer-ish form normalizes to [VERSION]
    for raw in [
        "v1.2.3",
        "1.2.3",
        "v1.2.3-rc.1",
        "1.2.3+build.5",
        "V0.10.4-beta.2",
    ] {
        assert_eq!(
            sub.normalize(raw),
            "[VERSION]",
            "version form {raw} must normalize to [VERSION]"
        );
    }
}

#[test]
fn identity_substitution_normalizes_provider_and_account() {
    // arrange
    let sub = clean_room_substitution();

    // act
    let (out, reps) =
        sub.normalize_detailed("signed in to umans-ai-coding-plan as dev@example.com");

    // assert
    assert_eq!(out, "signed in to [PROVIDER] as [ACCOUNT]");
    assert_eq!(
        reps.iter().map(|rep| rep.kind).collect::<Vec<_>>(),
        vec![IdentityKind::Provider, IdentityKind::Account]
    );
}

#[test]
fn identity_substitution_normalizes_multiple_spans_in_one_line() {
    // arrange
    let sub = clean_room_substitution();
    let line = "\u{2b22} Harness v1.2.3 | umans-ai-coding-plan | dev@example.com";

    // act
    let (out, reps) = sub.normalize_detailed(line);

    // assert
    assert_eq!(out, "[LOGO] [PRODUCT] [VERSION] | [PROVIDER] | [ACCOUNT]");
    assert_eq!(reps.len(), 5);
}

#[test]
fn identity_substitution_leaves_words_containing_a_token_untouched() {
    // arrange
    let sub = clean_room_substitution();

    // act + assert: a case mismatch alone protects a larger word ...
    assert_eq!(
        sub.normalize("harnessing the Harness power"),
        "harnessing the [PRODUCT] power"
    );
    // ... and word boundaries still protect an exact-case embedding.
    assert_eq!(sub.normalize("Harnesses remain"), "Harnesses remain");

    // arrange: the exact lowercase token registered
    let lower = IdentitySubstitution::new().with_product_title("harness");

    // act + assert: `harness` inside `harnessing` is never replaced
    assert_eq!(
        lower.normalize("harnessing a harness safely"),
        "harnessing a [PRODUCT] safely"
    );
}

#[test]
fn identity_substitution_leaves_functional_numbers_and_addresses_untouched() {
    // arrange
    let sub = clean_room_substitution();

    // act + assert: token counts, IPs/ports, short and four-component
    // numbers, and embedded spans are functional, never identity.
    assert_eq!(sub.normalize("  1.7K / 262K"), "  1.7K / 262K");
    assert_eq!(
        sub.normalize("connected to 10.0.0.1:8080"),
        "connected to 10.0.0.1:8080"
    );
    assert_eq!(sub.normalize("chapter 1.2 notes"), "chapter 1.2 notes");
    assert_eq!(sub.normalize("release 1.2.3.4"), "release 1.2.3.4");
    assert_eq!(
        sub.normalize("embedded v1.2.3x here"),
        "embedded v1.2.3x here"
    );
}

#[test]
fn identity_substitution_passes_through_functional_text_unchanged() {
    // arrange
    let sub = clean_room_substitution();
    let footer = "  Shift+Tab:mode  \u{2502}  Ctrl+x:shortcuts";

    // act
    let (out, reps) = sub.normalize_detailed(footer);

    // assert
    assert_eq!(out, footer);
    assert!(reps.is_empty(), "no identity span expected: {reps:?}");
}

#[test]
fn identity_substitution_is_opt_in_per_category() {
    // arrange + act + assert: unregistered categories are inert
    let title_only = IdentitySubstitution::new().with_product_title("Harness");
    assert_eq!(title_only.normalize("Harness v1.2.3"), "[PRODUCT] v1.2.3");

    let version_only = IdentitySubstitution::new().with_version();
    assert_eq!(
        version_only.normalize("Harness v1.2.3"),
        "Harness [VERSION]"
    );

    // act + assert: an empty registry is the exact identity function
    let none = IdentitySubstitution::new();
    assert_eq!(none.normalize("Harness v1.2.3"), "Harness v1.2.3");
}

#[test]
fn identity_substitution_records_exact_spans() {
    // arrange
    let sub = clean_room_substitution();

    // act
    let (out, reps) = sub.normalize_detailed("ab Harness cd");

    // assert
    assert_eq!(out, "ab [PRODUCT] cd");
    assert_eq!(reps.len(), 1);
    assert_eq!(reps[0].start_byte, 3);
    assert_eq!(reps[0].end_byte, 10);
    assert_eq!(reps[0].original, "Harness");
}

#[test]
fn identity_substitution_normalizes_frame_identity_text_to_parity() {
    // arrange: reference capture with the reference product's identity
    let cols = 44;
    let mut reference = SemanticFrame::new(cols, 3, CursorState::visible_block(0, 0));
    put_text(&mut reference, 0, 0, "\u{2b22} Codex v5.5.0");
    put_text(&mut reference, 1, 0, "openai user@ref.io");
    put_text(&mut reference, 2, 0, "Shift+Tab:mode");

    // arrange: Harness capture with Harness identity, same functional UI
    let mut harness = SemanticFrame::new(cols, 3, CursorState::visible_block(0, 0));
    put_text(&mut harness, 0, 0, "\u{2b22} Harness v1.2.3");
    put_text(&mut harness, 1, 0, "umans-ai-coding-plan dev@example.com");
    put_text(&mut harness, 2, 0, "Shift+Tab:mode");

    // assert: raw captures differ on identity text
    assert_ne!(frame_line(&reference, 0), frame_line(&harness, 0));
    assert_ne!(frame_line(&reference, 1), frame_line(&harness, 1));

    // act
    let sub = paired_substitution();

    // assert: identity normalization proves text parity
    assert_eq!(
        sub.normalize_frame_lines(&reference),
        sub.normalize_frame_lines(&harness)
    );
    assert_eq!(
        sub.normalize_frame_lines(&harness),
        vec![
            "[LOGO] [PRODUCT] [VERSION]".to_owned(),
            "[PROVIDER] [ACCOUNT]".to_owned(),
            "Shift+Tab:mode".to_owned(),
        ]
    );
}

#[test]
fn identity_substitution_still_exposes_functional_frame_drift() {
    // arrange: captures agree on identity rows but disagree functionally
    let cols = 44;
    let mut reference = SemanticFrame::new(cols, 3, CursorState::visible_block(0, 0));
    put_text(&mut reference, 0, 0, "\u{2b22} Codex v5.5.0");
    put_text(&mut reference, 1, 0, "openai user@ref.io");
    put_text(&mut reference, 2, 0, "Shift+Tab:mode");

    let mut harness = SemanticFrame::new(cols, 3, CursorState::visible_block(0, 0));
    put_text(&mut harness, 0, 0, "\u{2b22} Harness v1.2.3");
    put_text(&mut harness, 1, 0, "umans-ai-coding-plan dev@example.com");
    // Functional drift: a different keybinding label is not identity text.
    put_text(&mut harness, 2, 0, "Ctrl+x:shortcuts");

    // act
    let sub = paired_substitution();
    let reference_rows = sub.normalize_frame_lines(&reference);
    let harness_rows = sub.normalize_frame_lines(&harness);

    // assert: normalization never hides functional drift
    assert_ne!(reference_rows, harness_rows);
    assert_eq!(reference_rows[0], harness_rows[0]);
    assert_eq!(reference_rows[1], harness_rows[1]);
    assert_eq!(reference_rows[2], "Shift+Tab:mode");
    assert_eq!(harness_rows[2], "Ctrl+x:shortcuts");
}
