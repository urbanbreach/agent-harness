use super::*;

#[test]
fn settled_detached_transcript_return_restores_follow_mode() {
    // arrange — Given a settled, overflowing transcript detached at its top.
    let transcript = (0..80)
        .map(|line| format!("settled response line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut app = transcript_selection_test_app_with_text(&transcript);
    let area = Rect::new(0, 0, 80, 24);
    app.set_frame_area(area);
    let _ = render_text(&app, area.width, area.height);
    let max_scroll = app.transcript_view.viewport.max_scroll();
    assert!(
        max_scroll > 0,
        "fixture must overflow the transcript viewport"
    );
    assert!(!app.active_turn_in_progress());
    app.transcript_view.set_following(false);
    app.transcript_view.set_offset(max_scroll);
    app.transcript_view.return_to_live_hovered = true;
    let (column, row) = (0..area.height)
        .flat_map(|row| (0..area.width).map(move |column| (column, row)))
        .find(|(column, row)| crate::ui::transcript_return_to_live_hit(&app, area, *column, *row))
        .expect("detached transcript must expose the centered return affordance");

    // act — When the operator activates the affordance after the turn has settled.
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

    // assert — Then the viewport jumps to the bottom and resumes following.
    assert!(app.transcript_following());
    assert_eq!(app.transcript_scroll_offset(), 0);
    assert!(!app.transcript_view.return_to_live_hovered);
}
