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

pub(super) fn welcome_mouse_move_applies_hover_state_to_the_action_row() {
    let mut app = AppState::new_startup(Vec::new(), None);
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
    let mut app = AppState::new_startup(Vec::new(), None);
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

    // When: the same geometry is observed, then the terminal width changes.
    app.set_frame_area(TEST_FRAME_AREA);
    let same_frame_kept_hover = app.hovered_live_turn_stop;
    app.set_frame_area(Rect::new(
        TEST_FRAME_AREA.x,
        TEST_FRAME_AREA.y,
        TEST_FRAME_AREA.width + 1,
        TEST_FRAME_AREA.height,
    ));

    // Then: stable geometry preserves hover, while resized geometry cannot keep stale hits.
    assert!(same_frame_kept_hover);
    assert!(!app.hovered_live_turn_stop);
}

pub(super) fn command_palette_mouse_hover_moves_keyboard_selection() {
    // Given: a rendered command palette with at least two selectable rows.
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    let (column, row) = modal_target_position(&app, TEST_FRAME_AREA, ModalTarget::Row(1));
    let initial = app.palette_selected;

    // When: the pointer moves onto a different command row.
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

    // Then: Grok's picker contract moves the keyboard selection with hover.
    assert!(handled);
    assert_ne!(app.palette_selected, initial);
}

pub(super) fn command_palette_mouse_down_activates_row_without_release() {
    // Given: the command palette points at the settings command.
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    for character in "settings".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    let (column, row) = modal_target_position(&app, TEST_FRAME_AREA, ModalTarget::Row(0));

    // When: the left button is pressed on the row, without a matching Up event.
    let handled = app.handle_mouse(
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

    // Then: the row activates immediately, matching Grok picker behavior.
    assert!(handled);
    assert!(app.settings_editor_visible);
}

pub(super) fn command_palette_outside_mouse_down_dismisses_top_overlay() {
    // Given: the command palette is the current top overlay.
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::CommandPalette));

    // When: the pointer is pressed outside the popup bounds.
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

    // Then: only the top modal closes and the event is consumed.
    assert!(handled);
    assert!(!app.palette_visible);
}

pub(super) fn command_palette_wheel_scrolls_three_rows_without_changing_selection() {
    // Given: a compact command palette whose command list exceeds the viewport.
    let area = Rect::new(0, 0, 60, 20);
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    app.palette_selected = 0;
    let before = render_text(&app, area.width, area.height);

    // When: one wheel-down event is delivered over the list.
    let handled = app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: area.width / 2,
            row: area.height / 2,
            modifiers: KeyModifiers::NONE,
        },
        area,
        None,
        None,
        None,
    );
    let after = render_text(&app, area.width, area.height);

    // Then: the visual offset advances by Grok's three-row wheel step while
    // keyboard selection remains unchanged.
    assert!(handled);
    assert_eq!(app.palette_selected, 0);
    assert_ne!(after, before);
}

pub(super) fn top_modal_preempts_pointer_targets_from_lower_overlays() {
    // Given: a theme dialog rendered above an already-open command palette.
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    let palette_target = modal_target_position(&app, TEST_FRAME_AREA, ModalTarget::Row(1));
    app.theme_dialog_visible = true;
    let initial_palette_selection = app.palette_selected;

    // When: the pointer moves where the lower palette row was rendered.
    let handled = app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column: palette_target.0,
            row: palette_target.1,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    // Then: the top modal consumes routing and the lower selection is untouched.
    assert!(handled);
    assert_eq!(app.palette_selected, initial_palette_selection);
    assert_eq!(app.overlay_stack().top(), Some(OverlayKind::ThemeDialog));
}

pub(super) fn modal_resize_invalidates_stale_close_hover_geometry() {
    // Given: the palette close button is hovered in the current frame.
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    app.set_frame_area(TEST_FRAME_AREA);
    let (column, row) = modal_target_position(&app, TEST_FRAME_AREA, ModalTarget::Close);
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
    assert!(app.modal_close_hovered_for_test());

    // When: frame geometry changes before another pointer event arrives.
    app.set_frame_area(Rect::new(0, 0, 80, 24));

    // Then: stale chrome hover cannot survive the resize generation.
    assert!(!app.modal_close_hovered_for_test());
}

pub(super) fn first_modal_pointer_contact_preserves_keyboard_derived_scroll() {
    // Given: keyboard selection has moved below the command palette viewport.
    let area = Rect::new(0, 0, 60, 20);
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    app.palette_selected = app.palette_filtered.len().saturating_sub(1);
    let model = crate::ui::ui_overlays::modal_surface_model(&app, area)
        .expect("command palette surface model");
    assert!(model.max_scroll > 0);
    let (column, row) = modal_target_position(&app, area, ModalTarget::Close);

    // When: the first pointer event lands on chrome rather than a row.
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Moved,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
        None,
        None,
        None,
    );

    // Then: binding pointer state keeps the viewport that keyboard selection rendered.
    let offset = app.modal_visual_offset(
        ModalSurfaceKey::Overlay {
            kind: OverlayKind::CommandPalette,
            view: ModalViewKey::Primary,
        },
        0,
        model.max_scroll,
    );
    assert!(offset > 0);
}

pub(super) fn toggles_wheel_offset_drives_rendered_rows() {
    // Given: a compact toggles menu with more visual rows than its viewport.
    let area = Rect::new(0, 0, 60, 20);
    let mut app = AppState::new_live(None, false, None);
    app.open_toggles_menu();
    let model = crate::ui::ui_overlays::modal_surface_model(&app, area)
        .expect("toggles modal surface model");
    assert!(model.max_scroll > 0);
    let before = render_text(&app, area.width, area.height);

    // When: the wheel advances the shared modal viewport.
    let handled = app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: area.width / 2,
            row: area.height / 2,
            modifiers: KeyModifiers::NONE,
        },
        area,
        None,
        None,
        None,
    );
    let after = render_text(&app, area.width, area.height);

    // Then: the rendered rows move with the pointer-owned hit model.
    assert!(handled);
    assert_ne!(after, before);
}

pub(super) fn modal_keyboard_input_invalidates_pointer_owned_state() {
    // Given: pointer state is bound to an open command palette.
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    let (column, row) = modal_target_position(&app, TEST_FRAME_AREA, ModalTarget::Close);
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
    assert!(app.modal_close_hovered_for_test());

    // When: keyboard navigation becomes the active modal interaction source.
    app.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));

    // Then: stale hover and wheel ownership cannot survive that generation.
    assert!(!app.modal_close_hovered_for_test());
}

pub(super) fn yolo_footer_targets_match_visible_action_spans() {
    // Given: the nested YOLO confirmation footer is visible.
    let mut app = AppState::new_live(None, false, None);
    app.open_toggles_menu();
    app.toggles_yolo_confirm_visible = true;
    let model = crate::ui::ui_overlays::modal_surface_model(&app, TEST_FRAME_AREA)
        .expect("YOLO confirmation surface model");

    // When: confirm and cancel target geometry is read from the shared model.
    let confirm = model
        .regions
        .iter()
        .find(|region| region.target == ModalTarget::Footer(ModalAction::Activate))
        .expect("confirm target");
    let cancel = model
        .regions
        .iter()
        .find(|region| region.target == ModalTarget::Footer(ModalAction::Cancel))
        .expect("cancel target");

    // Then: each target covers only its rendered label, not the whole footer.
    assert_eq!(
        (confirm.area, cancel.area),
        (
            Rect::new(
                model.popup.x.saturating_add(2),
                model.popup.bottom() - 2,
                13,
                1
            ),
            Rect::new(
                model.popup.x.saturating_add(18),
                model.popup.bottom() - 2,
                10,
                1
            ),
        )
    );
}

pub(super) fn yolo_footer_remains_visible_when_filter_shrinks_parent() {
    // Given: the toggles menu is filtered to the single YOLO entry.
    let mut app = AppState::new_live(None, false, None);
    app.open_toggles_menu();
    for character in "yolo".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }

    // When: the selected entry opens its confirmation dialog.
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let rendered = render_text(&app, 100, 30);

    // Then: the exact pointer-action footer remains visibly rendered.
    assert!(rendered.contains("Enter confirm   Esc cancel"));
}

pub(super) fn error_footer_targets_match_visible_action_spans() {
    // Given: error details renders its fallback message and footer actions.
    let mut app = AppState::new_live(None, false, None);
    app.error_details_visible = true;
    let model = crate::ui::ui_overlays::modal_surface_model(&app, TEST_FRAME_AREA)
        .expect("error details surface model");

    // When: close and resubmit target geometry is read from the shared model.
    let close = model
        .regions
        .iter()
        .find(|region| region.target == ModalTarget::Footer(ModalAction::Cancel))
        .expect("footer close target");
    let resubmit = model
        .regions
        .iter()
        .find(|region| region.target == ModalTarget::Footer(ModalAction::Resubmit))
        .expect("resubmit target");

    // Then: the targets follow the rendered fallback footer rather than a fixed bottom row.
    assert_eq!(
        (close.area, resubmit.area),
        (
            Rect::new(
                model.popup.x.saturating_add(1),
                model.popup.y.saturating_add(5),
                9,
                1
            ),
            Rect::new(
                model.popup.x.saturating_add(15),
                model.popup.y.saturating_add(5),
                10,
                1
            ),
        )
    );
}

fn modal_target_position(app: &AppState, area: Rect, target: ModalTarget) -> (u16, u16) {
    let model =
        crate::ui::ui_overlays::modal_surface_model(app, area).expect("active modal surface model");
    let region = model
        .regions
        .iter()
        .find(|region| region.target == target)
        .expect("target region");
    (
        region.area.x.saturating_add(region.area.width / 2),
        region.area.y.saturating_add(region.area.height / 2),
    )
}

pub(super) fn pointer_drag_suppresses_stale_hover_feedback() {
    // Given: a hover affordance was active before a pointer drag began.
    let mut app = AppState::new_live(None, false, None);
    app.hovered_live_turn_stop = true;

    // When: a left-button drag is delivered outside an active selection.
    let changed = app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 5,
            row: 5,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    // Then: hover feedback is cleared for the drag generation.
    assert!(changed);
    assert!(!app.hovered_live_turn_stop);
}

pub(super) fn transcript_navigation_keys_match_scroll_expectations() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Details;
    app.transcript_view.last_transcript_max_scroll.set(42);
    app.transcript_view.last_transcript_viewport_height.set(12);

    app.handle_key(key(KeyCode::PageUp));
    assert_eq!(app.transcript_view.transcript_scroll, 10);
    assert!(!app.transcript_view.follow_mode);

    app.handle_key(key(KeyCode::PageDown));
    assert_eq!(app.transcript_view.transcript_scroll, 0);
    assert!(app.transcript_view.follow_mode);

    app.handle_key(key(KeyCode::PageDown));
    assert_eq!(app.transcript_view.transcript_scroll, 0);
    assert!(app.transcript_view.follow_mode);

    app.handle_key(key(KeyCode::Home));
    assert_eq!(app.transcript_view.transcript_scroll, 42);
    assert!(!app.transcript_view.follow_mode);

    app.handle_key(key(KeyCode::PageDown));
    assert_eq!(app.transcript_view.transcript_scroll, 32);
    assert!(!app.transcript_view.follow_mode);

    app.handle_key(key(KeyCode::End));
    assert_eq!(app.transcript_view.transcript_scroll, 0);
    assert!(app.transcript_view.follow_mode);
}

fn detached_resize_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    for turn in 0..8_u64 {
        let request_id = format!("req_resize_{turn}");
        let first_seq = turn * 3 + 1;
        app.ingest_event(envelope(
            first_seq,
            &request_id,
            EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
                request_id: request_id.clone().into(),
                text: format!("resize prompt {turn}"),
            }),
        ));
        app.ingest_event(envelope(
            first_seq + 1,
            &request_id,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.clone().into(),
                provider_id: "mock".to_string(),
                model_id: "model-resize".to_string(),
                prompt_summary: format!("resize prompt {turn}"),
                request_digest: format!("digest-resize-{turn}"),
                metadata: None,
            }),
        ));
        app.ingest_event(envelope(
            first_seq + 2,
            &request_id,
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.clone().into(),
                delta: format!("resize response {turn}\nrow a\nrow b\nrow c\nRESIZE_TAIL_{turn}"),
            }),
        ));
    }
    let compact = Rect::new(0, 0, 80, 20);
    app.set_frame_area(compact);
    let _ = render_text(&app, compact.width, compact.height);
    let max_scroll = app.transcript_view.last_transcript_max_scroll.get();
    assert!(max_scroll > 5, "fixture must provide detached scroll range");
    app.set_transcript_page_flip_state(PageFlipState::Idle.begin(0).preserve_at(max_scroll));
    app.scroll_page_up(5);
    app
}

pub(super) fn detached_page_flip_reconciles_when_resize_reaches_bottom() {
    // Given: detached transcript navigation in a compact viewport with scrollable history.
    let mut app = detached_resize_app();

    // When: reflow into a viewport tall enough to reach the transcript bottom.
    let expanded = Rect::new(0, 0, 140, 100);
    app.set_frame_area(expanded);
    let rendered = render_text(&app, expanded.width, expanded.height);

    // Then: stale detached rows cannot blank the viewport and follow resumes at the tail.
    assert!(rendered.contains("RESIZE_TAIL_7"), "{rendered}");
    assert!(app.transcript_page_flip_scroll_top().is_none());
    assert!(app.transcript_following());
}

pub(super) fn detached_page_flip_survives_resize_with_remaining_overflow() {
    // Given: detached transcript navigation in a compact viewport with scrollable history.
    let mut app = detached_resize_app();

    // When: width changes while the painted transcript still exceeds the viewport.
    let resized = Rect::new(0, 0, 81, 20);
    app.set_frame_area(resized);
    let _ = render_text(&app, resized.width, resized.height);

    // Then: resize keeps manual detachment until the painted viewport reaches the tail.
    assert!(app.transcript_page_flip_scroll_top().is_some());
    assert!(!app.transcript_following());
}

pub(super) fn shift_right_left_on_details_focus_navigates_user_turns() {
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_a",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_a".into(),
            text: "First turn".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_a",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_a".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "First turn".to_string(),
            request_digest: "digest-a".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_b",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_b".into(),
            text: "Second turn".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_b",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_b".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Second turn".to_string(),
            request_digest: "digest-b".to_string(),
            metadata: None,
        }),
    ));

    assert!(
        app.activities.len() >= 2,
        "fixture must produce at least two user turns, got {}",
        app.activities.len()
    );

    app.focus = Focus::Details;
    app.transcript_view.selected_activity_index = 0;
    app.transcript_view.follow_mode = false;

    app.handle_key(key_with_modifiers(KeyCode::Right, KeyModifiers::SHIFT));
    assert_eq!(
        app.transcript_view.selected_activity_index, 1,
        "Shift+Right on transcript focus must advance to the next user turn"
    );
    assert_eq!(app.focus, Focus::Details);

    app.handle_key(key_with_modifiers(KeyCode::Left, KeyModifiers::SHIFT));
    assert_eq!(
        app.transcript_view.selected_activity_index, 0,
        "Shift+Left on transcript focus must return to the previous user turn"
    );
    assert_eq!(app.focus, Focus::Details);
}

pub(super) fn page_up_down_with_prompt_focus_scrolls_transcript_without_clearing_draft() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "draft text".to_string();
    app.composer.prompt_cursor = 10;
    app.transcript_view.last_transcript_max_scroll.set(42);
    app.transcript_view.last_transcript_viewport_height.set(12);

    app.handle_key(key(KeyCode::PageUp));
    assert_eq!(app.transcript_view.transcript_scroll, 10);
    assert!(!app.transcript_view.follow_mode);
    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(app.composer.prompt_buffer, "draft text");
    assert_eq!(app.composer.prompt_cursor, 10);

    app.handle_key(key(KeyCode::PageDown));
    assert_eq!(app.transcript_view.transcript_scroll, 0);
    assert!(app.transcript_view.follow_mode);

    app.handle_key(key(KeyCode::PageDown));
    assert!(app.transcript_view.follow_mode);
    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(app.composer.prompt_buffer, "draft text");
    assert_eq!(app.composer.prompt_cursor, 10);
}

pub(super) fn ctrl_up_down_with_prompt_focus_scrolls_transcript_by_one_row() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "draft text".to_string();
    app.composer.prompt_cursor = 10;
    app.transcript_view.last_transcript_max_scroll.set(42);

    app.handle_key(key_with_modifiers(KeyCode::Up, KeyModifiers::CONTROL));
    assert_eq!(app.transcript_view.transcript_scroll, 1);
    assert!(!app.transcript_view.follow_mode);
    assert_eq!(app.composer.prompt_buffer, "draft text");

    app.handle_key(key_with_modifiers(KeyCode::Up, KeyModifiers::CONTROL));
    assert_eq!(app.transcript_view.transcript_scroll, 2);

    app.handle_key(key_with_modifiers(KeyCode::Down, KeyModifiers::CONTROL));
    assert_eq!(app.transcript_view.transcript_scroll, 1);
    assert_eq!(app.focus, Focus::Prompt);
}

pub(super) fn active_stream_more_below_click_returns_to_live() {
    // Given: an active stream detached from a scrollable transcript tail.
    let mut app = detached_resize_app();
    let area = Rect::new(0, 0, 80, 20);
    let _ = render_text(&app, area.width, area.height);
    let targets = (0..area.height)
        .flat_map(|row| (0..area.width).map(move |column| (column, row)))
        .filter(|(column, row)| ui::transcript_return_to_live_hit(&app, area, *column, *row))
        .collect::<Vec<_>>();
    assert_eq!(
        targets.len(),
        1,
        "active affordance must expose one painted-cell target"
    );

    // When: the user clicks the active more-below affordance.
    let (column, row) = targets[0];
    let handled = app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
        Some(WheelTarget::Transcript),
        None,
        None,
    );

    // Then: the viewport returns to live follow mode immediately.
    assert!(handled);
    assert!(app.transcript_following());
    assert!(app.transcript_page_flip_scroll_top().is_none());
    assert!(
        app.transcript_view.transcript_click_activated_on_down,
        "return-to-live MouseDown must consume the matching MouseUp"
    );

    let released = app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
        Some(WheelTarget::Transcript),
        None,
        None,
    );
    assert!(released);
    assert!(!app.transcript_view.transcript_click_activated_on_down);
}

pub(super) fn completed_stream_more_below_affordance_remains_passive() {
    // Given: detached history after every visible turn has completed.
    let mut app = detached_resize_app();
    for activity in &mut app.activities {
        activity.status = ActivityStatus::Done;
    }
    let area = Rect::new(0, 0, 80, 20);
    let rendered = render_text(&app, area.width, area.height);

    // When: checking every cell of the visible passive affordance surface.
    let has_target = (0..area.height).any(|row| {
        (0..area.width).any(|column| ui::transcript_return_to_live_hit(&app, area, column, row))
    });

    // Then: the completion-state glyph remains visible but never becomes actionable.
    assert!(
        rendered.contains('▼'),
        "completed detached history keeps its passive indicator"
    );
    assert!(!has_target);
}

pub(super) fn detached_measured_viewport_has_no_stale_timeline_targets() {
    // Given: a measured transcript viewport detached without a page-flip override.
    let app = detached_resize_app();
    let area = Rect::new(0, 0, 80, 20);
    app.cancel_transcript_page_flip();
    assert!(!app.transcript_following());

    // When: hit-testing every cell through the integrated timeline path.
    let targets = (0..area.height)
        .flat_map(|row| (0..area.width).map(move |column| (column, row)))
        .filter_map(|(column, row)| ui::transcript_timeline_turn_at(&app, area, column, row))
        .collect::<Vec<_>>();

    // Then: stale live-tail marker geometry cannot remain interactive while detached.
    assert!(
        targets.is_empty(),
        "detached timeline targets were {targets:?}"
    );
}

pub(super) fn vanished_selection_anchor_stays_closed_through_mouse_up() {
    // Given: an active drag whose semantic endpoints were captured from transcript content.
    let mut app = transcript_selection_test_app_with_text("anchored selection text");
    let area = TEST_FRAME_AREA;
    let (column, row, width) = transcript_selection_text_bounds(&app, "anchored selection text");
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
        None,
        None,
        None,
    );
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: column + width.saturating_sub(1),
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
        None,
        None,
        None,
    );
    let _ = render_text(&app, area.width, area.height);
    assert!(app
        .transcript_view
        .transcript_selection_anchors
        .get()
        .is_some());
    app.activities[0].first_seq = 100;
    app.activities[0].transcript_text = "replacement content with unrelated cells".to_string();
    app.activities[0].revision = app.activities[0].revision.saturating_add(1);
    app.bump_transcript_render_epoch();

    // When: the pointer is released after the anchored surface disappears.
    let stale_hit = ui::transcript_selection_cell(&app, area, column, row);
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
        None,
        None,
        None,
    );

    // Then: stale cells cannot replace the unresolved semantic selection or reach copy.
    assert_eq!(stale_hit, None);
    assert!(app.transcript_view.transcript_selection.is_none());
    assert!(app
        .transcript_view
        .transcript_selection_anchors
        .get()
        .is_none());
}

pub(super) fn selection_mouse_up_does_not_activate_underlying_tool_target() {
    // Given: a transcript selection drag ending over an interactive tool row.
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(envelope(
        1,
        "req_selection_mouse_up",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_selection_mouse_up".into(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "selection release".to_string(),
            request_digest: "digest-selection-release".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_selection_mouse_up",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_selection_mouse_up".into(),
            tool_id: "shell.run".to_string(),
            args_summary: r#"{"cmd":"false"}"#.to_string(),
            args_digest: "digest-selection-release-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_selection_mouse_up",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_selection_mouse_up".into(),
            status: ToolCallStatus::Failed,
            output_summary: Some("exit code: 1\nstderr: nope".to_string()),
            output_digest: None,
            output_json: None,
            metadata: None,
        }),
    ));
    let area = TEST_FRAME_AREA;
    let (column, row) = (0..area.height)
        .flat_map(|row| (0..area.width).map(move |column| (column, row)))
        .find(|(column, row)| ui::transcript_mouse_target(&app, area, *column, *row).is_some())
        .expect("fixture must expose an interactive tool target");
    let cell = (0..area.height)
        .flat_map(|row| (0..area.width).map(move |column| (column, row)))
        .find_map(|(column, row)| ui::transcript_selection_cell(&app, area, column, row))
        .expect("fixture must expose selectable transcript content");
    app.set_transcript_selection(cell, cell);
    app.transcript_view.transcript_selection_dragging = true;
    assert!(!app
        .transcript_view
        .expanded_tool_outputs
        .contains("tc_selection_mouse_up"));

    // When: the selection gesture releases over that tool target.
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
        None,
        None,
        None,
    );

    // Then: MouseUp is consumed by selection finalization and never toggles the tool.
    assert!(!app
        .transcript_view
        .expanded_tool_outputs
        .contains("tc_selection_mouse_up"));
}

pub(super) fn shift_left_on_prompt_focus_still_selects_chars() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.composer.prompt_buffer = "hello".to_string();
    app.composer.prompt_cursor = 5;

    app.handle_key(key_with_modifiers(KeyCode::Left, KeyModifiers::SHIFT));

    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(app.composer.prompt_buffer, "hello");
    assert_eq!(app.composer.prompt_cursor, 4);
    assert_eq!(app.composer.selection_anchor, Some(5));
    assert_eq!(app.transcript_view.selected_activity_index, 0);
}

pub(super) fn mouse_wheel_scrolls_inspector_when_hovered() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::List;
    app.details_scroll = 2;
    app.transcript_view.transcript_scroll = 4;

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 90,
            row: 12,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Inspector),
        None,
        None,
    );
    assert_eq!(app.details_scroll, 5);
    assert_eq!(app.transcript_view.transcript_scroll, 4);
    assert_eq!(app.focus, Focus::List);

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 90,
            row: 12,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Inspector),
        None,
        None,
    );
    assert_eq!(app.details_scroll, 2);
    assert_eq!(app.transcript_view.transcript_scroll, 4);
    assert_eq!(app.focus, Focus::List);
}

pub(super) fn mouse_wheel_ignores_non_scrollable_areas() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.details_scroll = 6;
    app.transcript_view.transcript_scroll = 2;
    app.transcript_view.follow_mode = false;

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 2,
            row: 2,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    assert_eq!(app.details_scroll, 6);
    assert_eq!(app.transcript_view.transcript_scroll, 2);
    assert!(!app.transcript_view.follow_mode);
    assert_eq!(app.focus, Focus::Prompt);
}

pub(super) fn mouse_click_toggles_operator_sidebar_section_without_stealing_focus() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.details_scroll = 6;

    assert!(app.operator_sidebar_section_collapsed(OperatorSidebarSection::ModifiedFiles));

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 90,
            row: 12,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        Some(OperatorSidebarSection::ModifiedFiles),
        None,
    );

    assert!(!app.operator_sidebar_section_collapsed(OperatorSidebarSection::ModifiedFiles));
    assert_eq!(app.details_scroll, 0);
    assert_eq!(app.focus, Focus::Prompt);
}

pub(super) fn edit_applied_auto_opens_modified_files_section() {
    let mut app = AppState::new_live(None, false, None);
    assert!(app.operator_sidebar_section_collapsed(OperatorSidebarSection::ModifiedFiles));

    app.ingest_event(envelope(
        1,
        "req_edit_open",
        EventV1::EditApplied(EditAppliedEvent {
            edit_id: "edit-1".to_string(),
            path: "src/ui.rs".to_string(),
            new_file_digest: "digest-1".to_string(),
            diff_rel_path: None,
            diff_digest: None,
        }),
    ));

    assert!(!app.operator_sidebar_section_collapsed(OperatorSidebarSection::ModifiedFiles));
}

pub(super) fn diff_hunk_navigation_advances_and_retreats_between_hunks() {
    // arrange
    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let artifacts_dir = run_dir.path().join("artifacts");
    fs::create_dir_all(&artifacts_dir).unwrap_or_abort();
    fs::write(
        artifacts_dir.join("two-hunks.diff"),
        "--- docs/demo.md\n+++ docs/demo.md\n@@ -1,3 +1,3 @@\n alpha\n-old one\n+new one\n keep\n@@ -20,3 +20,3 @@\n before\n-old two\n+new two\n after\n",
    )
    .unwrap_or_abort();

    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    app.focus = Focus::Details;
    app.ingest_event(envelope(
        1,
        "req_diff_nav",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_diff_nav".into(),
            provider_id: "default".to_string(),
            model_id: "model-diff".to_string(),
            prompt_summary: "review diff hunks".to_string(),
            request_digest: "digest-diff-nav-request".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_diff_nav",
        EventV1::ToolCallRequested(ToolCallRequestedEvent {
            tool_call_id: "tc_diff_nav".into(),
            tool_id: "apply_patch".to_string(),
            args_summary: r#"{"patchText":"*** Begin Patch"}"#.to_string(),
            args_digest: "digest-diff-nav-args".to_string(),
            metadata: None,
        }),
    ));
    app.ingest_event(envelope(
        3,
        "req_diff_nav",
        EventV1::ToolCallFinished(ToolCallFinishedEvent {
            tool_call_id: "tc_diff_nav".into(),
            status: ToolCallStatus::Succeeded,
            output_summary: Some("Success. Updated the following files".to_string()),
            output_digest: Some("digest-diff-nav-output".to_string()),
            output_json: Some(serde_json::json!({
                "files": ["M docs/demo.md"],
                "edits": [
                    {
                        "edit_id": "apply-patch-demo",
                        "path": "docs/demo.md",
                        "summary": "apply patch update docs/demo.md",
                        "deleted": false,
                        "diff_rel_path": "artifacts/two-hunks.diff",
                        "diff_digest": "digest-two-hunks"
                    }
                ]
            })),
            metadata: None,
        }),
    ));
    app.set_patch_file_output_expanded_for_test("tc_diff_nav", "docs/demo.md", true);

    let frame_area = Rect::new(0, 0, 100, 14);
    app.set_frame_area(frame_area);
    let _rendered = render_debug(&app, frame_area.width, frame_area.height);
    let hunk_rows = crate::ui::transcript_diff_hunk_rows(&app, frame_area);
    assert_eq!(hunk_rows.len(), 2, "expected two navigable diff hunks");
    app.transcript_view.follow_mode = false;
    app.transcript_view.transcript_scroll = app.transcript_view.last_transcript_max_scroll.get();
    app.set_transcript_page_flip_state(PageFlipState::Idle.begin(0).preserve_at(0));

    // act
    app.handle_key(key_with_modifiers(KeyCode::Char('n'), KeyModifiers::ALT));
    let first_hunk = app.selected_diff_hunk_row_for_test().unwrap_or_abort();
    assert!(!app.transcript_view.follow_mode);
    assert!(
        !app.transcript_page_flip_preserving(),
        "diff navigation must cancel the submit-time page flip"
    );
    assert_eq!(
        app.transcript_view
            .last_transcript_max_scroll
            .get()
            .saturating_sub(app.transcript_scroll_offset()),
        first_hunk.min(app.transcript_view.last_transcript_max_scroll.get()),
        "diff navigation must move the visible scroll owner to the selected hunk"
    );

    app.handle_key(key_with_modifiers(KeyCode::Char('n'), KeyModifiers::ALT));
    let second_hunk = app.selected_diff_hunk_row_for_test().unwrap_or_abort();
    assert!(
        second_hunk > first_hunk,
        "next hunk should advance: first={first_hunk}, second={second_hunk}"
    );

    app.handle_key(key_with_modifiers(KeyCode::Char('p'), KeyModifiers::ALT));
    // assert
    assert_eq!(app.selected_diff_hunk_row_for_test(), Some(first_hunk));
}

pub(super) fn dragging_transcript_scrollbar_updates_scroll_position() {
    let mut app = AppState::new_live(None, false, None);
    app.transcript_view.last_transcript_max_scroll.set(100);
    app.transcript_view.follow_mode = false;
    app.transcript_view.transcript_scroll = 50;

    let scrollbar = TranscriptScrollbarHit {
        lane: Rect::new(72, 1, 2, 20),
        track: Rect::new(72, 2, 2, 18),
        thumb: Rect::new(72, 6, 2, 4),
        max_scroll: 100,
    };

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 72,
            row: 7,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Transcript),
        None,
        Some(scrollbar),
    );
    assert!(app.transcript_scrollbar_dragging());

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: 72,
            row: 14,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Transcript),
        None,
        None,
    );

    assert!(!app.transcript_view.follow_mode);
    assert_eq!(app.transcript_view.transcript_scroll, 21);

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 72,
            row: 17,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Transcript),
        None,
        None,
    );
    assert!(!app.transcript_scrollbar_dragging());
}

pub(super) fn clicking_transcript_scrollbar_track_without_thumb_does_not_start_drag() {
    let mut app = AppState::new_live(None, false, None);
    app.transcript_view.last_transcript_max_scroll.set(80);

    let scrollbar = TranscriptScrollbarHit {
        lane: Rect::new(72, 1, 2, 20),
        track: Rect::new(72, 2, 2, 18),
        thumb: Rect::new(72, 6, 2, 4),
        max_scroll: 80,
    };

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 72,
            row: 3,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        Some(WheelTarget::Transcript),
        None,
        Some(scrollbar),
    );

    assert!(!app.transcript_scrollbar_dragging());
    assert!(app.transcript_view.follow_mode);
    assert_eq!(app.transcript_view.transcript_scroll, 0);
}

pub(super) fn identical_local_prompt_echoes_adopt_request_ids_in_submission_order() {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("build", "mock:model-test").with_mode_label("Test"),
    );
    for _ in 0..2 {
        for character in "same queued prompt".chars() {
            app.handle_key(key(KeyCode::Char(character)));
        }
        app.handle_key(key(KeyCode::Enter));
    }
    assert_eq!(
        app.activities
            .iter()
            .filter(|activity| activity.request_id.is_empty())
            .count(),
        2
    );

    app.ingest_event(envelope(
        1,
        "req_first",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_first".into(),
            text: "same queued prompt".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_second",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_second".into(),
            text: "same queued prompt".to_string(),
        }),
    ));

    assert_eq!(app.activities[0].request_id, "req_first");
    assert_eq!(app.activities[1].request_id, "req_second");
    assert_eq!(app.activities[0].first_seq, 1);
    assert_eq!(app.activities[1].first_seq, 2);
}
