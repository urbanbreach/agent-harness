use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::UnwrapOrAbort;
use harness_tui::design_contract::{LifecycleState, ViewportId};
use harness_tui::transcript_identity::{ReplayTurn, TranscriptIdentity};
use harness_tui::transcript_timeline::{
    MarkerInteraction, TimelineHitMap, TimelineJump, TimelineMarker, TimelineNavigation,
    TimelineStatus, TimelineTurn, clip_marker_label, geometry_for_rect, geometry_for_viewport,
    marker_display_width,
};
use ratatui::layout::Rect;

fn event_turn(index: u64, status: TimelineStatus, lifecycle: LifecycleState) -> TimelineTurn {
    TimelineTurn::from_replay(
        ReplayTurn::event(100 + index, index, 1),
        usize::try_from(index).unwrap_or_abort() * 3,
        2,
        status,
        lifecycle,
    )
}

fn checkpoint_turn(index: u64) -> TimelineTurn {
    TimelineTurn::from_replay(
        ReplayTurn::checkpoint(900, index, 1),
        usize::try_from(index).unwrap_or_abort() * 3,
        2,
        TimelineStatus::Compacted,
        LifecycleState::Compacting,
    )
}

fn many_turns() -> Vec<TimelineTurn> {
    vec![
        event_turn(0, TimelineStatus::Completed, LifecycleState::Completed),
        event_turn(1, TimelineStatus::Streaming, LifecycleState::Streaming),
        event_turn(2, TimelineStatus::Failed, LifecycleState::Failed),
        event_turn(3, TimelineStatus::Completed, LifecycleState::Completed),
    ]
}

#[test]
fn empty_transcript_has_a_rail_but_no_markers() {
    let geometry = geometry_for_viewport(ViewportId::Default80x24, &[], 0);

    assert_eq!(geometry.marker_rects.len(), 0);
    assert_eq!(geometry.rail_rect, Rect::new(0, 0, 1, 24));
}

#[test]
fn one_turn_marker_uses_event_derived_turn_id_and_row() {
    let turn = event_turn(4, TimelineStatus::Completed, LifecycleState::Completed);
    let geometry = geometry_for_rect(Rect::new(3, 2, 80, 20), std::slice::from_ref(&turn), 0);

    assert_eq!(geometry.marker_rects.len(), 1);
    assert_eq!(geometry.marker_rects[0].turn_id, turn.marker.turn_id);
    assert_eq!(geometry.marker_rects[0].rect.y, 2 + 12);
}

#[test]
fn many_turns_render_in_order_across_all_task_six_viewports() {
    let turns = many_turns();

    for viewport in ViewportId::ALL {
        let geometry = geometry_for_viewport(viewport, &turns, 0);
        let rows = geometry
            .marker_rects
            .iter()
            .map(|marker| marker.rect.y)
            .collect::<Vec<_>>();
        assert!(rows.windows(2).all(|window| window[0] <= window[1]));
        assert!(geometry.rail_rect.width <= viewport.dimensions().0);
    }
}

#[test]
fn streaming_marker_exposes_distinct_active_and_hover_styles() {
    let marker = TimelineMarker::from_replay(
        ReplayTurn::event(11, 0, 1),
        TimelineStatus::Streaming,
        LifecycleState::Streaming,
    );

    let active = marker.style(MarkerInteraction::Active);
    let hovered = marker.style(MarkerInteraction::Hovered);
    assert_ne!(active, hovered);
    assert_eq!(active.glyph, marker.glyph());
}

#[test]
fn failed_marker_is_the_target_of_jump_to_failed() {
    let turns = many_turns();
    let mut navigation = TimelineNavigation::from_turns(turns.clone(), 10).unwrap_or_abort();

    let snapshot = navigation
        .jump(TimelineJump::JumpToFailed)
        .unwrap_or_abort();
    assert_eq!(snapshot.selected_turn_id, Some(turns[2].marker.turn_id));
}

#[test]
fn compacted_checkpoint_marker_uses_checkpoint_identity() {
    let turn = checkpoint_turn(2);
    let expected = ReplayTurn::checkpoint(900, 2, 1).turn_id();

    assert_eq!(turn.marker.turn_id, expected);
    assert_eq!(turn.marker.status, TimelineStatus::Compacted);
    assert_eq!(turn.marker.lifecycle_state, LifecycleState::Compacting);
}

#[test]
fn narrow_geometry_clips_marker_labels_to_available_cells() {
    let turn = event_turn(0, TimelineStatus::Completed, LifecycleState::Completed)
        .with_label("completed marker label");
    let geometry = geometry_for_viewport(ViewportId::Compact40x10, &[turn], 0);
    let marker = &geometry.marker_rects[0];

    assert!(marker.clipped);
    assert!(marker.rect.width <= 1);
    assert!(marker.rect.right() <= geometry.rail_rect.right().saturating_add(4));
}

#[test]
fn cjk_marker_width_is_measured_without_splitting_a_wide_cell() {
    assert_eq!(marker_display_width("成功"), 4);
    assert_eq!(clip_marker_label("成功", 3), "成");
    assert_eq!(marker_display_width(&clip_marker_label("成功", 3)), 2);
}

#[test]
fn keyboard_jumps_land_on_expected_turn_ids_and_preserve_anchor() {
    let turns = many_turns();
    let mut navigation = TimelineNavigation::from_turns(turns.clone(), 3).unwrap_or_abort();
    let first = navigation.jump(TimelineJump::NextTurn).unwrap_or_abort();
    let streaming = navigation
        .jump(TimelineJump::JumpToStreaming)
        .unwrap_or_abort();
    let failed = navigation
        .jump(TimelineJump::JumpToFailed)
        .unwrap_or_abort();

    assert_eq!(first.selected_turn_id, Some(turns[0].marker.turn_id));
    assert_eq!(streaming.selected_turn_id, Some(turns[1].marker.turn_id));
    assert_eq!(failed.selected_turn_id, Some(turns[2].marker.turn_id));
    assert!(failed.anchor.is_some());
}

#[test]
fn mouse_hit_rectangles_match_the_rendered_marker_cells() {
    let turns = many_turns();
    let geometry = geometry_for_viewport(ViewportId::Default80x24, &turns, 0);
    let hit_map = TimelineHitMap::from_geometry(&geometry);
    let mut navigation = TimelineNavigation::from_turns(turns.clone(), 24).unwrap_or_abort();

    for marker in &geometry.marker_rects {
        assert_eq!(
            hit_map.hit_test(marker.rect.x, marker.rect.y),
            Some(marker.turn_id)
        );
        assert_eq!(hit_map.hit_test(marker.rect.right(), marker.rect.y), None);
    }
    assert_eq!(
        hit_map.hit_test(geometry.rail_rect.x, geometry.rail_rect.y),
        None
    );
    let selected = navigation
        .select_at(
            &hit_map,
            geometry.marker_rects[1].rect.x,
            geometry.marker_rects[1].rect.y,
        )
        .unwrap_or_abort();
    assert_eq!(
        selected.map(|snapshot| snapshot.selected_turn_id),
        Some(Some(turns[1].marker.turn_id))
    );
}

#[test]
fn key_adapter_matches_next_turn_navigation() {
    let turns = many_turns();
    let mut navigation = TimelineNavigation::from_turns(turns.clone(), 10).unwrap_or_abort();
    let key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);

    navigation.handle_key(key).unwrap_or_abort();

    assert_eq!(navigation.selected_turn_id(), Some(turns[0].marker.turn_id));
}

#[test]
fn reflowed_event_turns_preserve_selected_id_and_scroll_anchor() {
    let turns = many_turns();
    let mut navigation = TimelineNavigation::from_turns(turns.clone(), 3).unwrap_or_abort();
    let initial = navigation
        .jump(TimelineJump::JumpToFailed)
        .unwrap_or_abort();
    let shifted = turns
        .iter()
        .map(|turn| {
            TimelineTurn::from_replay(
                turn.replay_turn(),
                turn.row.saturating_add(2),
                turn.height,
                turn.marker.status,
                turn.marker.lifecycle_state,
            )
        })
        .collect::<Vec<_>>();
    let identity = TranscriptIdentity::from_replay(shifted.iter().map(TimelineTurn::replay_turn))
        .unwrap_or_abort();
    let updated = navigation
        .update_document(identity, shifted)
        .unwrap_or_abort();

    assert_eq!(updated.selected_turn_id, initial.selected_turn_id);
    assert_eq!(updated.scroll_top, initial.scroll_top.saturating_add(2));
}
