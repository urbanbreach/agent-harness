use harness_tui::transcript_block_viewer::{
    ViewerBlockContent, ViewerCloseReason, ViewerMode, ViewerReturnSnapshot, ViewerState,
};
use harness_tui::transcript_block_viewer::{render_surface, render_to_buffer};
use harness_tui::transcript_blocks::FoldState;
use harness_tui::transcript_identity::{BlockId, FocusFollowState, ReplayTurn, TranscriptFocus};
use harness_tui::transcript_scroll::{LogicalAnchor, TranscriptLayout};
use harness_tui::transcript_selection::{
    CellPoint, LocalPlatform, NavigationKey, TmuxSequence, Viewport,
};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn block_id() -> BlockId {
    ReplayTurn::event(41, 2, 1).block_id(0)
}

fn return_snapshot(id: BlockId) -> TestResult<ViewerReturnSnapshot> {
    let layout = TranscriptLayout::from_heights([(id, 12.0)], 5.0)?;
    let anchor = LogicalAnchor::capture(&layout, 3.25)?;
    Ok(ViewerReturnSnapshot::new(
        FoldState::Collapsed,
        FocusFollowState::new(TranscriptFocus::Timeline, false),
        anchor,
    ))
}

fn viewer(text: &str) -> TestResult<ViewerState> {
    Ok(ViewerState::open(
        block_id(),
        ViewerBlockContent::new(text, Some(text)),
        return_snapshot(block_id())?,
    )?)
}

#[test]
fn search_wraps_back_and_forward_and_reports_no_result() -> TestResult {
    // Given: a selected block with two Unicode matches.
    let mut viewer = viewer("中 alpha 中")?;

    // When: the query is entered and navigated in both directions.
    let first = viewer.set_search_query("中");
    let forward = viewer.search_forward();
    let forward_wrap = viewer.search_forward();
    let backward_wrap = viewer.search_backward();
    let absent = viewer.set_search_query("missing");

    // Then: count, current match, wrap, and no-result state are explicit.
    assert_eq!(first.match_count, 2);
    assert_eq!(first.current_match, Some(0));
    assert!(!forward.wrapped);
    assert_eq!(forward.current_match, Some(1));
    assert!(forward_wrap.wrapped);
    assert_eq!(forward_wrap.current_match, Some(0));
    assert!(backward_wrap.wrapped);
    assert_eq!(backward_wrap.current_match, Some(1));
    assert!(absent.no_result);
    assert_eq!(absent.match_count, 0);
    Ok(())
}

#[test]
fn search_accepts_complete_unicode_graphemes_and_rejects_partial_ones() -> TestResult {
    // Given: combining text, a ZWJ emoji, and CJK content.
    let mut viewer = viewer("e\u{301} 👩‍💻 中")?;

    // When: complete clusters and an incomplete cluster are searched.
    let combining = viewer.set_search_query("e\u{301}");
    let emoji = viewer.set_search_query("👩‍💻");
    let partial = viewer.set_search_query("\u{301}");

    // Then: complete graphemes match while a combining suffix cannot be split.
    assert_eq!(combining.match_count, 1);
    assert_eq!(emoji.match_count, 1);
    assert!(partial.no_result);
    Ok(())
}

#[test]
fn wrapped_and_raw_modes_route_selection_through_transcript_primitives() -> TestResult {
    // Given: a narrow viewer so the wrapped selection crosses a soft row.
    let mut viewer = viewer("alpha beta\ngamma")?;
    viewer.resize(6, 3)?;

    // When: word, keyboard, and mouse selections are made in each mode.
    let word = viewer.select_word(CellPoint::new(0, 1));
    assert_eq!(viewer.copy_selection_text()?, "alpha");
    assert_eq!(Some(word), viewer.selection());
    viewer.move_cursor(NavigationKey::Right, false);
    viewer.move_cursor(NavigationKey::Right, true);
    let mouse = viewer.mouse_drag(
        CellPoint::new(0, 0),
        CellPoint::new(1, 4),
        Viewport::new(0, 2),
    )?;
    assert_eq!(viewer.copy_selection_text()?, "alpha beta");
    assert_eq!(mouse.focus.row, 1);
    viewer.toggle_mode()?;
    assert_eq!(viewer.mode(), ViewerMode::Raw);
    viewer.select_line(CellPoint::new(2, 1));

    // Then: raw selection remains grapheme-safe and is copied through the same primitive.
    assert_eq!(viewer.copy_selection_text()?, "gamma");
    Ok(())
}

#[test]
fn denied_copy_is_typed_and_non_destructive_for_local_and_osc52_routes() -> TestResult {
    // Given: an active selection.
    let mut viewer = viewer("copy me")?;
    viewer.select_line(CellPoint::new(0, 0));
    let before = viewer.selection();

    // When: both clipboard routes are denied.
    let local = viewer.copy_local_with_runner(
        LocalPlatform::Linux { wayland: false },
        |_command, _text| Ok(false),
    );
    let osc52 = viewer.copy_osc52(false, TmuxSequence::Direct);

    // Then: errors are typed and the selection is unchanged.
    assert!(local.is_err());
    assert!(osc52.is_err());
    assert_eq!(viewer.selection(), before);
    Ok(())
}

#[test]
fn viewer_scroll_is_independent_and_survives_resize_with_anchor_identity() -> TestResult {
    // Given: content taller than the viewer viewport.
    let mut viewer = viewer("one two three four five six seven eight")?;
    viewer.resize(7, 2)?;

    // When: the viewer scrolls, animates, and is resized.
    viewer.scroll_by(3.0)?;
    let before = viewer.scroll_anchor()?;
    viewer.resize(14, 4)?;
    let frame = viewer.sample_scroll(500);

    // Then: viewer scroll state uses its own stable block anchor and settles safely.
    assert_eq!(before.block_id(), block_id());
    assert_eq!(viewer.scroll_anchor()?.block_id(), block_id());
    assert!(frame.value >= 0.0);
    Ok(())
}

#[test]
fn closing_restores_fold_focus_follow_and_anchor_exactly() -> TestResult {
    // Given: an in-place state captured before entering the viewer.
    let snapshot = return_snapshot(block_id())?;
    let viewer = ViewerState::open(
        block_id(),
        ViewerBlockContent::new("selected", None),
        snapshot,
    )?;

    // When: the full-screen viewer closes.
    let closed = viewer.close();

    // Then: the complete return snapshot is byte-for-byte equivalent.
    assert_eq!(closed.reason(), ViewerCloseReason::Closed);
    assert_eq!(closed.return_snapshot(), snapshot);
    Ok(())
}

#[test]
fn disappearing_block_closes_safely_without_losing_return_state() -> TestResult {
    // Given: an open viewer for a currently present block.
    let snapshot = return_snapshot(block_id())?;
    let mut viewer = ViewerState::open(
        block_id(),
        ViewerBlockContent::new("selected", None),
        snapshot,
    )?;

    // When: compaction or deletion reports that the block is gone.
    let closed = viewer
        .reconcile_block(false)
        .ok_or("viewer did not close")?;

    // Then: the viewer is inactive and returns the original state safely.
    assert!(!viewer.is_open());
    assert_eq!(closed.reason(), ViewerCloseReason::BlockDisappeared);
    assert_eq!(closed.return_snapshot(), snapshot);
    Ok(())
}

#[test]
fn wrapped_and_raw_render_surfaces_highlight_current_match_and_selection() -> TestResult {
    // Given: a viewer with a selected line and an active search match.
    let mut viewer = viewer("alpha beta")?;
    viewer.select_line(CellPoint::new(0, 1));
    viewer.set_search_query("beta");
    let area = Rect::new(0, 0, 24, 5);

    // When: both rendering modes are materialized into a terminal buffer.
    let wrapped = render_surface(&viewer, area);
    let mut buffer = Buffer::empty(area);
    render_to_buffer(&mut buffer, area, &wrapped);
    viewer.toggle_mode()?;
    let raw = viewer.render_surface(area);

    // Then: mode labels and match/selection state survive the render boundary.
    assert_eq!(wrapped.mode, ViewerMode::Wrapped);
    assert!(wrapped.lines[0].selected);
    assert!(wrapped.lines[0].current_match);
    assert_eq!(raw.mode, ViewerMode::Raw);
    assert!(buffer.content.iter().any(|cell| cell.symbol() == "b"));
    Ok(())
}
