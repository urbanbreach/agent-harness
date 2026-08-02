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

pub(super) fn disabled_copy_on_select_leaves_right_click_to_the_terminal() {
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

    assert!(copied.lock().unwrap_or_abort().is_none());
    assert!(app.operator_sidebar_selection().is_some());
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
    let mut app = transcript_selection_test_app_with_text(&expected);
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

pub(super) fn disabled_copy_on_select_preserves_selection_for_terminal_right_click() {
    let _guard = ClipboardModeGuard::disabled_copy_on_select();
    let copied = Arc::new(Mutex::new(None::<String>));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        *sink.lock().unwrap_or_abort() = Some(text.to_string());
        Ok(())
    })));

    let mut app = transcript_selection_test_app();
    let (column, row, _) = drag_transcript_selection(&mut app, "Copy this exact reply");

    assert!(app.transcript_selection().is_some());
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

    assert!(copied.lock().unwrap_or_abort().is_none());
    assert!(app.transcript_selection().is_some());
}

pub(super) fn disabled_copy_on_select_supports_ctrl_c_and_escape() {
    let _guard = ClipboardModeGuard::disabled_copy_on_select();
    let copied = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        sink.lock().unwrap_or_abort().push(text.to_string());
        Ok(())
    })));

    let mut copy_app = transcript_selection_test_app();
    drag_transcript_selection(&mut copy_app, "Copy this exact reply");
    assert!(copy_app.transcript_selection().is_some());

    copy_app.set_frame_area(TEST_FRAME_AREA);
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

pub(super) fn transcript_selection_hit_testing_reuses_cached_snapshot_during_drag() {
    let app = transcript_selection_test_app();
    let (column, row, width) = transcript_selection_text_bounds(&app, "Copy this exact reply");

    reset_transcript_selection_cache_metrics_for_test();

    for offset in 0..width {
        assert!(transcript_selection_cell(&app, TEST_FRAME_AREA, column + offset, row,).is_some());
    }

    assert_eq!(transcript_selection_cache_build_count_for_test(), 1);
}

pub(super) fn transcript_selection_snapshot_uses_transcript_rail_for_user_rows() {
    let app = transcript_selection_test_app();
    let snapshot = transcript_selection_debug_snapshot(&app, TEST_FRAME_AREA).unwrap_or_abort();
    let user_row = snapshot
        .rows
        .iter()
        .find(|row| row.contains("Select this"))
        .unwrap_or_abort();

    assert!(
        user_row.trim_start().starts_with("❯ Select this"),
        "user selection row should preserve the transcript marker and padding\n{:#?}",
        snapshot.rows
    );
    assert!(
        !user_row.contains("█Select this"),
        "user selection row must not use the downgraded prompt rail block\n{user_row}"
    );
}

pub(super) fn mouse_wheel_does_not_build_transcript_selection_snapshot() {
    let mut app = transcript_selection_test_app();

    reset_transcript_selection_cache_metrics_for_test();

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

    assert_eq!(transcript_selection_cache_build_count_for_test(), 0);
}

pub(super) fn transcript_selection_render_reuses_cached_snapshot() {
    let mut app = transcript_selection_test_app();
    let (column, row, width) = transcript_selection_text_bounds(&app, "Copy this exact reply");
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

    reset_transcript_selection_cache_metrics_for_test();

    let backend = TestBackend::new(TEST_FRAME_AREA.width, TEST_FRAME_AREA.height);
    let mut terminal = Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, &app))
        .unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, &app))
        .unwrap_or_abort();

    assert_eq!(transcript_selection_cache_build_count_for_test(), 1);
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

pub(super) fn transcript_render_key_is_cached_across_selection_drag_path() {
    let mut app = transcript_selection_test_app();

    AppState::reset_transcript_render_key_metrics_for_test();

    let (column, row, width) = transcript_selection_text_bounds(&app, "Copy this exact reply");

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
    terminal
        .draw(|frame| render_app(frame, &app))
        .unwrap_or_abort();

    assert_eq!(AppState::transcript_render_key_build_count_for_test(), 1);
}

pub(super) fn transcript_render_key_reuses_cache_until_marked_dirty() {
    let mut app = transcript_selection_test_app();

    AppState::reset_transcript_render_key_metrics_for_test();

    let initial_key = app.transcript_render_cache_key();
    let cached_key = app.transcript_render_cache_key();
    assert_eq!(initial_key, cached_key);
    assert_eq!(AppState::transcript_render_key_build_count_for_test(), 1);

    app.mark_transcript_dirty_for_test();

    let dirty_key = app.transcript_render_cache_key();
    assert_ne!(initial_key, dirty_key);
    assert_eq!(AppState::transcript_render_key_build_count_for_test(), 2);
}
