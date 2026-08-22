use harness_tui::theme_tokens::LifecycleState;
use harness_tui::transcript_blocks::{BlockKind, BlockLifecycle};
use harness_tui::transcript_identity::ReplayTurn;
use harness_tui::transcript_integration::{
    BlockSeed, TimelineStatus, TranscriptComposite, TranscriptEvent, TurnSeed,
};
use ratatui::layout::Rect;

const SETTLED_ACTIVITY_COUNT: usize = 10_000;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn turn(index: usize, kind: BlockKind, lifecycle: BlockLifecycle) -> Vec<TranscriptEvent> {
    let turn_index = u64::try_from(index).unwrap_or(u64::MAX);
    let replay = ReplayTurn::event(turn_index.saturating_add(1), turn_index, 1);
    vec![
        TranscriptEvent::TurnStarted(TurnSeed::new(
            replay,
            TimelineStatus::Completed,
            LifecycleState::Completed,
        )),
        TranscriptEvent::BlockCreated(BlockSeed {
            id: replay.block_id(0),
            turn_id: replay.turn_id(),
            kind,
            lifecycle,
            content: format!("settled activity {index}"),
            raw: None,
        }),
    ]
}

fn settled_events(last_kind: BlockKind) -> Vec<TranscriptEvent> {
    (0..SETTLED_ACTIVITY_COUNT)
        .flat_map(|index| {
            let kind = if index + 1 == SETTLED_ACTIVITY_COUNT {
                last_kind
            } else {
                BlockKind::Assistant
            };
            turn(index, kind, BlockLifecycle::Completed)
        })
        .collect()
}

fn composite_with(events: &[TranscriptEvent]) -> TestResult<TranscriptComposite> {
    let mut composite = TranscriptComposite::new(Rect::new(0, 0, 120, 40))?;
    composite.replace_events(events.to_vec())?;
    Ok(composite)
}

#[test]
fn token_update_touches_one_activity_and_block_after_ten_thousand_settle() -> TestResult {
    // Given: ten thousand settled activities projected through the replay boundary.
    let events = settled_events(BlockKind::Assistant);
    let mut incremental = composite_with(&events)?;
    let last = SETTLED_ACTIVITY_COUNT - 1;
    let mut updated_turn = turn(last, BlockKind::Assistant, BlockLifecycle::Completed);
    let TranscriptEvent::BlockCreated(block) = &mut updated_turn[1] else {
        return Err("turn fixture must contain a block".into());
    };
    block.content.push_str(" + token");
    incremental.reset_work_metrics();

    // When: one live token updates the final activity.
    incremental.sync_turn(updated_turn)?;

    // Then: no historical event is replayed or block remeasured.
    let metrics = incremental.work_metrics();
    assert_eq!(metrics.activities_touched, 1);
    assert_eq!(metrics.blocks_touched, 1);
    assert_eq!(metrics.blocks_measured, 1);
    assert_eq!(metrics.placements_reflowed, 1);
    assert_eq!(metrics.turns_reflowed, 1);
    assert_eq!(metrics.events_reprojected, 0);
    assert_eq!(metrics.full_rebuilds, 0);
    assert!(incremental.cache().len() <= incremental.cache().capacity());

    let replayed = composite_with(incremental.events())?;
    assert_eq!(incremental.view(), replayed.view());
    Ok(())
}

#[test]
fn tool_update_touches_one_activity_and_block_after_ten_thousand_settle() -> TestResult {
    // Given: ten thousand settled activities ending in one tool block.
    let events = settled_events(BlockKind::Tool);
    let mut incremental = composite_with(&events)?;
    let last = SETTLED_ACTIVITY_COUNT - 1;
    incremental.reset_work_metrics();

    // When: the tool returns to a streaming lifecycle without changing history.
    let updated_turn = turn(last, BlockKind::Tool, BlockLifecycle::Streaming);
    incremental.sync_turn(updated_turn)?;

    // Then: only that activity and tool block are damaged and measured.
    let metrics = incremental.work_metrics();
    assert_eq!(metrics.activities_touched, 1);
    assert_eq!(metrics.blocks_touched, 1);
    assert_eq!(metrics.blocks_measured, 1);
    assert_eq!(metrics.placements_reflowed, 1);
    assert_eq!(metrics.turns_reflowed, 1);
    assert_eq!(metrics.events_reprojected, 0);
    assert_eq!(metrics.full_rebuilds, 0);
    assert!(incremental.cache().len() <= incremental.cache().capacity());

    let replayed = composite_with(incremental.events())?;
    assert_eq!(incremental.view(), replayed.view());
    Ok(())
}

#[test]
fn resize_remeasures_projected_blocks_without_replaying_events() -> TestResult {
    // Given: a settled transcript whose replay projection is already materialized.
    let events = turn(0, BlockKind::Assistant, BlockLifecycle::Completed);
    let mut composite = composite_with(&events)?;
    composite.reset_work_metrics();

    // When: the viewport width changes.
    composite.resize(Rect::new(0, 0, 100, 30))?;

    // Then: layout is rebuilt from projected state without replaying its event history.
    let metrics = composite.work_metrics();
    assert_eq!(metrics.events_reprojected, 0);
    assert_eq!(metrics.full_rebuilds, 0);
    assert_eq!(metrics.blocks_measured, 1);
    Ok(())
}
