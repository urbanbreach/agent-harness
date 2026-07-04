use super::*;

use super::opencode_subagent_parity_apps as app_fixtures;

pub(super) fn subagent_footer_hover_elevates_parent_target() {
    let mut app = app_fixtures::child_footer_app();
    let (column, row) = footer_target_position(&app, "Parent ↑");

    assert_eq!(
        subagent_footer_target_at(&app, TEST_FRAME_AREA, column, row),
        Some(SubagentFooterTarget::Parent)
    );
    assert_ne!(
        rendered_cell_bg(&app, column, row),
        Theme::default().surface.panel_elevated
    );

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

    assert_eq!(
        app.hovered_subagent_footer_target,
        Some(SubagentFooterTarget::Parent)
    );
    assert_eq!(
        rendered_cell_bg(&app, column, row),
        Theme::default().surface.panel_elevated
    );
    assert_eq!(app.hovered_transcript_target(), None);
}

pub(super) fn subagent_footer_parent_click_restores_parent_session() {
    let mut app = app_fixtures::child_footer_app();
    let (column, row) = footer_target_position(&app, "Parent ↑");

    click_footer_target(&mut app, column, row);

    assert_ne!(
        app.overlay_stack().top(),
        Some(OverlayKind::SubagentActions)
    );
    assert_eq!(app.current_session_id(), Some("parent"));
}

pub(super) fn subagent_footer_sibling_clicks_switch_between_children() {
    let mut app = app_fixtures::sibling_after_navigation_app();
    assert_eq!(app.current_session_id(), Some("child_a"));

    let (next_column, next_row) = footer_target_position(&app, "Next →");
    assert_eq!(
        subagent_footer_target_at(&app, TEST_FRAME_AREA, next_column, next_row),
        Some(SubagentFooterTarget::Next)
    );
    click_footer_target(&mut app, next_column, next_row);
    assert_eq!(app.current_session_id(), Some("child_b"));

    let (prev_column, prev_row) = footer_target_position(&app, "Prev ←");
    assert_eq!(
        subagent_footer_target_at(&app, TEST_FRAME_AREA, prev_column, prev_row),
        Some(SubagentFooterTarget::Previous)
    );
    click_footer_target(&mut app, prev_column, prev_row);
    assert_eq!(app.current_session_id(), Some("child_a"));
}

fn footer_target_position(app: &AppState, needle: &str) -> (u16, u16) {
    transcript_click_position(app, needle)
}

fn click_footer_target(app: &mut AppState, column: u16, row: u16) {
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
}

pub(super) fn subagent_footer_scrollbar_drag_release_does_not_navigate() {
    let mut app = app_fixtures::child_footer_app();
    let before = app.current_session_id().map(str::to_owned);
    let (footer_col, footer_row) = footer_target_position(&app, "Parent ↑");
    assert_eq!(
        subagent_footer_target_at(&app, TEST_FRAME_AREA, footer_col, footer_row),
        Some(SubagentFooterTarget::Parent)
    );

    app.transcript_view.last_transcript_max_scroll.set(100);
    app.transcript_view.follow_mode = false;
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
    assert!(
        app.transcript_scrollbar_dragging(),
        "scrollbar drag should begin on thumb down"
    );

    app.handle_mouse(
        MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: footer_col,
            row: footer_row,
            modifiers: KeyModifiers::NONE,
        },
        TEST_FRAME_AREA,
        None,
        None,
        None,
    );

    assert_eq!(
        app.current_session_id(),
        before.as_deref(),
        "scrollbar drag released over footer must not navigate"
    );
    assert!(
        !app.transcript_scrollbar_dragging(),
        "drag state must be cleared after release"
    );
    assert!(
        app.pending_subagent_footer_target.is_none(),
        "no pending footer press should remain after release"
    );
}

pub(super) fn subagent_footer_up_only_release_does_not_activate() {
    let mut app = app_fixtures::child_footer_app();
    let before = app.current_session_id().map(str::to_owned);
    let (column, row) = footer_target_position(&app, "Parent ↑");

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
    assert_eq!(
        app.hovered_subagent_footer_target,
        Some(SubagentFooterTarget::Parent)
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

    assert_eq!(
        app.current_session_id(),
        before.as_deref(),
        "mouse-up without a preceding down on the footer must not navigate"
    );
    assert!(
        app.pending_subagent_footer_target.is_none(),
        "no pending footer press should remain after release"
    );
}
