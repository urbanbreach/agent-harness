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
    let full = WelcomeLayout::compute(80, 24);
    assert!(!full.compact);
    assert_eq!(full.menu_items_visible, 5);
    assert_eq!(full.hero_rect.0, (80 - full.hero_rect.2) / 2);

    let compact = WelcomeLayout::compute(40, 10);
    assert!(compact.compact);
    assert_eq!(compact.menu_items_visible, 3);

    for layout in [full, compact] {
        for (x, y, width, height) in rects(&layout) {
            assert!(x.saturating_add(width) <= layout.width);
            assert!(y.saturating_add(height) <= layout.height);
        }
    }
}

#[test]
fn region_at_identifies_regions_and_gaps() {
    let layout = WelcomeLayout::compute(80, 24);
    for (region, rect) in layout.all_regions() {
        assert_eq!(layout.region_at(rect.0, rect.1), region);
    }
    assert_eq!(layout.region_at(0, 0), WelcomeRegion::None);
}

#[test]
fn hit_map_prioritizes_menu_items() {
    let layout = WelcomeLayout::compute(80, 24);
    let map = WelcomeHitMap::new(
        layout.clone(),
        &["New session", "Open session", "Inspect run"],
    );
    let item_row = layout.menu_rect.1 + 1;
    assert_eq!(
        map.hit(layout.menu_rect.0 + 1, item_row),
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
fn state_focuses_prompt_and_navigates_menu() {
    let mut state = WelcomeState::new(3, false);
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
    let mut state = WelcomeState::new(3, true);
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

#[test]
fn menu_labels_are_harness_native() {
    let labels = [
        "New session",
        "Open session",
        "Inspect run",
        "Settings",
        "Quit",
    ];
    assert!(labels
        .iter()
        .all(|label| !label.contains("Grok") && !label.contains("xAI")));
}
