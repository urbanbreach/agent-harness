fn startup_with_affordances_visible() -> AppState {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(200));
    app
}

pub(super) fn welcome_mouse_move_applies_hover_state_to_the_action_row() {
    for frame_area in [TEST_FRAME_AREA, Rect::new(0, 0, 80, 40)] {
        let mut app = startup_with_affordances_visible();
        let startup_area = FrameLayoutPlan::for_app(&app, frame_area)
            .transcript
            .unwrap_or_abort();
        let layout = app.welcome_layout(startup_area);
        let header = layout.changelog_header_rect.unwrap_or_abort();
        let mut terminal =
            Terminal::new(TestBackend::new(frame_area.width, frame_area.height))
                .unwrap_or_abort();

        for (target, header_hovered, hovered_action) in [
            (layout.action_rects[0], false, Some(0)),
            (layout.action_rects[2], false, Some(2)),
            (header, true, None),
            (layout.action_rects[2], false, Some(2)),
            (header, true, None),
            ((0, 0, 1, 1), false, None),
        ] {
            let changed = app.handle_mouse(
                MouseEvent {
                    kind: MouseEventKind::Moved,
                    column: target.0,
                    row: target.1,
                    modifiers: KeyModifiers::NONE,
                },
                frame_area,
                None,
                None,
                None,
            );
            terminal
                .draw(|frame| render_app(frame, &app))
                .unwrap_or_abort();
            let buffer = terminal.backend().buffer();
            let heading = &buffer[(header.0, header.1)];

            assert!(changed, "moving to {target:?} must redraw at {frame_area:?}");
            assert_eq!(
                (heading.fg, heading.modifier.contains(Modifier::DIM)),
                if header_hovered {
                    (app.theme().text.primary, false)
                } else {
                    (app.theme().text.secondary, true)
                },
                "heading hover at {target:?} in {frame_area:?}"
            );
            for (index, rect) in layout.action_rects.iter().enumerate() {
                assert_eq!(
                    buffer[(rect.0, rect.1)].bg,
                    if hovered_action == Some(index) {
                        app.theme().surface.card
                    } else {
                        app.theme().surface.canvas
                    },
                    "menu row {index} at {target:?} in {frame_area:?}"
                );
            }
        }
    }
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


pub(super) fn welcome_changelog_expanded_mouse_down_opens_release_notes_and_up_is_inert() {
    // arrange: the semantic-ready startup state already exposes expanded Changelog content.
    let mut app = AppState::new_startup(Vec::new(), None);
    let frame_area = Rect::new(0, 0, 100, 30);
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
    // Given: the pointer presses the Changelog heading at the canonical viewport.
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

    // Then: opening the modal retains hover on the heading alone.
    assert!(app.welcome_state().changelog_header_hovered());
    assert_eq!(app.welcome_state().hovered_action(), None);
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
