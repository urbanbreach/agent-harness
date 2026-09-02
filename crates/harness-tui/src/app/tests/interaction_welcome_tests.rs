fn startup_with_affordances_visible() -> AppState {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(200));
    app
}

pub(super) fn welcome_mouse_move_applies_hover_state_to_the_action_row() {
    let mut app = startup_with_affordances_visible();
    let (column, row) = transcript_click_position(&app, "New worktree");

    let changed = app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    assert_eq!(
        (
            changed,
            app.welcome_state().hovered_action(),
            rendered_cell_bg(&app, column, row),
        ),
        (true, Some(0), app.theme().surface.card)
    );
}

pub(super) fn welcome_mouse_move_away_clears_hover_state_and_row_surface() {
    let mut app = startup_with_affordances_visible();
    let (column, row) = transcript_click_position(&app, "New worktree");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    let changed = app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    assert_eq!(
        (
            changed,
            app.welcome_state().hovered_action(),
            rendered_cell_bg(&app, column, row),
        ),
        (true, None, app.theme().surface.canvas)
    );
}

pub(super) fn welcome_changelog_mouse_down_expands_the_startup_panel() {
    // arrange
    let mut app = startup_with_affordances_visible();
    let (column, row) = transcript_click_position(&app, "Changelog");

    // act
    let changed = app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    // assert
    assert!(changed);
    assert!(app.startup_welcome_expanded());
    assert!(app.overlay_stack().top().is_none());
    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(
        app.welcome_state().focus(),
        crate::welcome_surface::WelcomeFocus::Prompt
    );
}

pub(super) fn welcome_changelog_expanded_mouse_down_opens_release_notes_and_up_is_inert() {
    // arrange: the semantic-ready startup state already exposes expanded Changelog content.
    let mut app = AppState::new_startup(Vec::new(), None);
    let frame_area = Rect::new(0, 0, 100, 30);
    app.expand_startup_changelog();
    let (column, row) = transcript_click_position_in_area(&app, frame_area, "Changelog");

    // act: physical down activates the expanded target; release follows afterward.
    let down_changed = app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        frame_area,
        None,
        None,
        None,
    );
    let overlay_after_down = app.overlay_stack().top();
    let focus_after_down = app.focus;
    let scroll_after_down = app.release_notes_scroll;
    let up_changed = app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        frame_area,
        None,
        None,
        None,
    );

    // assert: down opens the modal and up neither closes nor replaces it.
    assert!(down_changed);
    assert_eq!(overlay_after_down, Some(OverlayKind::ReleaseNotes));
    assert_eq!(app.overlay_stack().top(), overlay_after_down);
    assert_eq!(app.focus, focus_after_down);
    assert_eq!(app.release_notes_scroll, scroll_after_down);
    assert!(app.overlay_stack().blocks_pointer_interaction());
    assert!(!up_changed);
}

pub(super) fn welcome_changelog_release_away_cancels_modal_activation() {
    // Given: Changelog is pressed at its collapsed-menu location.
    let mut app = startup_with_affordances_visible();
    let frame_area = Rect::new(0, 0, 100, 30);
    let (column, row) = transcript_click_position_in_area(&app, frame_area, "Changelog");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        frame_area,
        None,
        None,
        None,
    );

    // When: the pointer releases away from the startup panel.
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        frame_area,
        None,
        None,
        None,
    );

    // Then: the inline preview remains, but no modal is activated.
    assert!(app.startup_welcome_expanded());
    assert!(app.overlay_stack().top().is_none());
}

pub(super) fn welcome_changelog_drag_cancels_modal_activation() {
    let mut app = startup_with_affordances_visible();
    let frame_area = Rect::new(0, 0, 100, 30);
    let (column, row) = transcript_click_position_in_area(&app, frame_area, "Changelog");
    for event in [
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: column.saturating_add(1),
            row,
            modifiers: KeyModifiers::NONE,
        },
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
    ] {
        app.handle_mouse(event, frame_area, None, None, None);
    }

    assert!(app.startup_welcome_expanded());
    assert!(app.overlay_stack().top().is_none());
}

pub(super) fn welcome_changelog_keyboard_activation_opens_modal_and_restores_focus() {
    // Given: keyboard focus is on the Changelog menu action.
    let mut app = AppState::new_startup(Vec::new(), None);
    for code in [KeyCode::Tab, KeyCode::Down, KeyCode::Down] {
        app.handle_key(key(code));
    }
    assert_eq!(app.focus, Focus::List);

    // When: Enter opens release notes and Esc closes it.
    app.handle_key(key(KeyCode::Enter));
    assert!(app.overlay_stack().top().is_some());
    app.handle_key(key(KeyCode::Esc));

    // Then: the exact pre-modal focus is restored.
    assert!(app.overlay_stack().top().is_none());
    assert_eq!(app.focus, Focus::List);
}

pub(super) fn welcome_changelog_mouse_down_preserves_pointer_hover_for_inline_preview() {
    // Given: the pointer presses the collapsed Changelog action at the canonical viewport.
    let mut app = startup_with_affordances_visible();
    let frame_area = Rect::new(0, 0, 100, 30);
    let (column, row) = transcript_click_position_in_area(&app, frame_area, "Changelog");

    // When: the physical click completes without a pointer-move event.
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        frame_area,
        None,
        None,
        None,
    );

    // Then: expansion retains the pointer-owned Changelog hover state.
    assert_eq!(app.welcome_state().hovered_action(), Some(2));
}

pub(super) fn welcome_changelog_mouse_down_renders_a_bright_expanded_header() {
    // Given: Changelog was activated by a real pointer down/up at 100x30.
    let mut app = startup_with_affordances_visible();
    let frame_area = Rect::new(0, 0, 100, 30);
    let (column, row) = transcript_click_position_in_area(&app, frame_area, "Changelog");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        frame_area,
        None,
        None,
        None,
    );

    // When: the expanded header cells are rendered.
    let styles = rendered_changelog_header_styles(&app, frame_area);

    // Then: all nine cells are primary and non-DIM.
    assert!(styles.iter().all(|(foreground, modifiers)| {
        *foreground == app.theme().text.primary && !modifiers.contains(Modifier::DIM)
    }));
}

pub(super) fn welcome_changelog_pointer_move_away_restores_the_dim_header() {
    // Given: a pointer click expanded Changelog and retained its hover state.
    let mut app = startup_with_affordances_visible();
    let frame_area = Rect::new(0, 0, 100, 30);
    let (column, row) = transcript_click_position_in_area(&app, frame_area, "Changelog");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        frame_area,
        None,
        None,
        None,
    );

    // When: the pointer moves away from the welcome block.
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        frame_area,
        None,
        None,
        None,
    );

    // Then: all nine header cells return to DIM.
    assert!(rendered_changelog_header_styles(&app, frame_area)
        .iter()
        .all(|(_, modifiers)| modifiers.contains(Modifier::DIM)));
}

pub(super) fn welcome_changelog_keyboard_activation_does_not_synthesize_pointer_hover() {
    // Given: keyboard focus is moved from Prompt to Changelog.
    let mut app = AppState::new_startup(Vec::new(), None);
    for code in [KeyCode::Tab, KeyCode::Down, KeyCode::Down] {
        app.handle_key(key(code));
    }

    // When: Enter activates the real Changelog action.
    app.handle_key(key(KeyCode::Enter));

    assert!(app.welcome_state().hovered_action().is_none());
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::ReleaseNotes));
}

pub(super) fn welcome_changelog_click_brightens_the_compact_section_header() {
    // Given: the compact 80x24 startup surface exposes the Changelog action.
    let mut app = startup_with_affordances_visible();
    let frame_area = Rect::new(0, 0, 80, 24);
    let (column, row) = transcript_click_position_in_area(&app, frame_area, "Changelog");

    // When: a real down/up click activates it without moving the pointer.
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        frame_area,
        None,
        None,
        None,
    );

    // Then: the compact informational section uses the same bright hover style.
    assert!(rendered_compact_changelog_section_styles(&app, frame_area)
        .iter()
        .all(|(foreground, modifiers)| {
            *foreground == app.theme().text.primary && !modifiers.contains(Modifier::DIM)
        }));
}
