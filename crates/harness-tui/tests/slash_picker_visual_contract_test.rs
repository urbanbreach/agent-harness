#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "deterministic rendering contracts use fail-fast asserts"
)]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::{app::AppState, render_test::render_to_buffer, ui};
use ratatui::layout::Rect;

fn row_containing(buffer: &ratatui::buffer::Buffer, needle: &str) -> (u16, u16) {
    for y in 0..buffer.area.height {
        let row = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        if let Some(x) = row.find(needle) {
            return (u16::try_from(x).expect("terminal width fits in u16"), y);
        }
    }
    panic!("expected terminal row containing {needle:?}");
}

#[test]
fn slash_picker_keeps_unselected_command_labels_readable() {
    // Given: the prompt-anchored slash picker is open with /agents selected.
    let mut app = AppState::new_live(None, false, None);
    app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    let expected_label_fg = app.theme().text.primary;

    // When: an unselected command row renders.
    let buffer = render_to_buffer(&app, Rect::new(0, 0, 100, 30), |app, frame, _area| {
        ui::render_app(frame, app)
    });
    let (auth_x, auth_y) = row_containing(&buffer, "/auth");

    // Then: its label uses the readable primary foreground, not palette-black text.
    assert_eq!(buffer[(auth_x, auth_y)].fg, expected_label_fg);
}
