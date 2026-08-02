#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "deterministic rendering contracts use fail-fast asserts"
)]

use harness_tui::{
    app::AppState,
    render_test::{render_to_buffer, render_to_string},
    ui,
};
use ratatui::layout::Rect;

fn cell_for(buffer: &ratatui::buffer::Buffer, needle: &str) -> ratatui::buffer::Cell {
    for y in 0..buffer.area.height {
        let row = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>();
        if let Some(x) = row.find(needle) {
            return buffer[(u16::try_from(x).expect("terminal width fits in u16"), y)].clone();
        }
    }
    panic!("expected terminal cell containing {needle:?}");
}

#[test]
fn theme_dialog_labels_the_default_theme_harness_chat() {
    // Given: the active theme picker is rendering the canonical default entry.
    let mut app = AppState::new_live(None, false, None);
    app.theme_dialog_visible = true;

    // When: its live overlay is rendered.
    let rendered = render_to_string(&app, Rect::new(0, 0, 100, 30), |app, frame, _area| {
        ui::render_app(frame, app);
    });

    // Then: the visible label matches the current default theme identity.
    assert!(rendered.contains("Harness Chat"), "{rendered}");
    assert!(!rendered.contains("Harness Dark"), "{rendered}");
    assert!(rendered.contains("Themes"), "{rendered}");
    assert!(!rendered.contains("Commands"), "{rendered}");

    let buffer = render_to_buffer(&app, Rect::new(0, 0, 100, 30), |app, frame, _area| {
        ui::render_app(frame, app);
    });
    assert_eq!(
        cell_for(&buffer, "harness-light").fg,
        app.theme().text.primary
    );
    assert_eq!(
        cell_for(&buffer, "enter apply").fg,
        app.theme().text.tertiary
    );
}
