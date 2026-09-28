use super::*;
use crate::app::Focus;
use crate::UnwrapOrAbort;
use unicode_segmentation::UnicodeSegmentation;

#[test]
fn replay_session_layout_never_reserves_live_anchor() {
    let app = AppState::new_replay(std::path::PathBuf::from("/tmp/replay-session"), Vec::new());
    let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, 100, 30));

    assert!(plan.live_anchor.is_none());
}

#[test]
fn startup_composer_input_height_uses_harness_terminal_scaled_cap() {
    let text = "line\n".repeat(20);

    assert_eq!(startup_composer_input_height(&text, 75, 18), 6);
    assert_eq!(startup_composer_input_height(&text, 75, 48), 16);
}

#[test]
fn startup_dock_is_bottom_aligned_with_horizontal_inset() {
    let app = AppState::new_startup(Vec::new(), None);
    let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, 120, 32));
    let dock = plan.dock.unwrap_or_abort();

    assert_eq!(
        dock.shell.x,
        plan.shell.x.saturating_add(STARTUP_COMPOSER_INSET_X)
    );
    assert_eq!(
        dock.shell.width,
        plan.shell
            .width
            .saturating_sub(STARTUP_COMPOSER_INSET_X.saturating_mul(2))
    );
    assert_eq!(
        dock.shell.y,
        plan.shell
            .y
            .saturating_add(plan.shell.height.saturating_sub(dock.shell.height)),
        "startup composer docks to the bottom of the content shell"
    );
    assert_eq!(
        dock.shell.height, 4,
        "single-line startup dock is 3-row composer plus spacer"
    );
    assert_eq!(
        dock.composer.height, 3,
        "single-line startup composer content is 3 rows"
    );
}

#[test]
fn live_post_turn_dock_keeps_horizontal_inset_matching_freeze() {
    // Given: live shell (not startup) at freeze-primary 120×40
    let mut app = AppState::new_live(None, false, None);
    assert!(
        !app.startup_shell_visible(),
        "new_live must use post-startup dock path"
    );

    // When: layout is planned
    let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, 120, 40));
    let dock = plan.dock.unwrap_or_abort();

    // Then: composer dock matches freeze lead=2 inset (h1-stream-probe / run1-draft)
    assert_eq!(
        dock.shell.x,
        plan.shell.x.saturating_add(STARTUP_COMPOSER_INSET_X),
        "live dock shell.x must keep freeze horizontal inset"
    );
    assert_eq!(
        dock.shell.width,
        plan.shell
            .width
            .saturating_sub(STARTUP_COMPOSER_INSET_X.saturating_mul(2)),
        "live dock shell.width must keep freeze horizontal inset"
    );
    assert_eq!(
        dock.composer.x, dock.shell.x,
        "live composer band must share shell inset"
    );
    assert_eq!(
        dock.composer.width, dock.shell.width,
        "live composer band must share shell width"
    );
    app.focus = crate::app::Focus::Details;
    let unfocused = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, 120, 40));
    assert_eq!(
        unfocused.composer, plan.composer,
        "focus changes must not resize the empty composer"
    );
    assert_eq!(
        unfocused.transcript, plan.transcript,
        "focus changes must not move transcript rows"
    );
}

#[test]
fn quiet_overlays_remain_centered_after_dock_merge() {
    let theme = Theme::default();

    let mut palette = AppState::new_live(None, false, None);
    palette.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    let palette_plan = FrameLayoutPlan::for_app(&palette, Rect::new(0, 0, 100, 30));
    let palette_overlay = palette_plan.palette_overlay.unwrap_or_abort();
    let expected = command_palette_overlay_area(
        palette_plan.root,
        &theme,
        theme.live_shell_layout(100, 30),
        palette_plan.session_contract,
        &palette,
    )
    .unwrap_or_abort();
    assert_eq!(palette_overlay, expected);
}

#[test]
fn startup_palette_overlay_prefers_compact_modal_dimensions() {
    let mut palette = AppState::new_startup(Vec::new(), None);
    palette.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('p'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    let plan = FrameLayoutPlan::for_app(&palette, Rect::new(0, 0, 100, 30));
    let overlay = plan.palette_overlay.unwrap_or_abort();

    assert_eq!(overlay.width, 60);
    assert_eq!(overlay.x, 20);
    assert_eq!(overlay.y, (30 - overlay.height) / 2);
    assert_eq!(overlay.height, 25);
    assert!(overlay.bottom() <= plan.root.bottom());
}

#[test]
fn live_session_operator_sidebar_is_none_at_all_widths_including_wide() {
    // arrange
    let app = AppState::new_live(None, false, None);
    for (width, height) in [
        (120u16, 40u16),
        (100, 30),
        (80, 24),
        (79, 24),
        (80, 23),
        (60, 20),
        (121, 40),
        (140, 36),
        (160, 40),
    ] {
        // act
        let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, width, height));
        // assert
        assert!(
            plan.operator_sidebar.is_none(),
            "operator_sidebar must be None at {width}x{height}; got {:?}",
            plan.operator_sidebar
        );
        assert!(
            plan.details_overlay.is_none(),
            "details_overlay must be None at {width}x{height}; got {:?}",
            plan.details_overlay
        );
        assert!(
            plan.wheel_hit_areas.inspector.is_none(),
            "inspector hit area must be None at {width}x{height}; got {:?}",
            plan.wheel_hit_areas.inspector
        );
        assert!(
            plan.wheel_hit_areas.overlay.is_none(),
            "overlay hit area must be None at {width}x{height}; got {:?}",
            plan.wheel_hit_areas.overlay
        );
    }
}

#[test]
fn live_details_drawer_uses_secondary_overlay_not_primary_sidebar() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    app.live_details_drawer_open = true;
    for (width, height) in [(160u16, 40u16), (120, 40), (100, 30)] {
        // act
        let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, width, height));
        // assert
        assert!(
            plan.operator_sidebar.is_none(),
            "details drawer must not allocate primary operator_sidebar at {width}x{height}"
        );
        assert!(
            plan.details_overlay.is_some(),
            "details drawer must expose secondary overlay at {width}x{height}"
        );
    }
}

#[test]
fn live_session_transcript_and_composer_span_full_shell_width() {
    // arrange
    let app = AppState::new_live(None, false, None);
    for (width, height) in [
        (120u16, 40u16),
        (100, 30),
        (80, 24),
        (79, 24),
        (80, 23),
        (60, 20),
        (121, 40),
        (160, 40),
    ] {
        // act
        let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, width, height));
        let transcript = plan
            .transcript
            .unwrap_or_else(|| panic!("transcript must exist at {width}x{height}"));
        let composer = plan
            .composer
            .unwrap_or_else(|| panic!("composer must exist at {width}x{height}"));
        // assert
        assert_eq!(
            transcript.width, plan.shell.width,
            "transcript must span full shell width at {width}x{height}; transcript={transcript:?} shell={:?}",
            plan.shell
        );
        let expected_inset = STARTUP_COMPOSER_INSET_X;
        assert_eq!(
            composer.x,
            plan.shell.x.saturating_add(expected_inset),
            "composer must use freeze-matched horizontal inset at {width}x{height}; composer={composer:?} shell={:?}",
            plan.shell
        );
        assert_eq!(
            composer.width,
            plan.shell.width.saturating_sub(expected_inset.saturating_mul(2)),
            "composer must use freeze-matched width at {width}x{height}; composer={composer:?} shell={:?}",
            plan.shell
        );
        assert_eq!(
            transcript.x, plan.shell.x,
            "transcript must share shell left edge at {width}x{height}"
        );
        assert!(
            transcript.y + transcript.height <= composer.y,
            "transcript must sit above composer at {width}x{height}; transcript={transcript:?} composer={composer:?}"
        );
    }
}
