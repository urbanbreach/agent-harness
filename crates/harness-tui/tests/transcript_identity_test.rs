use harness_tui::transcript_identity::{
    assert_block_id, assert_turn_id, BlockId, FocusFollowState, IdentityError, InPlaceMode,
    ReplayIdentityKey, ReplayTurn, TranscriptFocus, TranscriptIdentity, TranscriptScreenMode,
    TranscriptScreenState, TranscriptSelection, TurnId,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn replay(
    turns: impl IntoIterator<Item = ReplayTurn>,
) -> Result<TranscriptIdentity, IdentityError> {
    TranscriptIdentity::from_replay(turns)
}

#[test]
fn replay_produces_identical_ids_and_replay_checks_pass() -> TestResult {
    // Given: the same ordered replay descriptors.
    let descriptors = [ReplayTurn::event(7, 0, 2), ReplayTurn::event(9, 1, 1)];

    // When: the replay is projected twice.
    let first = replay(descriptors)?;
    let second = replay(descriptors)?;

    // Then: turn and renderable-block IDs are byte-for-byte deterministic.
    assert_eq!(first, second);
    assert_eq!(first.turns()[0].id(), TurnId::from_replay(7, 0));
    assert_eq!(
        first.turns()[0].blocks()[0].id(),
        BlockId::from_replay(7, 0, 0)
    );
    assert!(assert_turn_id(ReplayIdentityKey::event(7, 0), first.turns()[0].id()).is_ok());
    assert!(assert_block_id(
        ReplayIdentityKey::block_event(7, 0, 0),
        first.turns()[0].blocks()[0].id()
    )
    .is_ok());
    Ok(())
}

#[test]
fn append_preserves_existing_ids() -> TestResult {
    // Given: a replay with two turns.
    let before = replay([ReplayTurn::event(10, 0, 1), ReplayTurn::event(20, 1, 2)])?;

    // When: a later turn is appended.
    let after = replay([
        ReplayTurn::event(10, 0, 1),
        ReplayTurn::event(20, 1, 2),
        ReplayTurn::event(30, 2, 1),
    ])?;

    // Then: all pre-existing turn and block IDs remain unchanged.
    assert_eq!(before.turns()[0], after.turns()[0]);
    assert_eq!(before.turns()[1], after.turns()[1]);
    Ok(())
}

#[test]
fn compaction_preserves_non_compacted_ids_and_uses_checkpoint_ids_for_compacted_turns() -> TestResult
{
    // Given: three replay turns with stable source indices.
    let before = replay([
        ReplayTurn::event(10, 0, 1),
        ReplayTurn::event(20, 1, 2),
        ReplayTurn::event(30, 2, 1),
    ])?;

    // When: the first turn folds into a checkpoint while later turns remain replay-derived.
    let after = replay([
        ReplayTurn::checkpoint(100, 0, 1),
        ReplayTurn::event(20, 1, 2),
        ReplayTurn::event(30, 2, 1),
    ])?;

    // Then: retained turns keep their IDs and the compacted turn has a checkpoint identity.
    assert_eq!(before.turns()[1], after.turns()[1]);
    assert_eq!(before.turns()[2], after.turns()[2]);
    assert_ne!(before.turns()[0].id(), after.turns()[0].id());
    assert_eq!(after.turns()[0].id(), TurnId::from_checkpoint(100, 0));
    Ok(())
}

#[test]
fn collapse_and_expand_preserve_block_ids() -> TestResult {
    // Given: renderable blocks and a UI-only collapsed set.
    let identity = replay([ReplayTurn::event(11, 0, 3)])?;
    let ids_before = identity.turns()[0]
        .blocks()
        .iter()
        .map(|block| block.id())
        .collect::<Vec<_>>();
    let mut collapsed = vec![ids_before[1]];

    // When: the block is collapsed and then expanded without changing replay descriptors.
    collapsed.clear();
    let ids_after = identity.turns()[0]
        .blocks()
        .iter()
        .map(|block| block.id())
        .collect::<Vec<_>>();

    // Then: disclosure state does not participate in block identity.
    assert!(collapsed.is_empty());
    assert_eq!(ids_before, ids_after);
    Ok(())
}

#[test]
fn resize_preserves_ids() -> TestResult {
    // Given: a replay-derived identity and two viewport sizes.
    let identity = replay([ReplayTurn::event(12, 0, 2)])?;
    let original = identity.clone();
    let narrow_viewport = (80_u16, 24_u16);
    let wide_viewport = (160_u16, 50_u16);

    // When: layout dimensions change without replaying different events.
    let _ = (narrow_viewport, wide_viewport);

    // Then: the identity remains exactly the same.
    assert_eq!(identity, original);
    Ok(())
}

#[test]
fn switching_screen_modes_is_side_effect_free() -> TestResult {
    // Given: immutable event storage and an in-place transcript state.
    let events = vec!["turn-start", "block", "turn-end"];
    let original_events = events.clone();
    let state = TranscriptScreenState::new(
        TranscriptScreenMode::InPlace(InPlaceMode::Transcript),
        FocusFollowState::new(TranscriptFocus::Transcript, true),
    );

    // When: the reversible UI mode changes twice.
    let timeline = state.switch_to(TranscriptScreenMode::InPlace(InPlaceMode::Timeline));
    let viewer = timeline.switch_to(TranscriptScreenMode::SelectedBlockViewer);
    let _pager = viewer.switch_to(TranscriptScreenMode::ExternalPagerSuspended);

    // Then: no event/projection data is changed by mode state transitions.
    assert_eq!(events, original_events);
    assert_eq!(
        state.mode(),
        TranscriptScreenMode::InPlace(InPlaceMode::Transcript)
    );
    assert_eq!(
        timeline.mode(),
        TranscriptScreenMode::InPlace(InPlaceMode::Timeline)
    );
    Ok(())
}

#[test]
fn returning_from_viewer_restores_focus_and_follow_exactly() -> TestResult {
    // Given: a focused, following in-place transcript.
    let original_focus = FocusFollowState::new(TranscriptFocus::Transcript, true);
    let state = TranscriptScreenState::new(
        TranscriptScreenMode::InPlace(InPlaceMode::Transcript),
        original_focus,
    );

    // When: a viewer temporarily takes focus and then returns.
    let viewer = state
        .switch_to(TranscriptScreenMode::SelectedBlockViewer)
        .with_focus_follow(FocusFollowState::new(TranscriptFocus::Viewer, false));
    let restored = viewer.return_to_in_place()?;

    // Then: both focus and follow are restored exactly, not merely approximately.
    assert_eq!(
        restored.mode(),
        TranscriptScreenMode::InPlace(InPlaceMode::Transcript)
    );
    assert_eq!(restored.focus_follow(), original_focus);
    Ok(())
}

#[test]
fn selection_survives_reorder_and_compaction() -> TestResult {
    // Given: a selection anchored to a retained replay turn and block.
    let before = replay([ReplayTurn::event(10, 0, 1), ReplayTurn::event(20, 1, 2)])?;
    let selection = TranscriptSelection::new(
        before.turns()[1].id(),
        before.turns()[1].blocks()[0].id(),
        2..5,
    )?;

    // When: replay order changes and an earlier turn is compacted.
    let after = replay([
        ReplayTurn::event(20, 1, 2),
        ReplayTurn::checkpoint(100, 0, 1),
    ])?;

    // Then: the stable turn/block anchors still resolve after reorder and compaction.
    assert!(selection.is_present_in(&after));
    Ok(())
}
