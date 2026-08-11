use harness_tui::dashboard::SelectionKey;
use harness_tui::dashboard_peek::{DashboardPeek, PeekLimits, PeekSessionStatus, TailUpdateKind};
use harness_tui::transcript_blocks::{BlockKind, BlockLifecycle, BlockSnapshot, FoldState};
use harness_tui::transcript_identity::BlockId;
use harness_tui::transcript_scroll::{FollowMode, TranscriptLayout};
use harness_tui::UnwrapOrAbort;

fn key(value: &str) -> SelectionKey {
    SelectionKey::try_new(value.to_string()).unwrap_or_abort()
}

fn block(sequence: u64, content: &str) -> BlockSnapshot {
    BlockSnapshot {
        id: BlockId::from_replay(sequence, 0, 0),
        kind: BlockKind::Assistant,
        lifecycle: BlockLifecycle::Streaming,
        content: content.to_string(),
        fold_state: FoldState::Expanded,
        raw: None,
    }
}

fn layout(blocks: &[BlockSnapshot], viewport_extent: f64) -> TranscriptLayout {
    TranscriptLayout::from_heights(
        blocks.iter().map(|snapshot| (snapshot.id, 10.0)),
        viewport_extent,
    )
    .unwrap_or_abort()
}

#[test]
fn dashboard_peek_preserves_each_session_draft_when_switching() {
    let mut peek = DashboardPeek::new(10.0).unwrap_or_abort();
    let session_a = key("session-a");
    let session_b = key("session-b");
    peek.sync_sessions(&[session_a.clone(), session_b.clone()])
        .unwrap_or_abort();

    peek.select(&session_a).unwrap_or_abort();
    peek.set_draft("draft a").unwrap_or_abort();
    peek.select(&session_b).unwrap_or_abort();
    peek.set_draft("draft b").unwrap_or_abort();

    assert_eq!(peek.draft_for(&session_a), Some("draft a"));
    assert_eq!(peek.draft_for(&session_b), Some("draft b"));
    assert_eq!(peek.view_for(&session_a).unwrap_or_abort().draft, "draft a");
    assert_eq!(peek.view().unwrap_or_abort().session_id, session_b);
}

#[test]
fn dashboard_peek_restores_detached_scroll_per_session() {
    let mut peek = DashboardPeek::new(10.0).unwrap_or_abort();
    let session_a = key("session-a");
    let session_b = key("session-b");
    let blocks = vec![block(1, "one"), block(2, "two"), block(3, "three")];
    peek.sync_sessions(&[session_a.clone(), session_b.clone()])
        .unwrap_or_abort();
    peek.replace_blocks(&session_a, &blocks).unwrap_or_abort();
    peek.set_layout(&session_a, layout(&blocks, 10.0))
        .unwrap_or_abort();
    peek.select(&session_a).unwrap_or_abort();
    peek.scroll_by(5.0).unwrap_or_abort();
    let before_switch = peek.view().unwrap_or_abort().scroll_top;

    peek.select(&session_b).unwrap_or_abort();
    peek.select(&session_a).unwrap_or_abort();
    let restored = peek.view().unwrap_or_abort();
    assert_eq!(restored.follow, FollowMode::Detached);
    assert!((restored.scroll_top - before_switch).abs() < 1e-9);
}

#[test]
fn dashboard_peek_reports_incremental_append_without_rebuild() {
    let mut peek = DashboardPeek::new(10.0).unwrap_or_abort();
    let session = key("session-a");
    peek.sync_sessions(std::slice::from_ref(&session))
        .unwrap_or_abort();
    peek.replace_blocks(&session, &[block(1, "first")])
        .unwrap_or_abort();

    let update = peek
        .append_block(&session, block(2, "second"))
        .unwrap_or_abort();
    assert_eq!(update.kind, TailUpdateKind::Incremental);
    assert_eq!(update.appended, 1);
    assert_eq!(update.replaced, 0);
    assert_eq!(peek.view_for(&session).unwrap_or_abort().blocks.len(), 2);
}

#[test]
fn dashboard_peek_tracks_unread_while_detached_and_clears_at_bottom() {
    let mut peek = DashboardPeek::new(10.0).unwrap_or_abort();
    let session = key("session-a");
    let blocks = vec![block(1, "one"), block(2, "two"), block(3, "three")];
    peek.sync_sessions(std::slice::from_ref(&session))
        .unwrap_or_abort();
    peek.replace_blocks(&session, &blocks).unwrap_or_abort();
    peek.set_layout(&session, layout(&blocks, 10.0))
        .unwrap_or_abort();
    peek.select(&session).unwrap_or_abort();
    peek.scroll_by(5.0).unwrap_or_abort();
    peek.append_block(&session, block(4, "four"))
        .unwrap_or_abort();

    assert_eq!(peek.view().unwrap_or_abort().unread_count, 1);
    peek.jump_to_bottom().unwrap_or_abort();
    let view = peek.view().unwrap_or_abort();
    assert_eq!(view.unread_count, 0);
    assert_eq!(view.follow, FollowMode::Following);
}

#[test]
fn dashboard_peek_keeps_selection_through_reorder_and_marks_finished() {
    let mut peek = DashboardPeek::new(10.0).unwrap_or_abort();
    let session_a = key("session-a");
    let session_b = key("session-b");
    peek.sync_sessions(&[session_a.clone(), session_b.clone()])
        .unwrap_or_abort();
    peek.select(&session_b).unwrap_or_abort();

    peek.sync_sessions(&[session_b.clone(), session_a.clone()])
        .unwrap_or_abort();
    peek.finish(&session_b).unwrap_or_abort();

    let view = peek.view().unwrap_or_abort();
    assert_eq!(view.session_id, session_b);
    assert_eq!(view.status, PeekSessionStatus::Finished);
    assert!(peek
        .view_for(&session_a)
        .unwrap_or_abort()
        .blocks
        .is_empty());
}

#[test]
fn dashboard_peek_bounds_each_session_tail_cache() {
    let limits = PeekLimits::new(2, 2);
    let mut peek = DashboardPeek::with_limits(10.0, limits).unwrap_or_abort();
    let session = key("session-a");
    let blocks = vec![block(1, "one"), block(2, "two"), block(3, "three")];
    peek.sync_sessions(std::slice::from_ref(&session))
        .unwrap_or_abort();
    peek.replace_blocks(&session, &blocks).unwrap_or_abort();

    let view = peek.view_for(&session).unwrap_or_abort();
    assert_eq!(view.blocks.len(), 2);
    assert_eq!(view.blocks[0].content, "two");
    assert_eq!(view.blocks[1].content, "three");
    assert_eq!(peek.cached_block_count(&session), Some(2));
}

#[test]
fn dashboard_peek_isolates_content_and_follow_state_between_sessions() {
    let mut peek = DashboardPeek::new(10.0).unwrap_or_abort();
    let session_a = key("session-a");
    let session_b = key("session-b");
    let blocks_a = vec![block(1, "only-a")];
    let blocks_b = vec![block(2, "only-b")];
    peek.sync_sessions(&[session_a.clone(), session_b.clone()])
        .unwrap_or_abort();
    peek.replace_blocks(&session_a, &blocks_a).unwrap_or_abort();
    peek.replace_blocks(&session_b, &blocks_b).unwrap_or_abort();
    peek.set_layout(&session_a, layout(&blocks_a, 10.0))
        .unwrap_or_abort();
    peek.set_layout(&session_b, layout(&blocks_b, 10.0))
        .unwrap_or_abort();
    peek.select(&session_a).unwrap_or_abort();
    peek.scroll_by(1.0).unwrap_or_abort();

    let view_a = peek.view_for(&session_a).unwrap_or_abort();
    let view_b = peek.view_for(&session_b).unwrap_or_abort();
    assert_eq!(view_a.blocks[0].content, "only-a");
    assert_eq!(view_b.blocks[0].content, "only-b");
    assert_eq!(view_a.follow, FollowMode::Detached);
    assert_eq!(view_b.follow, FollowMode::Following);
}
