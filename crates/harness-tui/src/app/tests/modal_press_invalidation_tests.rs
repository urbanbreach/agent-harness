use super::*;

pub(super) fn modal_key_event_invalidates_armed_press() {
    // Given: a command row is armed for release activation.
    let (mut app, column, row) = armed_settings_palette();

    // When: keyboard navigation takes ownership before the release.
    app.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
    release(&mut app, column, row, TEST_FRAME_AREA);

    // Then: the stale pointer press cannot activate the command.
    assert!(!app.settings_editor_visible);
}

pub(super) fn modal_resize_invalidates_armed_press() {
    // Given: a command row is armed against the current frame geometry.
    let (mut app, column, row) = armed_settings_palette();

    // When: terminal geometry changes before the release.
    app.set_frame_area(Rect::new(0, 0, 80, 24));
    release(&mut app, column, row, TEST_FRAME_AREA);

    // Then: the stale geometry cannot activate the command.
    assert!(!app.settings_editor_visible);
}

pub(super) fn modal_non_left_event_invalidates_armed_press() {
    // Given: a command row is armed by the left button.
    let (mut app, column, row) = armed_settings_palette();

    // When: a non-left button event interrupts the press lifecycle.
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
    release(&mut app, column, row, TEST_FRAME_AREA);

    // Then: the interrupted press cannot activate the command.
    assert!(!app.settings_editor_visible);
}

pub(super) fn modal_owner_change_invalidates_armed_press() {
    // Given: a palette row is armed while the palette owns interaction.
    let (mut app, column, row) = armed_settings_palette();

    // When: a theme dialog becomes the top modal before release.
    app.theme_dialog_visible = true;
    release(&mut app, column, row, TEST_FRAME_AREA);

    // Then: the prior owner's press cannot activate through the new owner.
    assert!(!app.settings_editor_visible);
    assert!(app.theme_dialog_visible);
}

fn armed_settings_palette() -> (AppState, u16, u16) {
    let mut app = AppState::new_live(None, false, None);
    app.open_palette();
    for character in "settings".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }
    app.set_frame_area(TEST_FRAME_AREA);
    let model = crate::ui::ui_overlays::modal_surface_model(&app, TEST_FRAME_AREA)
        .expect("command palette surface model");
    let area = model
        .regions
        .iter()
        .find_map(|region| (region.target == ModalTarget::Row(0)).then_some(region.area))
        .expect("settings row");
    let column = area.x.saturating_add(area.width / 2);
    let row = area.y.saturating_add(area.height / 2);
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
    (app, column, row)
}

fn release(app: &mut AppState, column: u16, row: u16, area: Rect) {
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
}
