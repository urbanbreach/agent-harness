use super::*;
use crate::UnwrapOrAbort;

#[cfg(not(windows))]
pub(super) fn mouse_drag_copy_on_select_copies_transcript_text_and_clears_selection() {
    let copied = Arc::new(Mutex::new(None::<String>));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *sink.lock().unwrap_or_abort() = Some(text.to_string());
        Ok(())
    })));

    let mut app = transcript_selection_test_app();
    drag_transcript_selection(&mut app, "Copy this exact reply");

    assert_eq!(
        copied.lock().unwrap_or_abort().clone(),
        Some("Copy this exact reply".to_string())
    );
    assert!(app.transcript_selection().is_none());
    assert_eq!(
        app.toast()
            .map(|toast| (toast.message.as_str(), toast.variant)),
        Some(("Copied to clipboard", ToastVariant::Info))
    );

    crate::clipboard::set_copy_override(None);
}

#[cfg(not(windows))]
pub(super) fn mouse_drag_copy_on_select_copies_shell_card_text() {
    let copied = Arc::new(Mutex::new(None::<String>));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *sink.lock().unwrap_or_abort() = Some(text.to_string());
        Ok(())
    })));

    let mut app = shell_card_selection_test_app();
    let (column, row, width) = transcript_selection_text_bounds(&app, "copy target output");
    drag_transcript_selection_range(
        &mut app,
        (column, row),
        (column + width.saturating_sub(1), row),
    );

    assert_eq!(
        copied.lock().unwrap_or_abort().clone(),
        Some("copy target output".to_string())
    );
    assert!(app.transcript_selection().is_none());
    assert_eq!(
        app.toast()
            .map(|toast| (toast.message.as_str(), toast.variant)),
        Some(("Copied to clipboard", ToastVariant::Info))
    );

    crate::clipboard::set_copy_override(None);
}

#[cfg(not(windows))]
pub(super) fn mouse_drag_copy_on_select_copies_operator_sidebar_text() {
    let copied = Arc::new(Mutex::new(None::<String>));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *sink.lock().unwrap_or_abort() = Some(text.to_string());
        Ok(())
    })));

    let mut app = operator_sidebar_selection_test_app();
    drag_operator_sidebar_selection(&mut app, "Copy sidebar task");

    assert_eq!(
        copied.lock().unwrap_or_abort().clone(),
        Some("Copy sidebar task".to_string())
    );
    assert!(app.operator_sidebar_selection().is_none());
    assert_eq!(
        app.toast()
            .map(|toast| (toast.message.as_str(), toast.variant)),
        Some(("Copied to clipboard", ToastVariant::Info))
    );

    crate::clipboard::set_copy_override(None);
}

pub(super) fn disabled_copy_on_select_keeps_operator_sidebar_selection_until_right_click_copy() {
    let _guard = ClipboardModeGuard::disabled_copy_on_select();
    let copied = Arc::new(Mutex::new(None::<String>));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *sink.lock().unwrap_or_abort() = Some(text.to_string());
        Ok(())
    })));

    let mut app = operator_sidebar_selection_test_app();
    let (column, row, _) = drag_operator_sidebar_selection(&mut app, "Copy sidebar task");

    assert!(app.operator_sidebar_selection().is_some());
    assert!(copied.lock().unwrap_or_abort().is_none());

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
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
        copied.lock().unwrap_or_abort().clone(),
        Some("Copy sidebar task".to_string())
    );
    assert!(app.operator_sidebar_selection().is_none());
    assert_eq!(
        app.toast()
            .map(|toast| (toast.message.as_str(), toast.variant)),
        Some(("Copied to clipboard", ToastVariant::Info))
    );
}

pub(super) fn mouse_drag_copy_on_select_surfaces_error_toast_when_copy_fails() {
    crate::clipboard::set_copy_override(Some(Box::new(|_| {
        Err(std::io::Error::other("simulated clipboard failure"))
    })));

    let mut app = transcript_selection_test_app();
    drag_transcript_selection(&mut app, "Copy this exact reply");

    assert!(app.transcript_selection().is_none());
    assert_eq!(
        app.toast()
            .map(|toast| (toast.message.as_str(), toast.variant)),
        Some((
            "clipboard copy failed: simulated clipboard failure",
            ToastVariant::Error,
        ))
    );

    crate::clipboard::set_copy_override(None);
}

pub(super) fn mouse_drag_copy_on_select_preserves_multiline_text_without_render_padding() {
    let copied = Arc::new(Mutex::new(None::<String>));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *sink.lock().unwrap_or_abort() = Some(text.to_string());
        Ok(())
    })));

    let expected = [
        "Done.",
        "",
        "Changed:",
        "• docs/config.md",
        "",
        "What I changed:",
        "• Tightened the opening description to mention reliable software and compile-time guarantees.",
    ]
    .join("\n");
    let source = expected.replace("• ", "- ");
    let mut app = transcript_selection_test_app_with_text(&source);
    let start = transcript_selection_text_position(&app, "Done.");
    let (end_column, end_row, end_width) = transcript_selection_text_bounds(&app, "guarantees.");
    drag_transcript_selection_range(
        &mut app,
        start,
        (end_column + end_width.saturating_sub(1), end_row),
    );

    assert_eq!(copied.lock().unwrap_or_abort().clone(), Some(expected));
    assert!(app.transcript_selection().is_none());

    crate::clipboard::set_copy_override(None);
}

pub(super) fn disabled_copy_on_select_keeps_selection_until_right_click_copy() {
    let _guard = ClipboardModeGuard::disabled_copy_on_select();
    let copied = Arc::new(Mutex::new(None::<String>));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *sink.lock().unwrap_or_abort() = Some(text.to_string());
        Ok(())
    })));

    let body = format!(
        "Copy {}",
        "界 e\u{301} 👩‍💻 #\u{fe0f} wrapped words ".repeat(8)
    );
    let source = format!("{}\n\n[finish](https://example.com/copy)", body.trim_end());
    let expected = format!(
        "{}\n\nfinish\n\nLinks:\nhttps://example.com/copy",
        body.trim_end()
    );
    let mut app = transcript_selection_test_app_with_text(&source);
    let (column, row) = transcript_click_position(&app, "Copy");
    let (end_column, end_row) = transcript_click_position(&app, "finish");
    drag_transcript_selection_range(&mut app, (column - 1, row), (end_column + 4, end_row));

    assert!(app.transcript_selection().is_some());
    assert!(copied.lock().unwrap_or_abort().is_none());

    // The same source selection survives reflow, including wide and joined graphemes.
    let mut area = TEST_FRAME_AREA;
    for width in [40, 140, 40] {
        area.width = width;
        app.set_frame_area(area);
        let (column, row) = transcript_click_position_in_area(&app, area, "Copy");
        assert_eq!(
            rendered_cell_bg_in_area(&app, area, column - 1, row),
            app.theme().status.info
        );
    }
    let (column, row) = transcript_click_position_in_area(&app, area, "Copy");

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Right),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        area,
        None,
        None,
        None,
    );

    assert_eq!(copied.lock().unwrap_or_abort().clone(), Some(expected));
    assert!(app.transcript_selection().is_none());
    assert_eq!(
        app.toast()
            .map(|toast| (toast.message.as_str(), toast.variant)),
        Some(("Copied to clipboard", ToastVariant::Info))
    );
}

pub(super) fn disabled_copy_on_select_supports_ctrl_c_and_escape() {
    let _guard = ClipboardModeGuard::disabled_copy_on_select();
    let copied = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        sink.lock().unwrap_or_abort().push(text.to_string());
        Ok(())
    })));

    let text = (0..70)
        .map(|index| {
            if index == 10 {
                "Copy this exact reply".to_owned()
            } else {
                format!("Other reply {index:02}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut copy_app = transcript_selection_test_app_with_text(&text);
    copy_app.set_frame_area(TEST_FRAME_AREA);
    copy_app.scroll_goto_top();
    copy_app.scroll_page_down(10);
    copy_app.set_frame_area(TEST_FRAME_AREA);
    let painted = render_text(&copy_app, TEST_FRAME_AREA.width, TEST_FRAME_AREA.height);
    assert!(painted.contains("Select this"), "sticky prompt is visible");
    let (column, row) =
        transcript_click_position_in_area(&copy_app, TEST_FRAME_AREA, "Copy this exact reply");
    let column = column - 1; // The click helper targets one cell inside the label.
    drag_transcript_selection_range(&mut copy_app, (column, row), (column + 20, row));
    assert!(copy_app.transcript_selection().is_some());
    assert_eq!(
        rendered_cell_bg(&copy_app, column, row),
        copy_app.theme().status.info
    );
    assert_ne!(
        rendered_cell_bg(&copy_app, column, row + 1),
        copy_app.theme().status.info
    );

    // Copy still owns the selected source after the viewport leaves it.
    copy_app.scroll_goto_bottom();
    copy_app.set_frame_area(TEST_FRAME_AREA);
    assert!(
        !render_text(&copy_app, TEST_FRAME_AREA.width, TEST_FRAME_AREA.height)
            .contains("Copy this exact reply")
    );
    copy_app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));

    assert_eq!(
        copied.lock().unwrap_or_abort().as_slice(),
        ["Copy this exact reply"]
    );
    assert!(copy_app.transcript_selection().is_none());

    let mut escape_app = transcript_selection_test_app();
    drag_transcript_selection(&mut escape_app, "Copy this exact reply");
    assert!(escape_app.transcript_selection().is_some());

    escape_app.handle_key(key(KeyCode::Esc));

    assert!(escape_app.transcript_selection().is_none());
    assert_eq!(
        copied.lock().unwrap_or_abort().as_slice(),
        ["Copy this exact reply"]
    );
}

#[cfg(not(windows))]
pub(super) fn mouse_drag_copy_on_select_keeps_body_rows_aligned_after_reasoning_gap() {
    let copied = Arc::new(Mutex::new(None::<String>));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *sink.lock().unwrap_or_abort() = Some(text.to_string());
        Ok(())
    })));

    let mut app = transcript_selection_test_app_with_reasoning(
        "Trace the exact rows first",
        "Copy this exact reply",
    );
    drag_transcript_selection(&mut app, "Copy this exact reply");

    assert_eq!(
        copied.lock().unwrap_or_abort().clone(),
        Some("Copy this exact reply".to_string())
    );
    assert!(app.transcript_selection().is_none());

    crate::clipboard::set_copy_override(None);
}

pub(super) fn expanded_edit_ctrl_c_copies_canonical_unified_patches() {
    let _guard = ClipboardModeGuard::disabled_copy_on_select();
    let copied = Arc::new(Mutex::new(None::<String>));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *sink.lock().unwrap_or_abort() = Some(text.to_string());
        Ok(())
    })));

    let run_dir = tempfile::tempdir().unwrap_or_abort();
    let mut app = transcript_selection_test_app();
    app.session_path = Some(run_dir.path().to_path_buf());
    app.activities[0].transcript_text.clear();
    let make_edit = |id: &str, path: &str, old: &str, new: &str, seq: u64| ToolCallEntry {
        hook_executions: Vec::new(),
        tool_call_id: id.to_string(),
        tool_id: "edit".to_string(),
        canonical_tool_id: Some("edit".to_string()),
        alias_source_tool_id: None,
        resolved_tool_identity: None,
        args_summary: serde_json::json!({
            "filePath": path,
            "oldString": old,
            "newString": new
        })
        .to_string(),
        args_digest: format!("digest-{id}"),
        lifecycle_state: None,
        status: ToolCallDisplayStatus::Succeeded,
        output_summary: Some("edited".to_string()),
        output_digest: Some(format!("output-{id}")),
        output_json: None,
        truncated_output: None,
        edit: None,
        lineage: None,
        artifact_refs: Vec::new(),
        timing_elapsed_ms: None,
        permissions: Vec::new(),
        first_seq: seq,
        last_seq: seq,
        first_mono_ms: seq,
        last_mono_ms: seq,
        first_timestamp: None,
        last_timestamp: None,
    };
    app.activities[0].tool_calls = vec![
        make_edit("create", "created.txt", "", "created line\n", 10),
        make_edit("modify", "modified.txt", "old line\n", "new line\n", 20),
        make_edit("delete", "deleted.txt", "deleted line\n", "", 30),
    ];
    for id in ["create", "modify", "delete"] {
        assert!(app.is_tool_output_expanded_for_test(id));
    }

    let (start_column, start_row, _) = transcript_selection_text_bounds(&app, "created line");
    let (end_column, end_row, end_width) = transcript_selection_text_bounds(&app, "deleted line");
    drag_transcript_selection_range(
        &mut app,
        (start_column, start_row),
        (end_column.saturating_add(end_width), end_row),
    );
    app.set_frame_area(TEST_FRAME_AREA);
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));

    let patch = copied.lock().unwrap_or_abort().clone().unwrap_or_abort();
    assert!(patch.contains("--- /dev/null\n+++ created.txt\n@@ -0,0 +1,1 @@"));
    assert!(patch.contains("--- modified.txt\n+++ modified.txt\n@@ -1,1 +1,1 @@"));
    assert!(patch.contains("--- deleted.txt\n+++ /dev/null\n@@ -1,1 +0,0 @@"));
    assert!(patch.contains("+created line"));
    assert!(patch.contains("-old line\n+new line"));
    assert!(patch.contains("-deleted line"));
    assert!(!patch.contains("\n\nLinks:\n"));
    assert_eq!(
        crate::ui::structured_diff_stats(&patch, None, false),
        Some((2, 2))
    );
    crate::clipboard::set_copy_override(None);
}

#[test]
fn ordinary_transcript_copy_exports_selected_destinations_but_patch_copy_does_not() {
    // Given: an assistant response containing a safe markdown link.
    let _guard = ClipboardModeGuard::disabled_copy_on_select();
    let copied = Arc::new(Mutex::new(None::<String>));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *sink.lock().unwrap_or_abort() = Some(text.to_string());
        Ok(())
    })));
    let mut app = transcript_selection_test_app_with_text(
        "Read [docs](https://example.com/docs) before applying the patch.",
    );
    drag_transcript_selection(&mut app, "docs");
    app.set_frame_area(TEST_FRAME_AREA);

    // When: ordinary transcript copy is invoked through the production key path.
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));

    // Then: selected destination export is explicit on ordinary copy.
    assert_eq!(
        copied.lock().unwrap_or_abort().clone(),
        Some("docs\n\nLinks:\nhttps://example.com/docs".to_string())
    );
    crate::clipboard::set_copy_override(None);
}

#[test]
fn production_render_app_emits_only_balanced_safe_osc8_through_frame_backend() {
    // Given: production transcript rendering with one safe and one unsafe destination.
    let mut app = transcript_selection_test_app_with_text(
        "[safe](https://example.com/safe) [bad](javascript:alert(1))",
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap_or_abort();

    app.set_frame_area(Rect::new(0, 0, 120, 40));
    // When: pure paint supplies cells and runtime projection supplies link metadata.
    terminal
        .draw(|frame| render_app(frame, &app))
        .unwrap_or_abort();
    let cells = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            (
                u16::try_from(index % 120).unwrap_or_abort(),
                u16::try_from(index / 120).unwrap_or_abort(),
                cell.clone(),
            )
        })
        .collect::<Vec<_>>();
    let (mut output, writer, receiver) = crate::terminal::FrameOutput::bounded(1);
    let mut backend = crate::terminal::FrameOutputBackend::new(writer);
    output.begin_frame().unwrap_or_abort();
    backend.set_hyperlinks(std::mem::take(&mut app.transcript_view.hyperlinks));
    ratatui::backend::Backend::draw(
        &mut backend,
        cells.iter().map(|(x, y, cell)| (*x, *y, cell)),
    )
    .unwrap_or_abort();
    output.finish_frame().unwrap_or_abort();
    let frame = receiver.try_recv().unwrap_or_abort();
    let bytes = String::from_utf8_lossy(&frame.bytes);

    // Then: only the safe target is emitted and every OSC-8 open has a close.
    assert!(bytes.contains("https://example.com/safe"), "{bytes:?}");
    assert!(!bytes.contains("javascript:"), "{bytes:?}");
    let opens = bytes.matches("]8;;https://").count();
    let markers = bytes.matches("]8;;").count();
    assert!(opens > 0);
    assert_eq!(markers, opens.saturating_mul(2));
}

pub(super) fn transcript_selection_snapshot_preserves_user_card_marker() {
    let app = transcript_selection_test_app();
    let snapshot = transcript_selection_debug_snapshot(&app, TEST_FRAME_AREA).unwrap_or_abort();
    let user_row = snapshot
        .rows
        .iter()
        .find(|row| row.contains("Select this"))
        .unwrap_or_abort();

    assert!(
        user_row.trim_start().starts_with("❯ Select this"),
        "user selection row should preserve the stable card marker and padding\n{:#?}",
        snapshot.rows
    );
    assert!(
        !user_row.contains("█Select this"),
        "user selection row must not use the downgraded prompt rail block\n{user_row}"
    );
}

pub(super) fn transcript_selection_render_stays_aligned_after_large_reasoning_block() {
    let thinking = (0..30)
        .map(|idx| format!("Reasoning line {idx}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut app = transcript_selection_test_app_with_reasoning(&thinking, "Target answer line");
    let (column, row, width) = transcript_selection_text_bounds(&app, "Target answer line");

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
    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: column + width.saturating_sub(1),
            row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    let backend = TestBackend::new(TEST_FRAME_AREA.width, TEST_FRAME_AREA.height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, &app))
        .unwrap_or_abort();

    let buffer = terminal.backend().buffer();
    let highlight = crate::theme::Theme::default().status.info;
    assert_eq!(buffer[(column, row)].bg, highlight);

    let far_above_row = row.saturating_sub(20);
    if far_above_row != row {
        assert_ne!(buffer[(column, far_above_row)].bg, highlight);
    }
}
