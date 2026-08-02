//! Startup shell geometry locked to the frozen Grok Build welcome captures.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration geometry tests use fail-fast assertions"
)]

use harness_tui::app::AppState;
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

const CLIPBOARD_WARNING: &str = "Clipboard may be unreachable.";
const TERMINAL_SETUP_HINT: &str = "See /terminal-setup for potential fixes.";

fn startup_app_with_clipboard_warning() -> AppState {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.status_banner = Some(CLIPBOARD_WARNING.to_string());
    app
}

fn render_at(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn rounded_top_border_rows(rendered: &str) -> Vec<usize> {
    rendered
        .lines()
        .enumerate()
        .filter_map(|(index, line)| line.contains('╭').then_some(index))
        .collect()
}

fn composer_height(rendered: &str) -> Option<usize> {
    let top = rounded_top_border_rows(rendered).last().copied()?;
    let rows: Vec<&str> = rendered.lines().collect();
    let bottom =
        rows.iter().enumerate().rev().find_map(|(index, line)| {
            (line.contains('╰') && line.contains('╯')).then_some(index)
        })?;
    Some(bottom.saturating_sub(top).saturating_add(1))
}

#[test]
fn startup_warning_uses_the_frozen_two_line_band_at_120x32() {
    // Given: the startup shell reports the frozen clipboard capability warning.
    let app = startup_app_with_clipboard_warning();

    // When: the primary 120×32 viewport renders.
    let rendered = render_at(&app, 120, 32);
    let rows: Vec<&str> = rendered.lines().collect();

    // Then: the two reference lines occupy their own right-biased band above the welcome card.
    assert!(
        rows[4].trim_end().ends_with(CLIPBOARD_WARNING),
        "{rendered}"
    );
    assert!(
        rows[5].trim_end().ends_with(TERMINAL_SETUP_HINT),
        "{rendered}"
    );
    assert_eq!(
        rounded_top_border_rows(&rendered),
        vec![7, 26],
        "{rendered}"
    );
}

#[test]
fn startup_runtime_clipboard_banner_uses_the_same_frozen_warning_band() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.status_banner =
        Some("System clipboard may be inaccessible, copy will use OSC 52.".to_string());

    let rendered = render_at(&app, 120, 32);
    let rows: Vec<&str> = rendered.lines().collect();

    assert!(
        rows[4].trim_end().ends_with(CLIPBOARD_WARNING),
        "{rendered}"
    );
    assert!(
        rows[5].trim_end().ends_with(TERMINAL_SETUP_HINT),
        "{rendered}"
    );
    assert!(
        !rendered.contains("Notices: System clipboard"),
        "{rendered}"
    );
}

#[test]
fn startup_shell_preserves_welcome_and_composer_breakpoints() {
    // Given: startup with the frozen clipboard warning enabled.
    let app = startup_app_with_clipboard_warning();

    // When / Then: every requested viewport keeps its measured welcome-collapse and composer geometry.
    for (width, height, welcome_is_boxed) in [
        (120, 32, true),
        (120, 40, true),
        (100, 30, true),
        (80, 24, false),
        (79, 24, false),
        (60, 20, false),
        (140, 40, true),
    ] {
        let rendered = render_at(&app, width, height);
        let expected_boxes = if welcome_is_boxed { 2 } else { 1 };
        assert_eq!(
            rounded_top_border_rows(&rendered).len(),
            expected_boxes,
            "startup box count at {width}x{height}\n{rendered}"
        );
        assert_eq!(
            composer_height(&rendered),
            Some(3),
            "startup composer height at {width}x{height}\n{rendered}"
        );
    }
}
