use harness_tui::design_contract::{LifecycleState, ViewportId};
use harness_tui::scheduling::DualClock;
use harness_tui::transcript_blocks::{BlockKind, BlockLifecycle, FoldState};
use harness_tui::transcript_identity::{BlockId, ReplayTurn};
use harness_tui::transcript_integration::{
    BlockSeed, BoundedLayoutCache, CacheInvalidation, CachedLayout, LayoutCacheKey,
    LifecycleCoordinator, LifecyclePhase, TimelineStatus, TranscriptComposite, TranscriptEvent,
    TranscriptViewModel, TurnSeed,
};
use ratatui::layout::Rect;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn key(id: BlockId, width: u16) -> LayoutCacheKey {
    LayoutCacheKey::new(id, width, FoldState::Expanded, BlockLifecycle::Streaming)
}

#[test]
fn cache_is_width_aware_bounded_and_lru_evicted() {
    // Given: a two-entry layout cache with three replay-derived keys.
    let ids = [
        ReplayTurn::event(1, 0, 1).block_id(0),
        ReplayTurn::event(2, 1, 1).block_id(0),
        ReplayTurn::event(3, 2, 1).block_id(0),
    ];
    let mut cache = BoundedLayoutCache::with_capacity(2);

    // When: the oldest entry is read, then a third entry is inserted and width changes.
    cache.insert(key(ids[0], 80), CachedLayout::new(2, 20, 40));
    cache.insert(key(ids[1], 80), CachedLayout::new(3, 30, 60));
    let _ = cache.get(&key(ids[0], 80));
    cache.insert(key(ids[2], 80), CachedLayout::new(4, 40, 80));

    // Then: LRU eviction and width invalidation are observable and bounded.
    assert_eq!(cache.len(), 2);
    assert!(cache.contains(&key(ids[0], 80)));
    assert!(!cache.contains(&key(ids[1], 80)));
    cache.apply(CacheInvalidation::WidthChanged { from: 80, to: 100 });
    assert_eq!(cache.len(), 0);
    assert!(cache.capacity() <= 256);
}

#[test]
fn lifecycle_frames_advance_only_through_the_fake_clock_scheduler() -> TestResult {
    // Given: one streaming thinking block and the task-10 dual fake clock.
    let id = ReplayTurn::event(9, 0, 1).block_id(0);
    let mut lifecycle = LifecycleCoordinator::new(false);
    lifecycle.set_block(id, BlockKind::Thinking, BlockLifecycle::Streaming);
    let clock = DualClock::new();

    // When: the clock is sampled before and after one animation deadline.
    let initial = lifecycle.tick(clock.snapshot());
    clock.tick_animation();
    let advanced = lifecycle.tick(clock.snapshot());

    // Then: the first frame is stable and the scheduled frame advances deterministically.
    assert_eq!(initial.states[0].phase, LifecyclePhase::Streaming);
    assert_eq!(initial.states[0].frame, 0);
    assert!(advanced.states[0].frame > initial.states[0].frame);
    lifecycle.set_block(id, BlockKind::Thinking, BlockLifecycle::Completed);
    assert!(!lifecycle.animation_active());
    Ok(())
}

fn scene() -> TestResult<TranscriptComposite> {
    let mut composite = TranscriptComposite::new(Rect::new(0, 0, 80, 24))?;
    let first = ReplayTurn::event(10, 0, 1);
    let second = ReplayTurn::event(20, 1, 1);
    composite.apply(TranscriptEvent::TurnStarted(TurnSeed::new(
        first,
        TimelineStatus::Streaming,
        LifecycleState::Thinking,
    )))?;
    composite.apply(TranscriptEvent::BlockCreated(BlockSeed {
        id: first.block_id(0),
        turn_id: first.turn_id(),
        kind: BlockKind::Thinking,
        lifecycle: BlockLifecycle::Streaming,
        content: "thinking".to_owned(),
        raw: None,
    }))?;
    composite.apply(TranscriptEvent::TurnStarted(TurnSeed::new(
        second,
        TimelineStatus::Completed,
        LifecycleState::Completed,
    )))?;
    composite.apply(TranscriptEvent::BlockCreated(BlockSeed {
        id: second.block_id(0),
        turn_id: second.turn_id(),
        kind: BlockKind::Tool,
        lifecycle: BlockLifecycle::Completed,
        content: "tool output".to_owned(),
        raw: None,
    }))?;
    Ok(composite)
}

fn interleaved_frame() -> TestResult<TranscriptViewModel> {
    let mut composite = scene()?;
    let first = ReplayTurn::event(10, 0, 1);
    let second = ReplayTurn::event(20, 1, 1);
    composite.set_block_lifecycle(first.block_id(0), BlockLifecycle::Completed)?;
    composite.toggle_fold(first.block_id(0))?;
    composite.scroll_by(2.0)?;
    composite.resize(Rect::new(0, 0, 100, 30))?;
    composite.open_viewer(first.block_id(0))?;
    composite.compact_turn(
        second.turn_id(),
        TurnSeed::new(
            ReplayTurn::checkpoint(90, 1, 0),
            TimelineStatus::Compacted,
            LifecycleState::Compacting,
        ),
    )?;
    composite.close_viewer()?;
    Ok(composite.view().clone())
}

#[test]
fn interleaved_lifecycle_fold_scroll_resize_compaction_and_viewer_exit_settle_deterministically(
) -> TestResult {
    // Given: two independently constructed event-derived transcript composites.
    let first = interleaved_frame()?;
    let second = interleaved_frame()?;

    // When: both follow the same lifecycle-rich operation sequence.
    // Then: the settled view model and fold/lifecycle state are identical.
    assert_eq!(first, second);
    assert_eq!(first.blocks.len(), 1);
    assert_eq!(first.blocks[0].fold_state, FoldState::Expanded);
    assert_eq!(first.blocks[0].lifecycle, BlockLifecycle::Completed);
    assert_eq!(
        first.viewport.width,
        ViewportId::Standard100x30.dimensions().0
    );
    Ok(())
}
