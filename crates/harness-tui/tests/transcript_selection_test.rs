#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "transcript selection tests use expect and unwrap for concise fixture assertions"
)]

use harness_tui::transcript_selection::{
    build_osc52, copy_local_with_runner, copy_with_metadata, hyperlink_sequence, BlockKind,
    CellPoint, CopyMetadata, CopyMetadataPolicy, Hyperlink, HyperlinkMap, LinkRange, LocalPlatform,
    NavigationKey, SelectionMode, TmuxSequence, Viewport, WrappedText, OSC52_MAX_BYTES,
};

#[test]
fn wrapped_drag_selection_joins_soft_line_breaks() {
    // Given: a narrow wrapped block and a drag from its first to last cell.
    let text = WrappedText::new("hello world", 5).expect("valid width");
    let start = CellPoint::new(0, 0);
    let end = CellPoint::new(1, 4);

    // When: the selected cells are copied.
    let copied = text
        .copy(text.drag(start, end))
        .expect("selection has text");

    // Then: soft wrapping does not insert a spurious newline.
    assert_eq!(copied, "hello world");
}

#[test]
fn cjk_and_emoji_selection_stays_on_grapheme_boundaries() {
    // Given: combining text, a ZWJ emoji, and a wide CJK grapheme.
    let text = WrappedText::new("e\u{301} 👩‍💻 中", 32).expect("valid width");

    // When: a cell inside the emoji is used as the drag endpoint.
    let emoji = text
        .grapheme_at(CellPoint::new(0, 3))
        .expect("emoji exists");
    let copied = text
        .copy(text.drag(CellPoint::new(0, 0), emoji.end))
        .expect("selection has text");

    // Then: no combining mark, ZWJ, or wide grapheme is split.
    assert_eq!(emoji.text, "👩‍💻");
    assert_eq!(copied, "e\u{301} 👩‍💻");
}

#[test]
fn word_and_line_selection_use_visible_rows() {
    // Given: two explicit transcript lines.
    let text = WrappedText::new("alpha beta\ngamma", 32).expect("valid width");

    // When: word and line modes are requested.
    let word = text.select(CellPoint::new(0, 7), SelectionMode::Word);
    let line = text.select(CellPoint::new(1, 2), SelectionMode::Line);

    // Then: the semantic units are selected, not byte fragments.
    assert_eq!(text.copy(word).expect("word exists"), "beta");
    assert_eq!(text.copy(line).expect("line exists"), "gamma");
}

#[test]
fn keyboard_movement_crosses_wraps_without_splitting_graphemes() {
    // Given: a wrapped row ending in a wide grapheme.
    let text = WrappedText::new("a 👩‍💻 b", 4).expect("valid width");

    // When: right movement is repeated from the first cell.
    let first = text.move_focus(CellPoint::new(0, 0), NavigationKey::Right);
    let second = text.move_focus(first, NavigationKey::Right);
    let next_row = text.move_focus(second, NavigationKey::Right);

    // Then: movement lands on grapheme starts and crosses the soft wrap.
    assert_eq!(first, CellPoint::new(0, 1));
    assert_eq!(second, CellPoint::new(0, 2));
    assert_eq!(next_row.row, 1);
}

#[test]
fn drag_to_viewport_edge_reports_autoscroll_without_losing_focus() {
    // Given: a block taller than the visible viewport.
    let text = WrappedText::new("one two three four five", 4).expect("valid width");
    let viewport = Viewport::new(0, 2);

    // When: dragging below the viewport.
    let drag = text.drag_with_autoscroll(CellPoint::new(0, 0), CellPoint::new(4, 3), viewport);

    // Then: focus remains at the requested row and scrolling is positive.
    assert_eq!(drag.focus, CellPoint::new(4, 3));
    assert_eq!(drag.autoscroll.lines, 3);
}

#[test]
fn metadata_copy_includes_only_visible_fields() {
    // Given: a visible block with all supported metadata.
    let metadata = CopyMetadata::new("turn-01", BlockKind::Assistant, "12:34:56");

    // When: visible metadata is enabled but block kind is hidden.
    let copied = copy_with_metadata(
        "answer",
        &metadata,
        CopyMetadataPolicy {
            include_turn_id: true,
            include_block_kind: false,
            include_timestamp: true,
        },
    );

    // Then: only the selected visible fields precede the content.
    assert_eq!(copied, "[turn-01] [12:34:56]\nanswer");
}

#[test]
fn osc52_rejects_oversized_payload_and_tmux_wraps_safe_sequence() {
    // Given: a payload over the named protocol limit.
    let oversized = "x".repeat(OSC52_MAX_BYTES + 1);

    // When: it is encoded for OSC52.
    let error = build_osc52(&oversized, TmuxSequence::Direct).expect_err("payload is too large");

    // Then: the typed limit error is returned before encoding.
    assert!(error.is_too_large());

    let sequence = build_osc52("copy", TmuxSequence::Tmux).expect("small payload");
    assert!(sequence.starts_with("\x1bPtmux;\x1b"));
    assert!(sequence.ends_with("\x1b\\"));
}

#[test]
fn denied_clipboard_is_typed_and_does_not_panic() {
    // Given: no local clipboard helper succeeds.
    let result = copy_local_with_runner(
        "copy",
        LocalPlatform::Linux { wayland: false },
        |_command, _text| Ok(false),
    );

    // When/Then: routing reports denial as a typed error.
    assert!(result.expect_err("clipboard is denied").is_denied());
}

#[test]
fn hyperlink_hover_click_and_tmux_osc8_are_sanitized() {
    // Given: one safe link and one control-character URL.
    let link = Hyperlink::new("docs", "https://example.com/docs", LinkRange::new(0, 2, 6))
        .expect("safe URL");
    let links = HyperlinkMap::new(vec![link]);

    // When: hover and click hit the same terminal cells.
    let point = CellPoint::new(0, 4);
    let hovered = links.hover(point).expect("link is hovered");
    let clicked = links.click(point).expect("link is clicked");

    // Then: both interactions resolve the same sanitized URL and OSC8 is tmux-safe.
    assert_eq!(hovered.url(), "https://example.com/docs");
    assert_eq!(clicked.url(), hovered.url());
    let sequence = hyperlink_sequence(&clicked, TmuxSequence::Tmux).expect("safe OSC8");
    assert!(sequence.contains("docs"));
    assert!(sequence.starts_with("\x1bPtmux;"));
    assert!(Hyperlink::new("bad", "https://example.com\n", LinkRange::new(0, 0, 1)).is_err());
}
