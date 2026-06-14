use super::*;

pub(super) fn focus_returns_after_palette_close() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Details;

    app.handle_key(key_with_modifiers(
        KeyCode::Char('p'),
        KeyModifiers::CONTROL,
    ));
    assert!(app.overlay_state.palette_visible);
    assert_eq!(app.focus, Focus::Details);

    app.handle_key(key(KeyCode::Esc));
    assert!(!app.overlay_state.palette_visible);
    assert_eq!(app.focus, Focus::Details);
}

pub(super) fn details_drawer_toggles_without_stealing_transcript_state() {
    let mut app = AppState::new_live(None, false, None);

    app.ingest_event(envelope(
        1,
        "req_a",
        EventV1::UserMessageSubmitted(UserMessageSubmittedEvent {
            request_id: "req_a".to_string(),
            text: "First".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        2,
        "req_a",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_a".to_string(),
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
            request_id: "req_b".to_string(),
            text: "Second".to_string(),
        }),
    ));
    app.ingest_event(envelope(
        4,
        "req_b",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_b".to_string(),
            provider_id: "openai".to_string(),
            model_id: "gpt-5-codex".to_string(),
            prompt_summary: "Second".to_string(),
            request_digest: "digest-b".to_string(),
            metadata: None,
        }),
    ));

    app.transcript_view.follow_mode = false;
    app.focus = Focus::Details;
    app.selected_activity_index = 0;
    app.details_scroll = 7;

    app.handle_key(key(KeyCode::Char('i')));
    assert!(app.details_drawer_open());
    assert_eq!(app.active_tab, Tab::Run);
    assert_eq!(app.focus, Focus::Details);
    assert!(!app.transcript_view.follow_mode);
    assert_eq!(app.selected_activity_index, 0);
    assert_eq!(app.details_scroll, 7);

    app.handle_key(key(KeyCode::Char('i')));
    assert!(!app.details_drawer_open());
    assert_eq!(app.active_tab, Tab::Run);
    assert_eq!(app.focus, Focus::Details);
    assert!(!app.transcript_view.follow_mode);
    assert_eq!(app.selected_activity_index, 0);
    assert_eq!(app.details_scroll, 7);
}

pub(super) fn mouse_wheel_scrolls_transcript_without_stealing_focus() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;

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
    assert!(app.transcript_view.follow_mode);
    assert_eq!(app.focus, Focus::Prompt);
}

pub(super) fn transcript_navigation_keys_match_scroll_expectations() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Details;
    app.transcript_view.last_transcript_max_scroll.set(42);

    app.handle_key(key(KeyCode::PageUp));
    assert_eq!(app.transcript_view.transcript_scroll, 10);
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
    let run_dir = tempfile::tempdir().expect("create run dir");
    let artifacts_dir = run_dir.path().join("artifacts");
    fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
    fs::write(
        artifacts_dir.join("two-hunks.diff"),
        "--- docs/demo.md\n+++ docs/demo.md\n@@ -1,3 +1,3 @@\n alpha\n-old one\n+new one\n keep\n@@ -20,3 +20,3 @@\n before\n-old two\n+new two\n after\n",
    )
    .expect("write diff fixture");

    let mut app = AppState::new_live(Some(run_dir.path().to_path_buf()), false, None);
    app.focus = Focus::Details;
    app.ingest_event(envelope(
        1,
        "req_diff_nav",
        EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
            request_id: "req_diff_nav".to_string(),
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
            tool_call_id: "tc_diff_nav".to_string(),
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
            tool_call_id: "tc_diff_nav".to_string(),
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

    // act
    app.handle_key(key_with_modifiers(KeyCode::Char('n'), KeyModifiers::ALT));
    let first_hunk = app
        .selected_diff_hunk_row_for_test()
        .expect("first hunk selected");
    assert!(!app.transcript_view.follow_mode);

    app.handle_key(key_with_modifiers(KeyCode::Char('n'), KeyModifiers::ALT));
    let second_hunk = app
        .selected_diff_hunk_row_for_test()
        .expect("second hunk selected");
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
