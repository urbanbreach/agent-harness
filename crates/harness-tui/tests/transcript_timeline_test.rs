use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::theme::{ColorLevel, Theme};
use harness_tui::theme_tokens::{LifecycleState, ViewportId};
use harness_tui::transcript_identity::{ReplayTurn, TranscriptIdentity};
use harness_tui::transcript_timeline::{
    clip_marker_label, geometry_for_rect, geometry_for_viewport, marker_display_width,
    MarkerInteraction, TimelineHitMap, TimelineJump, TimelineMarker, TimelineNavigation,
    TimelineStatus, TimelineTurn,
};
use harness_tui::UnwrapOrAbort;
use ratatui::layout::Rect;
use ratatui::style::Color;

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

fn response_turns() -> Vec<TimelineTurn> {
    vec![
        event_turn(0, TimelineStatus::Queued, LifecycleState::Queued),
        event_turn(1, TimelineStatus::Completed, LifecycleState::Completed),
        event_turn(2, TimelineStatus::Streaming, LifecycleState::Streaming),
        event_turn(3, TimelineStatus::Completed, LifecycleState::Completed),
        event_turn(4, TimelineStatus::Failed, LifecycleState::Failed),
        checkpoint_turn(5),
        event_turn(6, TimelineStatus::Completed, LifecycleState::Completed),
    ]
}

#[test]
fn empty_transcript_has_a_rail_but_no_markers() {
    // arrange
    // act
    let geometry = geometry_for_viewport(ViewportId::Default80x24, &[], 0);

    // assert
    assert_eq!(geometry.marker_rects.len(), 0);
    assert_eq!(geometry.rail_rect, Rect::new(0, 0, 1, 24));
}

#[test]
fn one_turn_marker_uses_event_derived_turn_id_and_row() {
    // arrange
    // act
    let turn = event_turn(4, TimelineStatus::Completed, LifecycleState::Completed);
    let geometry = geometry_for_rect(Rect::new(3, 2, 80, 20), std::slice::from_ref(&turn), 0);

    // assert
    assert_eq!(geometry.marker_rects.len(), 1);
    assert_eq!(geometry.marker_rects[0].turn_id, turn.marker.turn_id);
    assert_eq!(geometry.marker_rects[0].rect.y, 2 + 12);
}

#[test]
fn many_turns_render_in_order_across_all_task_six_viewports() {
    // arrange
    let turns = many_turns();

    // act
    for viewport in ViewportId::ALL {
        let geometry = geometry_for_viewport(viewport, &turns, 0);
        let rows = geometry
            .marker_rects
            .iter()
            .map(|marker| marker.rect.y)
            .collect::<Vec<_>>();
        // assert
        assert!(rows.windows(2).all(|window| window[0] <= window[1]));
        assert!(geometry.rail_rect.width <= viewport.dimensions().0);
    }
}

#[test]
fn streaming_marker_exposes_distinct_active_and_hover_styles() {
    // arrange
    let marker = TimelineMarker::from_replay(
        ReplayTurn::event(11, 0, 1),
        TimelineStatus::Streaming,
        LifecycleState::Streaming,
    );

    // act
    let theme = harness_tui::theme::Theme::harness_dark();
    let active = marker.style(MarkerInteraction::Active, &theme);
    let hovered = marker.style(MarkerInteraction::Hovered, &theme);
    // assert
    assert_ne!(active, hovered);
    assert_eq!(active.glyph, marker.glyph());
}

#[test]
fn timeline_markers_follow_active_terminal_color_level() {
    // arrange
    let marker = TimelineMarker::from_replay(
        ReplayTurn::event(11, 0, 1),
        TimelineStatus::Streaming,
        LifecycleState::Streaming,
    );

    // act
    for level in [
        ColorLevel::TrueColor,
        ColorLevel::Ansi256,
        ColorLevel::Basic,
        ColorLevel::None,
    ] {
        let theme = Theme::harness_dark().for_color_level(level);
        let style = marker.style(MarkerInteraction::Normal, &theme);
        match level {
            ColorLevel::TrueColor => {
                // assert
                assert!(matches!(style.foreground, Color::Rgb(..)));
                assert!(matches!(style.background, Color::Rgb(..)));
            }
            ColorLevel::Ansi256 => {
                assert!(matches!(style.foreground, Color::Indexed(_)));
                assert!(matches!(style.background, Color::Indexed(_)));
            }
            ColorLevel::Basic => {
                assert!(!matches!(
                    style.foreground,
                    Color::Rgb(..) | Color::Indexed(_)
                ));
                assert!(!matches!(
                    style.background,
                    Color::Rgb(..) | Color::Indexed(_)
                ));
            }
            ColorLevel::None => {
                assert_eq!(style.foreground, Color::Reset);
                assert_eq!(style.background, Color::Reset);
            }
        }
    }
}

#[test]
fn failed_marker_is_the_target_of_jump_to_failed() {
    // arrange
    let turns = many_turns();
    let mut navigation = TimelineNavigation::from_turns(turns.clone(), 10).unwrap_or_abort();

    // act
    let snapshot = navigation
        .jump(TimelineJump::JumpToFailed)
        .unwrap_or_abort();
    // assert
    assert_eq!(snapshot.selected_turn_id, Some(turns[2].marker.turn_id));
}

#[test]
fn compacted_checkpoint_marker_uses_checkpoint_identity() {
    // arrange
    // act
    let turn = checkpoint_turn(2);
    let expected = ReplayTurn::checkpoint(900, 2, 1).turn_id();

    // assert
    assert_eq!(turn.marker.turn_id, expected);
    assert_eq!(turn.marker.status, TimelineStatus::Compacted);
    assert_eq!(turn.marker.lifecycle_state, LifecycleState::Compacting);
}

#[test]
fn narrow_geometry_clips_marker_labels_to_available_cells() {
    // arrange
    // act
    let turn = event_turn(0, TimelineStatus::Completed, LifecycleState::Completed)
        .with_label("completed marker label");
    let geometry = geometry_for_viewport(ViewportId::Compact40x10, &[turn], 0);
    let marker = &geometry.marker_rects[0];

    // assert
    assert!(marker.clipped);
    assert!(marker.rect.width <= 1);
    assert!(marker.rect.right() <= geometry.rail_rect.right().saturating_add(4));
}

#[test]
fn cjk_marker_width_is_measured_without_splitting_a_wide_cell() {
    // arrange
    // act
    // assert
    assert_eq!(marker_display_width("成功"), 4);
    assert_eq!(clip_marker_label("成功", 3), "成");
    assert_eq!(marker_display_width(&clip_marker_label("成功", 3)), 2);
}

#[test]
fn keyboard_jumps_land_on_expected_turn_ids_and_preserve_anchor() {
    // arrange
    // act
    let turns = many_turns();
    let mut navigation = TimelineNavigation::from_turns(turns.clone(), 3).unwrap_or_abort();
    let first = navigation.jump(TimelineJump::NextTurn).unwrap_or_abort();
    let streaming = navigation
        .jump(TimelineJump::JumpToStreaming)
        .unwrap_or_abort();
    let failed = navigation
        .jump(TimelineJump::JumpToFailed)
        .unwrap_or_abort();

    // assert
    assert_eq!(first.selected_turn_id, Some(turns[0].marker.turn_id));
    assert_eq!(streaming.selected_turn_id, Some(turns[1].marker.turn_id));
    assert_eq!(failed.selected_turn_id, Some(turns[2].marker.turn_id));
    assert!(failed.anchor.is_some());
}

#[test]
fn mouse_hit_rectangles_match_the_rendered_marker_cells() {
    // arrange
    let turns = many_turns();
    let geometry = geometry_for_viewport(ViewportId::Default80x24, &turns, 0);
    let hit_map = TimelineHitMap::from_geometry(&geometry);
    let mut navigation = TimelineNavigation::from_turns(turns.clone(), 24).unwrap_or_abort();

    // act
    for marker in &geometry.marker_rects {
        // assert
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
    // arrange
    let turns = many_turns();
    let mut navigation = TimelineNavigation::from_turns(turns.clone(), 10).unwrap_or_abort();
    let key = KeyEvent::new(KeyCode::Down, KeyModifiers::NONE);

    // act
    navigation.handle_key(key).unwrap_or_abort();

    // assert
    assert_eq!(navigation.selected_turn_id(), Some(turns[0].marker.turn_id));
}

#[test]
fn reflowed_event_turns_preserve_selected_id_and_scroll_anchor() {
    // arrange
    // act
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

    // assert
    assert_eq!(updated.selected_turn_id, initial.selected_turn_id);
    assert_eq!(updated.scroll_top, initial.scroll_top.saturating_add(2));
}

#[test]
fn final_response_navigation_orders_stable_ids_and_skips_non_final_entries() {
    // arrange
    let turns = response_turns();
    let expected = [turns[1].turn_id(), turns[3].turn_id(), turns[6].turn_id()];
    let mut navigation = TimelineNavigation::from_turns(turns, 4).unwrap_or_abort();

    // act
    let selected = expected
        .iter()
        .map(|_| {
            navigation
                .jump(TimelineJump::NextResponse)
                .unwrap_or_abort()
                .selected_turn_id
        })
        .collect::<Vec<_>>();

    // assert
    assert_eq!(selected, expected.into_iter().map(Some).collect::<Vec<_>>());
}

#[test]
fn final_response_navigation_clamps_at_first_and_last_targets() {
    // arrange
    let turns = response_turns();
    let first = turns[1].turn_id();
    let last = turns[6].turn_id();
    let mut navigation = TimelineNavigation::from_turns(turns, 4).unwrap_or_abort();

    // act
    let before_first = navigation
        .jump(TimelineJump::PreviousResponse)
        .unwrap_or_abort();
    let mut after_last = before_first;
    for _ in 0..4 {
        after_last = navigation
            .jump(TimelineJump::NextResponse)
            .unwrap_or_abort();
    }
    let clamped_last = navigation
        .jump(TimelineJump::NextResponse)
        .unwrap_or_abort();

    // assert
    assert_eq!(before_first.selected_turn_id, Some(first));
    assert_eq!(after_last.selected_turn_id, Some(last));
    assert_eq!(clamped_last.selected_turn_id, Some(last));
}

#[test]
fn final_response_position_stays_stable_after_reflow_resize_and_compaction() {
    // arrange
    let turns = response_turns();
    let selected_id = turns[3].turn_id();
    let mut navigation = TimelineNavigation::from_turns(turns, 4).unwrap_or_abort();
    navigation
        .jump(TimelineJump::NextResponse)
        .unwrap_or_abort();
    let initial = navigation
        .jump(TimelineJump::NextResponse)
        .unwrap_or_abort();

    // act
    navigation.resize(8);
    let updated_turns = vec![
        checkpoint_turn(50),
        event_turn(1, TimelineStatus::Completed, LifecycleState::Completed),
        event_turn(2, TimelineStatus::Streaming, LifecycleState::Streaming),
        event_turn(3, TimelineStatus::Completed, LifecycleState::Completed),
        event_turn(4, TimelineStatus::Failed, LifecycleState::Failed),
        checkpoint_turn(5),
        event_turn(6, TimelineStatus::Completed, LifecycleState::Completed),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, turn)| {
        TimelineTurn::from_replay(
            turn.replay_turn(),
            index.saturating_mul(5),
            turn.height,
            turn.marker.status,
            turn.marker.lifecycle_state,
        )
    })
    .collect::<Vec<_>>();
    let identity =
        TranscriptIdentity::from_replay(updated_turns.iter().map(TimelineTurn::replay_turn))
            .unwrap_or_abort();
    let updated = navigation
        .update_document(identity, updated_turns)
        .unwrap_or_abort();

    // assert
    assert_eq!(initial.selected_turn_id, Some(selected_id));
    assert_eq!(updated.selected_turn_id, Some(selected_id));
    assert_eq!(
        initial.response_position.map(|p| (p.index, p.total)),
        Some((2, 3))
    );
    assert_eq!(
        updated.response_position.map(|p| (p.index, p.total)),
        Some((2, 3))
    );
}
