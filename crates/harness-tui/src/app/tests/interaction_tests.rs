use super::*;
use crate::transcript_scroll::PageFlipState;
use crate::UnwrapOrAbort;

pub(super) fn space_on_transcript_focus_focuses_prompt_for_typing() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Details;
    assert!(app.composer.prompt_buffer.is_empty());

    app.handle_key(key(KeyCode::Char(' ')));

    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(app.composer.prompt_buffer, " ");
    assert_eq!(app.composer.prompt_cursor, 1);
}

pub(super) fn letter_on_transcript_focus_focuses_prompt_and_inserts_char() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Details;

    app.handle_key(key(KeyCode::Char('h')));

    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(app.composer.prompt_buffer, "h");
    assert_eq!(app.composer.prompt_cursor, 1);
}

pub(super) fn focus_returns_after_palette_close() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Details;

    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    assert!(app.palette_visible);
    assert_eq!(app.focus, Focus::Details);

    app.handle_key(key(KeyCode::Esc));
    assert!(!app.palette_visible);
    assert_eq!(app.focus, Focus::Details);
}

include!("interaction_welcome_tests.rs");

pub(super) fn details_drawer_toggles_without_stealing_transcript_state() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_a",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_a".into(),
            text: "First".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_a",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_a".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "First".to_string(),
            request_digest: "digest-a".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_b",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_b".into(),
            text: "Second".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_b",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_b".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Second".to_string(),
            request_digest: "digest-b".to_string(),
            metadata: None,
        }),
    ));

    app.transcript_view.follow_mode = false;
    app.focus = Focus::Details;
    app.transcript_view.selected_activity_index = 0;
    app.details_scroll = 7;

    app.live_details_drawer_open = true;
    assert!(app.details_drawer_open());
    assert_eq!(app.active_tab, Tab::Run);
    assert_eq!(app.focus, Focus::Details);
    assert!(!app.transcript_view.follow_mode);
    assert_eq!(app.transcript_view.selected_activity_index, 0);
    assert_eq!(app.details_scroll, 7);

    app.live_details_drawer_open = false;
    assert!(!app.details_drawer_open());
    assert_eq!(app.active_tab, Tab::Run);
    assert_eq!(app.focus, Focus::Details);
    assert!(!app.transcript_view.follow_mode);
    assert_eq!(app.transcript_view.selected_activity_index, 0);
    assert_eq!(app.details_scroll, 7);
}

pub(super) fn mouse_wheel_scrolls_transcript_without_stealing_focus() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.transcript_view.last_transcript_max_scroll.set(42);

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Transcript),
        None,
        None,
    );
    assert!(!app.transcript_view.follow_mode);
    assert_eq!(app.transcript_view.transcript_scroll, 3);
    assert_eq!(app.focus, Focus::Prompt);

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Transcript),
        None,
        None,
    );
    assert_eq!(app.transcript_view.transcript_scroll, 0);
    assert!(!app.transcript_view.follow_mode);

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Transcript),
        None,
        None,
    );
    assert!(app.transcript_view.follow_mode);
    assert_eq!(app.focus, Focus::Prompt);
}

pub(super) fn resize_invalidates_geometry_dependent_pointer_state() {
    // Given: hover state resolved against the currently rendered frame.
    let mut app = AppState::new_live(None, false, None);
    app.set_frame_area(TEST_FRAME_AREA);
    app.hovered_live_turn_stop = true;
    app.transcript_view.return_to_live_hovered = true;

    // When: the same geometry is observed, then the terminal width changes.
    app.set_frame_area(TEST_FRAME_AREA);
    let same_frame_kept_hover = app.hovered_live_turn_stop;
    let same_frame_kept_return_hover = app.transcript_view.return_to_live_hovered;
    app.set_frame_area(Rect::new(
        TEST_FRAME_AREA.x,
        TEST_FRAME_AREA.y,
        TEST_FRAME_AREA.width + 1,
        TEST_FRAME_AREA.height,
    ));

    // Then: stable geometry preserves hover, while resized geometry cannot keep stale hits.
    assert!(same_frame_kept_hover);
    assert!(same_frame_kept_return_hover);
    assert!(!app.hovered_live_turn_stop);
    assert!(!app.transcript_view.return_to_live_hovered);
}

pub(super) fn command_palette_mouse_hover_moves_keyboard_selection() {
    // Given: a rendered command palette with at least two selectable rows.
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    let model = crate::ui::ui_overlays::modal_surface_model(&app, TEST_FRAME_AREA)
        .expect("command palette surface model");
    let row_region = model
        .regions
        .iter()
        .find(|region| region.target == ModalTarget::Row(1))
        .expect("second command row region")
        .area;
    let layout = crate::ui::ui_overlays::modal_list_row_layout(row_region, model.max_scroll);
    let column = row_region.x;
    let row = row_region.y;
    let initial = app.palette_selected;

    // When: the pointer moves onto a different command row through its unpainted gutter.
    let handled = app.handle_mouse(
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

    // keeps the outer gutter on the modal surface, and paints the inset band
    // with the softer hover material.
    assert!(handled);
    assert_ne!(app.palette_selected, initial);
    assert_eq!(
        rendered_cell_bg(&app, row_region.x, row_region.y),
        app.theme().surface.canvas
    );
    assert_eq!(
        rendered_cell_bg(&app, layout.content.x, row_region.y),
        Color::Rgb(44, 44, 44)
    );

    let scrollbar = layout.scrollbar.expect("scrolling palette scrollbar");
    let scrollbar_row = model
        .regions
        .iter()
        .find(|region| region.target == ModalTarget::Row(2))
        .expect("third command row region")
        .area;
    let handled = app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: scrollbar.x,
            row: scrollbar_row.y,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    assert!(handled);
    assert_eq!(app.palette_selected, 1);
    assert_eq!(
        rendered_cell_bg(&app, scrollbar.x, scrollbar_row.y),
        app.theme().surface.canvas
    );
}

pub(super) fn command_palette_matching_mouse_release_activates_row() {
    // Given: the command palette points at the settings command.
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    for character in "settings".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    let (column, row) = modal_target_position(&app, TEST_FRAME_AREA, ModalTarget::Row(0));

    // When: the left button is pressed on the row without a release.
    let down_handled = app.handle_mouse(
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

    // Then: the press is consumed but activation waits for a matching release.
    assert!(down_handled);
    assert!(!app.settings_editor_visible);

    let up_handled = app.handle_mouse(
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

    assert!(up_handled);
    assert!(app.settings_editor_visible);
}

pub(super) fn command_palette_outside_matching_mouse_release_dismisses_top_overlay() {
    // Given: the command palette is the current top overlay.
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::CommandPalette));

    // When: the pointer is pressed outside the popup bounds without a release.
    let down_handled = app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    // Then: dismissal waits for a matching outside release.
    assert!(down_handled);
    assert!(app.palette_visible);

    let up_handled = app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    assert!(up_handled);
    assert!(!app.palette_visible);
}

pub(super) fn release_notes_outside_mouse_down_dismisses_and_blocks_lower_surface() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.focus = Focus::List;
    app.open_release_notes();
    let prompt_before = app.composer.prompt_buffer.clone();

    let handled = app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    assert!(handled);
    assert_eq!(app.overlay_stack().top(), None);
    assert_eq!(app.focus, Focus::List);
    assert_eq!(app.composer.prompt_buffer, prompt_before);
}

pub(super) fn release_notes_close_target_uses_matching_release_and_restores_focus() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.focus = Focus::List;
    app.open_release_notes();
    let model =
        crate::ui::ui_overlays::modal_surface_model(&app, TEST_FRAME_AREA).unwrap_or_abort();
    let close = model
        .regions
        .iter()
        .find(|region| region.target == ModalTarget::Close)
        .unwrap_or_abort()
        .area;
    let column = close.x;
    let row = close.y;

    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        app.handle_mouse(
            MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            TEST_FRAME_AREA,
            None,
            None,
            None,
        );
        if matches!(kind, MouseEventKind::Down(_)) {
            assert!(app.release_notes_visible);
        }
    }

    assert!(!app.release_notes_visible);
    assert_eq!(app.focus, Focus::List);
}

pub(super) fn release_notes_keyboard_scroll_supports_steps_pages_and_bounds() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.set_frame_area(Rect::new(0, 0, 100, 30));
    app.open_release_notes();

    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.release_notes_scroll, 3);
    app.handle_key(key(KeyCode::End));
    let end = app.release_notes_scroll;
    assert!(end >= 3);
    app.handle_key(key(KeyCode::PageUp));
    assert!(app.release_notes_scroll < end);
    app.handle_key(key(KeyCode::Home));
    assert_eq!(app.release_notes_scroll, 0);
}

pub(super) fn command_palette_drag_cancels_armed_row_activation() {
    // Given: a row is armed by a left-button press.
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    for character in "settings".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    let (column, row) = modal_target_position(&app, TEST_FRAME_AREA, ModalTarget::Row(0));
    app.handle_mouse(
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

    // When: the pointer drags before releasing over the original row.
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: column.saturating_add(1),
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

    // Then: the stale press cannot activate the row.
    assert!(!app.settings_editor_visible);
    assert!(app.palette_visible);
}

pub(super) fn command_palette_release_on_different_target_cancels_activation() {
    // Given: the first command row is armed by a left-button press.
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    let first = modal_target_position(&app, TEST_FRAME_AREA, ModalTarget::Row(0));
    let second = modal_target_position(&app, TEST_FRAME_AREA, ModalTarget::Row(1));
    let initial = app.palette_selected;
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: first.0,
            row: first.1,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    // When: the button is released over a different row.
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: second.0,
            row: second.1,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    // Then: no command is activated and selection is unchanged by the release.
    assert!(app.palette_visible);
    assert_eq!(app.palette_selected, initial);
}

pub(super) fn command_palette_wheel_outside_popup_does_not_scroll() {
    // Given: a compact scrollable palette with pointer-owned offset initialized.
    let area = Rect::new(0, 0, 60, 20);
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    let model = crate::ui::ui_overlays::modal_surface_model(&app, area)
        .expect("command palette surface model");
    assert!(model.max_scroll > 0);
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: model.popup.x,
            row: model.popup.y,
            modifiers: KeyModifiers::NONE,
        },
        area,
        None,
        None,
        None,
    );

    // When: the wheel moves outside the owned popup.
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        area,
        None,
        None,
        None,
    );

    // Then: the modal viewport stays anchored.
    assert_eq!(
        app.modal_visual_offset(
            ModalSurfaceKey::Overlay {
                kind: OverlayKind::CommandPalette,
                view: ModalViewKey::Primary,
            },
            0,
            model.max_scroll,
        ),
        0
    );
}

pub(super) fn command_palette_scrollbar_drag_is_anchored_and_never_selects_rows() {
    // Given: a compact palette with a typed scrollbar and the first row selected.
    let area = Rect::new(0, 0, 60, 20);
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    app.palette_selected = 0;
    let model = crate::ui::ui_overlays::modal_surface_model(&app, area)
        .expect("command palette surface model");
    let scrollbar = model.scrollbar.expect("scrollbar geometry");
    assert_eq!(
        model.hit(scrollbar.track.x, scrollbar.track.y),
        Some(ModalTarget::Scrollbar)
    );

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: scrollbar.track.x,
            row: scrollbar.track.y,
            modifiers: KeyModifiers::NONE,
        },
        area,
        None,
        None,
        None,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: scrollbar.thumb.x,
            row: scrollbar.thumb.y,
            modifiers: KeyModifiers::NONE,
        },
        area,
        None,
        None,
        None,
    );

    // When: the armed thumb is dragged beyond the end of its track.
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: scrollbar.track.x,
            row: u16::MAX,
            modifiers: KeyModifiers::NONE,
        },
        area,
        None,
        None,
        None,
    );

    // Then: offset clamps to the range while the row selection stays unchanged.
    assert_eq!(app.palette_selected, 0);
    assert_eq!(
        app.modal_visual_offset(
            ModalSurfaceKey::Overlay {
                kind: OverlayKind::CommandPalette,
                view: ModalViewKey::Primary,
            },
            0,
            model.max_scroll,
        ),
        model.max_scroll
    );
}

pub(super) fn control_modified_release_invalidates_armed_modal_target() {
    // Given: the settings command row is armed by an unmodified press.
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    for character in "settings".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    let (column, row) = modal_target_position(&app, TEST_FRAME_AREA, ModalTarget::Row(0));
    app.handle_mouse(
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

    // When: the matching release carries a control modifier.
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::CONTROL,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    // Then: the modified event invalidates the press instead of activating it.
    assert!(!app.settings_editor_visible);
    assert!(app.palette_visible);
}

pub(super) fn modal_footer_matching_release_activates_action() {
    // Given: the error-details cancel footer is visible.
    let mut app = AppState::new_live(None, false, None);
    app.error_details_visible = true;
    let target = modal_target_position(
        &app,
        TEST_FRAME_AREA,
        ModalTarget::Footer(ModalAction::Cancel),
    );

    // When: the footer receives a complete left press and release.
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        app.handle_mouse(
            MouseEvent {
                kind,
                column: target.0,
                row: target.1,
                modifiers: KeyModifiers::NONE,
            },
            TEST_FRAME_AREA,
            None,
            None,
            None,
        );
    }

    // Then: the footer action closes the modal only on the matching release.
    assert!(!app.error_details_visible);
}

include!("interaction_tests_part2_test.rs");
include!("interaction_tests_part3_test.rs");
