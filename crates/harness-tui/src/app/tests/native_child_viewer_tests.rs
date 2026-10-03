use super::*;

pub(super) fn assert_child_viewer(app: &mut AppState) {
    let area = Rect::new(0, 0, 80, 24);
    app.set_frame_area(area);
    app.composer.vim_mode = true;
    app.handle_key(key(KeyCode::Enter));
    let viewer = app.transcript_viewer().unwrap_or_abort();
    let content = viewer.content().content().to_owned();
    assert!(content.contains("durable history"));
    assert!(viewer.render_surface(area).following);
    app.handle_key(key(KeyCode::Down));
    assert!(
        app.transcript_viewer()
            .unwrap_or_abort()
            .render_surface(area)
            .following
    );
    app.handle_pointer_event(
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 40,
            row: 12,
            modifiers: KeyModifiers::NONE,
        },
        area,
    );
    assert!(
        !app.transcript_viewer()
            .unwrap_or_abort()
            .render_surface(area)
            .following
    );
    app.handle_key(key(KeyCode::Char('F')));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('k'),
        KeyModifiers::CONTROL,
    ));
    assert!(
        !app.transcript_viewer()
            .unwrap_or_abort()
            .render_surface(area)
            .following
    );
    app.handle_key(key(KeyCode::Char('F')));
    assert_raw_roundtrip(app);
    assert_pointer_copy(app, area);
    app.handle_key(key(KeyCode::Char('/')));
    app.handle_paste("histoy");
    app.handle_key(key(KeyCode::Left));
    app.handle_paste("r\n");
    let viewer = app.transcript_viewer().unwrap_or_abort();
    assert_eq!(viewer.search().query(), "history");
    assert_eq!(viewer.input.editor.cursor().insertion_index(), 6);
    assert_eq!(viewer.search().matches().len(), 1);
    app.handle_key(key_with_modifiers(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    ));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('k'),
        KeyModifiers::CONTROL,
    ));
    app.handle_paste("child lifecycles, permissions, navigation, and durable history");
    assert_eq!(
        app.transcript_viewer()
            .unwrap_or_abort()
            .search()
            .matches()
            .len(),
        1
    );
    app.handle_key(key_with_modifiers(
        KeyCode::Char('a'),
        KeyModifiers::CONTROL,
    ));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('k'),
        KeyModifiers::CONTROL,
    ));
    app.handle_paste("history");
    app.handle_key(key(KeyCode::Enter));
    app.handle_paste("ignored");
    assert_eq!(
        app.transcript_viewer().unwrap_or_abort().search().query(),
        "history"
    );
    app.handle_key(key(KeyCode::Char('/')));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('f'),
        KeyModifiers::CONTROL,
    ));
    assert!(app.transcript_viewer().is_none());
    assert_eq!(app.current_session_id(), Some("child"));
    app.handle_key(key(KeyCode::Enter));
    assert_filter_selection(app);
    let layout = crate::transcript_block_viewer::viewer_layout(
        crate::layout::FrameLayoutPlan::for_app(app, area).shell,
    );
    assert_eq!(layout.popup, Rect::new(5, 5, 70, 14));
    assert!(app.handle_pointer_event(
        MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: layout.close.x,
            row: layout.close.y,
            modifiers: KeyModifiers::NONE,
        },
        area
    ));
    assert!(app.transcript_viewer().is_none());
    assert_eq!(app.current_session_id(), Some("child"));
    assert_quote_to_parent(app);
    assert_entry_raw_mode(app);
    app.composer.vim_mode = false;
    app.set_frame_area(Rect::new(0, 0, 120, 40));
}

fn assert_pointer_copy(app: &mut AppState, area: Rect) {
    use crate::transcript_block_viewer::ViewerBlockContent;
    let original = app.transcript_viewer().unwrap_or_abort().content().clone();
    app.transcript_viewer
        .as_mut()
        .unwrap_or_abort()
        .update_content(ViewerBlockContent::markdown(
            "alpha beta\n\nsecond paragraph",
        ))
        .unwrap_or_abort();
    app.handle_key(key(KeyCode::Home));
    let layout = crate::transcript_block_viewer::viewer_layout(
        crate::layout::FrameLayoutPlan::for_app(app, area).shell,
    );
    let copied = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&copied);
    crate::clipboard::set_copy_override(Some(Box::new(move |text| {
        sink.lock().unwrap_or_abort().push(text.to_string());
        Ok(())
    })));
    let pointer = |app: &mut AppState, kind, column| {
        app.handle_pointer_event(
            MouseEvent {
                kind,
                column: layout.body.x + column,
                row: layout.body.y,
                modifiers: KeyModifiers::NONE,
            },
            area,
        );
    };
    pointer(app, MouseEventKind::Down(MouseButton::Left), 1);
    assert!(app
        .transcript_viewer()
        .unwrap_or_abort()
        .selection()
        .is_none());
    pointer(app, MouseEventKind::Drag(MouseButton::Left), 3);
    pointer(app, MouseEventKind::Up(MouseButton::Left), 4);
    assert_eq!(
        copied.lock().unwrap_or_abort().last().map(String::as_str),
        Some("lpha")
    );
    assert_eq!(
        app.transcript_viewer().unwrap_or_abort().quote_text(),
        "lpha"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(app
        .transcript_viewer()
        .unwrap_or_abort()
        .selection()
        .is_none());
    app.handle_key(key(KeyCode::Char('y')));
    assert_eq!(
        copied.lock().unwrap_or_abort().last().map(String::as_str),
        Some("alpha beta")
    );
    assert!(render_text(app, 80, 24).contains("Copied!"));
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(501));
    app.refresh_motion_state();
    assert!(!render_text(app, 80, 24).contains("Copied!"));
    for expected in [None, Some("alpha"), Some("alpha beta")] {
        pointer(app, MouseEventKind::Down(MouseButton::Left), 1);
        pointer(app, MouseEventKind::Up(MouseButton::Left), 1);
        let viewer = app.transcript_viewer().unwrap_or_abort();
        assert_eq!(viewer.copy_selection_text().ok().as_deref(), expected);
    }
    app.handle_key(key(KeyCode::Esc));
    let count = copied.lock().unwrap_or_abort().len();
    pointer(app, MouseEventKind::Down(MouseButton::Left), 40);
    pointer(app, MouseEventKind::Up(MouseButton::Left), 40);
    assert!(app
        .transcript_viewer()
        .unwrap_or_abort()
        .selection()
        .is_none());
    assert_eq!(copied.lock().unwrap_or_abort().len(), count);
    crate::clipboard::set_copy_override(None);
    app.transcript_viewer
        .as_mut()
        .unwrap_or_abort()
        .update_content(original)
        .unwrap_or_abort();
}

fn assert_entry_raw_mode(app: &mut AppState) {
    use crate::transcript_block_viewer::ViewerMode;

    let entry = app.transcript_view.selected_entry;
    app.handle_key(key(KeyCode::Char('r')));
    assert!(app.transcript_viewer().is_none());
    assert_eq!(app.transcript_view.selected_entry, entry);
    for _ in 0..2 {
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(app.transcript_viewer_mode(), Some(ViewerMode::Raw));
        app.handle_key(key(KeyCode::Char('R')));
        assert_eq!(app.transcript_viewer_mode(), Some(ViewerMode::Raw));
        app.handle_key(key(KeyCode::Esc));
    }
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Char('r')));
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.transcript_viewer_mode(), Some(ViewerMode::Wrapped));
    app.handle_key(key(KeyCode::Esc));
}

fn assert_raw_roundtrip(app: &mut AppState) {
    app.handle_key(key(KeyCode::Char('F')));
    app.handle_key(key(KeyCode::Char('r')));
    assert_eq!(
        app.transcript_viewer().unwrap_or_abort().quote_text(),
        "Second paragraph for filter selection."
    );
    app.handle_key(key(KeyCode::Char('r')));
    // The reference rebases the unfrozen tail's source map after a raw toggle.
    // Its next toggle therefore selects the earlier blank line.
    assert_eq!(app.transcript_viewer().unwrap_or_abort().quote_text(), "");
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.transcript_viewer().unwrap_or_abort().quote_text(), "");
}

fn assert_quote_to_parent(app: &mut AppState) {
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key_with_modifiers(KeyCode::Enter, KeyModifiers::ALT));
    assert!(app.transcript_viewer().is_some());
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.current_session_id(), Some("parent"));
    assert_eq!(app.focus, Focus::Prompt);
    assert!(!app.tasks_pane.focused);
    assert_eq!(
        app.composer.prompt_buffer,
        "> Second paragraph for filter selection.\n\n"
    );
    app.handle_key(key_with_modifiers(
        KeyCode::Char('z'),
        KeyModifiers::CONTROL,
    ));
    assert_eq!(app.composer.prompt_buffer, "parent draft");
    app.execute_action(Action::ToggleTasks);
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.current_session_id(), Some("child"));
}

fn assert_filter_selection(app: &mut AppState) {
    app.handle_key(key(KeyCode::End));
    app.handle_key(key(KeyCode::Char('f')));
    app.handle_paste("Second|^Read");
    app.handle_key(key(KeyCode::Enter));
    let viewer = app.transcript_viewer().unwrap_or_abort();
    assert!(viewer
        .render_surface(Rect::new(0, 0, 80, 24))
        .lines
        .iter()
        .any(|line| line.text.contains("Read")));
    assert_eq!(
        viewer.quote_text(),
        "Second paragraph for filter selection."
    );
    app.handle_key(key(KeyCode::Char('f')));
    app.handle_key(key_with_modifiers(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(
        app.transcript_viewer().unwrap_or_abort().quote_text(),
        "Second paragraph for filter selection."
    );
}
