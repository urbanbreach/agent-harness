pub(super) fn trust_folder_prompt_preempts_lower_pointer_targets() {
    // Given: the startup action is covered by the trust-folder prompt.
    let mut app = AppState::new_startup(Vec::new(), None);
    let (column, row) = transcript_click_position(&app, "New worktree");
    app.trust_folder_prompt_visible = true;

    // When: the pointer moves over the lower action.
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

    // Then: the blocking prompt consumes routing without exposing lower hover state.
    assert!(handled);
    assert_eq!(app.welcome_state().hovered_action(), None);
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
    assert!(!app.transcript_view.follow_mode);

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
    assert!(!app.transcript_view.follow_mode);

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
        3,
        "active affordance must expose Grok's three-cell pointer target"
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

pub(super) fn completed_stream_more_below_affordance_is_actionable() {
    // Given: detached history after every visible turn has completed.
    let mut app = detached_resize_app();
    for activity in &mut app.activities {
        activity.status = ActivityStatus::Done;
    }
    let area = Rect::new(0, 0, 80, 20);
    let rendered = render_text(&app, area.width, area.height);

    // When: checking every cell of the visible affordance surface.
    let targets = (0..area.height)
        .flat_map(|row| (0..area.width).map(move |column| (column, row)))
        .filter(|(column, row)| ui::transcript_return_to_live_hit(&app, area, *column, *row))
        .collect::<Vec<_>>();

    // Then: the completion-state glyph remains visible with Grok's three-cell pointer target.
    assert!(
        rendered.contains('▼'),
        "completed detached history keeps its return indicator"
    );
    assert_eq!(targets.len(), 3);
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
