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
    app.handle_key(key(KeyCode::Char('/')));
    app.handle_paste("histoy");
    app.handle_key(key(KeyCode::Left));
    app.handle_paste("r\n");
    let viewer = app.transcript_viewer().unwrap_or_abort();
    assert_eq!(viewer.search().query(), "history");
    assert_eq!(viewer.input.editor.cursor().insertion_index(), 6);
    assert_eq!(viewer.search().matches().len(), 1);
    app.handle_key(key(KeyCode::Enter));
    app.handle_paste("ignored");
    assert_eq!(
        app.transcript_viewer().unwrap_or_abort().search().query(),
        "history"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(app.transcript_viewer().is_none());
    assert_eq!(app.current_session_id(), Some("child"));
    app.handle_key(key(KeyCode::Enter));
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
    app.composer.vim_mode = false;
    app.set_frame_area(Rect::new(0, 0, 120, 40));
}
