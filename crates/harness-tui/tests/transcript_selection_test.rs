#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "transcript selection tests use expect and unwrap for concise fixture assertions"
)]

use harness_tui::transcript_selection::{
    build_osc52, copy_local_with_runner, copy_with_metadata, copy_with_metadata_and_links,
    hyperlink_sequence, BlockKind, CellPoint, CopyMetadata, CopyMetadataPolicy, Hyperlink,
    HyperlinkMap, LinkRange, LocalPlatform, NavigationKey, SelectionMode, TmuxSequence, Viewport,
    WrappedText, OSC52_MAX_BYTES,
};

#[test]
fn wrapped_drag_selection_joins_soft_line_breaks() {
    // arrange
    // Given: a narrow wrapped block and a drag from its first to last cell.
    let text = WrappedText::new("hello world", 5).expect("valid width");
    let start = CellPoint::new(0, 0);
    let end = CellPoint::new(1, 4);

    // When: the selected cells are copied.
    let copied = text
        .copy(text.drag(start, end))
        .expect("selection has text");

    // act
    // Then: soft wrapping does not insert a spurious newline.
    // assert
    assert_eq!(copied, "hello world");
}

#[test]
fn cjk_and_emoji_selection_stays_on_grapheme_boundaries() {
    // arrange
    // Given: combining text, a ZWJ emoji, and a wide CJK grapheme.
    let text = WrappedText::new("e\u{301} 👩‍💻 中", 32).expect("valid width");

    // When: a cell inside the emoji is used as the drag endpoint.
    let emoji = text
        .grapheme_at(CellPoint::new(0, 3))
        .expect("emoji exists");
    let copied = text
        .copy(text.drag(CellPoint::new(0, 0), emoji.end))
        .expect("selection has text");

    // act
    // Then: no combining mark, ZWJ, or wide grapheme is split.
    // assert
    assert_eq!(emoji.text, "👩‍💻");
    assert_eq!(copied, "e\u{301} 👩‍💻");
}

#[test]
fn word_and_line_selection_use_visible_rows() {
    // arrange
    // Given: two explicit transcript lines.
    let text = WrappedText::new("alpha beta\ngamma", 32).expect("valid width");

    // When: word and line modes are requested.
    let word = text.select(CellPoint::new(0, 7), SelectionMode::Word);
    let line = text.select(CellPoint::new(1, 2), SelectionMode::Line);

    // act
    // Then: the semantic units are selected, not byte fragments.
    // assert
    assert_eq!(text.copy(word).expect("word exists"), "beta");
    assert_eq!(text.copy(line).expect("line exists"), "gamma");
}

#[test]
fn keyboard_movement_crosses_wraps_without_splitting_graphemes() {
    // arrange
    // Given: a wrapped row ending in a wide grapheme.
    let text = WrappedText::new("a 👩‍💻 b", 4).expect("valid width");

    // When: right movement is repeated from the first cell.
    let first = text.move_focus(CellPoint::new(0, 0), NavigationKey::Right);
    let second = text.move_focus(first, NavigationKey::Right);
    let next_row = text.move_focus(second, NavigationKey::Right);

    // act
    // Then: movement lands on grapheme starts and crosses the soft wrap.
    // assert
    assert_eq!(first, CellPoint::new(0, 1));
    assert_eq!(second, CellPoint::new(0, 2));
    assert_eq!(next_row.row, 1);
}

#[test]
fn drag_to_viewport_edge_reports_autoscroll_without_losing_focus() {
    // arrange
    // Given: a block taller than the visible viewport.
    let text = WrappedText::new("one two three four five", 4).expect("valid width");
    let viewport = Viewport::new(0, 2);

    // When: dragging below the viewport.
    let drag = text.drag_with_autoscroll(CellPoint::new(0, 0), CellPoint::new(4, 3), viewport);

    // act
    // Then: focus remains at the requested row and scrolling is positive.
    // assert
    assert_eq!(drag.focus, CellPoint::new(4, 3));
    assert_eq!(drag.autoscroll.lines, 3);
}

#[test]
fn metadata_copy_includes_only_visible_fields() {
    // arrange
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

    // act
    // Then: only the selected visible fields precede the content.
    // assert
    assert_eq!(copied, "[turn-01] [12:34:56]\nanswer");
}

#[test]
fn osc52_rejects_oversized_payload_and_tmux_wraps_safe_sequence() {
    // arrange
    // Given: a payload over the named protocol limit.
    let oversized = "x".repeat(OSC52_MAX_BYTES + 1);

    // When: it is encoded for OSC52.
    let error = build_osc52(&oversized, TmuxSequence::Direct).expect_err("payload is too large");

    // Then: the typed limit error is returned before encoding.
    assert!(error.is_too_large());

    // act
    let sequence = build_osc52("copy", TmuxSequence::Tmux).expect("small payload");
    // assert
    assert!(sequence.starts_with("\x1bPtmux;\x1b"));
    assert!(sequence.ends_with("\x1b\\"));
}

#[test]
fn denied_clipboard_is_typed_and_does_not_panic() {
    // arrange
    // Given: no local clipboard helper succeeds.
    let result = copy_local_with_runner(
        "copy",
        LocalPlatform::Linux { wayland: false },
        |_command, _text| Ok(false),
    );

    // act
    // When/Then: routing reports denial as a typed error.
    // assert
    assert!(result.expect_err("clipboard is denied").is_denied());
}

#[test]
fn hyperlink_hover_click_and_tmux_osc8_are_sanitized() {
    // arrange
    // Given: one safe link and one control-character URL.
    let link = Hyperlink::new("docs", "https://example.com/docs", LinkRange::new(0, 2, 6))
        .expect("safe URL");
    let links = HyperlinkMap::new(vec![link]);

    // When: hover and click hit the same terminal cells.
    let point = CellPoint::new(0, 4);
    let hovered = links.hover(point).expect("link is hovered");
    let clicked = links.click(point).expect("link is clicked");

    // act
    // Then: both interactions resolve the same sanitized URL and OSC8 is tmux-safe.
    // assert
    assert_eq!(hovered.url(), "https://example.com/docs");
    assert_eq!(clicked.url(), hovered.url());
    let sequence = hyperlink_sequence(&clicked, TmuxSequence::Tmux).expect("safe OSC8");
    assert!(sequence.contains("docs"));
    assert!(sequence.starts_with("\x1bPtmux;"));
    assert!(Hyperlink::new("bad", "https://example.com\n", LinkRange::new(0, 0, 1)).is_err());
}

#[test]
fn external_hyperlinks_allow_only_http_schemes_without_rewriting_valid_urls() {
    // Given: safe web destinations and executable/local URI schemes.
    let range = LinkRange::new(0, 0, 3);

    // When: destinations cross the hyperlink rendering boundary.
    let http = Hyperlink::new("http", "http://example.com/a?b=1#c", range).expect("http URL");
    let https = Hyperlink::new("https", "https://例え.test/資料", range).expect("https URL");

    // Then: valid raw URLs are preserved and unsafe schemes are rejected.
    assert_eq!(http.url(), "http://example.com/a?b=1#c");
    assert_eq!(https.url(), "https://例え.test/資料");
    for unsafe_url in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "ftp://example.com/file",
        "https://example.com/\u{1b}]8;;evil",
    ] {
        assert!(
            Hyperlink::new("unsafe", unsafe_url, range).is_err(),
            "accepted {unsafe_url:?}"
        );
    }
}

#[test]
fn metadata_copy_retains_visible_text_and_selected_link_destinations() {
    // Given: selected visible text, transcript metadata, and one selected destination.
    let metadata = CopyMetadata::new("turn-01", BlockKind::Assistant, "12:34:56");
    let link = Hyperlink::new("docs", "https://example.com/docs", LinkRange::new(0, 0, 3))
        .expect("safe URL");

    // When: copy metadata is assembled for the selection.
    let copied = copy_with_metadata_and_links(
        "Read docs",
        &metadata,
        CopyMetadataPolicy {
            include_turn_id: true,
            include_block_kind: true,
            include_timestamp: true,
        },
        &[link],
    );

    // Then: existing metadata and visible text remain, followed by the destination once.
    assert_eq!(
        copied,
        "[turn-01] [assistant] [12:34:56]\nRead docs\n\nLinks:\nhttps://example.com/docs"
    );
}
