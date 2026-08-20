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
