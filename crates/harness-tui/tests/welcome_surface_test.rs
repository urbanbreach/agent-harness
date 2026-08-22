use harness_tui::welcome_surface::{
    InputResult, WelcomeFocus, WelcomeHit, WelcomeHitMap, WelcomeInput, WelcomeLayout,
    WelcomeRegion, WelcomeState,
};

fn rects(layout: &WelcomeLayout) -> [(u16, u16, u16, u16); 5] {
    [
        layout.hero_rect,
        layout.logo_rect,
        layout.menu_rect,
        layout.prompt_rect,
        layout.status_rect,
    ]
}

#[test]
fn layout_matches_full_and_compact_viewports() {
    // arrange
    let full = WelcomeLayout::compute(120, 32);
    assert!(!full.compact);
    assert_eq!(full.menu_items_visible, 4);
    assert_eq!(full.panel_rect.map(|rect| rect.0), Some(3));

    let compact = WelcomeLayout::compute(40, 10);
    assert!(compact.compact);
    assert_eq!(compact.menu_items_visible, 4);

    // act
    for layout in [full, compact] {
        for (x, y, width, height) in rects(&layout) {
            // assert
            assert!(x.saturating_add(width) <= layout.width);
            assert!(y.saturating_add(height) <= layout.height);
        }
    }
}

#[test]
fn responsive_hero_stacks_below_90_columns_and_splits_at_or_above_90() {
    // arrange
    // act
    for (width, stacked) in [(80, true), (90, false), (100, false), (120, false)] {
        let layout = WelcomeLayout::compute(width, 19);

        // assert
        assert_eq!(
            layout.compact, stacked,
            "{width} columns must select the expected hero topology"
        );
        if stacked {
            assert!(layout.panel_rect.is_none());
            assert_eq!(layout.action_rects[0].0, layout.content_rect.0);
        } else {
            assert!(layout.panel_rect.is_some());
            assert_eq!(layout.logo_rect.2, 15);
            assert_eq!(layout.logo_rect.3, 7);
            assert!(
                layout.action_rects[0].0
                    >= layout.logo_rect.0.saturating_add(layout.logo_rect.2 + 2),
                "wide actions must occupy the details column at {width} columns"
            );
            assert_eq!(
                layout.action_rects[0].0.saturating_add(1),
                layout.content_rect.0.saturating_add(18),
                "the action hit row must include the focus marker at {width} columns"
            );
        }
    }
}

#[test]
fn hero_breakpoint_depends_on_width_even_when_height_is_short() {
    // arrange
    // act
    let stacked = WelcomeLayout::compute(89, 20);
    let split = WelcomeLayout::compute(90, 20);

    // assert
    assert!(stacked.compact);
    assert!(!split.compact);
}

#[test]
fn wide_hero_uses_compact_topology_when_the_panel_would_clip() {
    // arrange
    // act
    let clipped = WelcomeLayout::compute(90, 16);
    let complete = WelcomeLayout::compute(90, 19);

    // assert
    assert!(clipped.compact);
    assert!(clipped.panel_rect.is_none());
    assert!(!complete.compact);
    assert_eq!(complete.panel_rect.map(|panel| panel.3), Some(15));
}

#[test]
fn region_at_identifies_regions_and_gaps() {
    // arrange
    // act
    let layout = WelcomeLayout::compute(120, 32);
    for (region, rect) in layout.all_regions() {
        // assert
        assert_eq!(layout.region_at(rect.0, rect.1), region);
    }
    assert_eq!(layout.region_at(0, 0), WelcomeRegion::None);
}

#[test]
fn hit_map_prioritizes_menu_items() {
    // arrange
    // act
    let layout = WelcomeLayout::compute(120, 32);
    let map = WelcomeHitMap::new(
        layout.clone(),
        &["New session", "Open session", "Inspect run"],
    );
    let item_rect = layout.action_rects[0];
    // assert
    assert_eq!(
        map.hit(item_rect.0, item_rect.1),
        Some(WelcomeHit {
            region: WelcomeRegion::Menu,
            item_index: Some(0)
        })
    );
    assert_eq!(
        map.hit(layout.hero_rect.0, layout.hero_rect.1),
        Some(WelcomeHit {
            region: WelcomeRegion::Hero,
            item_index: None
        })
    );
    assert_eq!(map.hit(0, 0), None);
}

#[test]
fn welcome_dismissal_is_latched_until_reset() {
    // arrange
    let mut state = WelcomeState::new(3, false);

    // act
    state.dismiss_for_input();

    // assert
    assert!(state.is_dismissed());
}

#[test]
fn compact_layout_stays_inside_tiny_viewports() {
    // arrange
    // act
    for (width, height) in [(1, 1), (8, 4), (20, 12)] {
        let layout = WelcomeLayout::compute(width, height);
        for (x, y, rect_width, rect_height) in rects(&layout) {
            // assert
            assert!(x.saturating_add(rect_width) <= width);
            assert!(y.saturating_add(rect_height) <= height);
        }
        assert!(layout.content_rect.0.saturating_add(layout.content_rect.2) <= width);
        assert!(layout.content_rect.1.saturating_add(layout.content_rect.3) <= height);
    }
}

#[test]
fn state_focuses_the_clicked_menu_item() {
    // arrange
    // act
    let mut state = WelcomeState::new(3, false);

    // assert
    assert_eq!(state.focus_menu_item(2), InputResult::FocusChanged);
    assert_eq!(state.focus(), WelcomeFocus::Menu(2));
}

#[test]
fn state_tracks_the_hovered_menu_item_independently_from_focus() {
    // arrange
    let mut state = WelcomeState::new(3, false);

    // act
    let changed = state.set_hovered_action(Some(1));

    // assert
    assert!(changed);
    assert_eq!(state.hovered_action(), Some(1));
    assert_eq!(state.focus(), WelcomeFocus::Prompt);
}

#[test]
fn state_clears_an_out_of_range_hover_target() {
    // arrange
    let mut state = WelcomeState::new(3, false);
    state.set_hovered_action(Some(1));

    // act
    let changed = state.set_hovered_action(Some(3));

    // assert
    assert!(changed);
    assert_eq!(state.hovered_action(), None);
}

#[test]
fn state_focuses_prompt_and_navigates_menu() {
    // arrange
    // act
    let mut state = WelcomeState::new(3, false);
    // assert
    assert_eq!(state.focus(), WelcomeFocus::Prompt);
    assert_eq!(
        state.handle(WelcomeInput::FocusMenu),
        InputResult::FocusChanged
    );
    assert_eq!(state.focus(), WelcomeFocus::Menu(0));
    assert_eq!(state.handle(WelcomeInput::MoveDown), InputResult::Navigated);
    assert_eq!(state.focus(), WelcomeFocus::Menu(1));
    assert_eq!(
        state.handle(WelcomeInput::Activate),
        InputResult::MenuItemActivated(1)
    );
    assert_eq!(
        state.handle(WelcomeInput::FocusPrompt),
        InputResult::FocusChanged
    );
    assert_eq!(
        state.handle(WelcomeInput::Activate),
        InputResult::PromptActivated
    );
}

#[test]
fn state_select_cycles_focus_and_menu_navigation_wraps() {
    // arrange
    // act
    let mut state = WelcomeState::new(3, true);
    // assert
    assert_eq!(
        state.handle(WelcomeInput::Select),
        InputResult::FocusChanged
    );
    assert_eq!(state.focus(), WelcomeFocus::Menu(0));
    assert_eq!(state.handle(WelcomeInput::MoveUp), InputResult::Navigated);
    assert_eq!(state.focus(), WelcomeFocus::Menu(2));
    assert_eq!(state.handle(WelcomeInput::MoveDown), InputResult::Navigated);
    assert_eq!(state.focus(), WelcomeFocus::Menu(0));
    assert_eq!(
        state.handle(WelcomeInput::Select),
        InputResult::FocusChanged
    );
    assert_eq!(state.focus(), WelcomeFocus::StatusBar);
    assert_eq!(
        state.handle(WelcomeInput::Select),
        InputResult::FocusChanged
    );
    assert_eq!(state.focus(), WelcomeFocus::Prompt);
}
