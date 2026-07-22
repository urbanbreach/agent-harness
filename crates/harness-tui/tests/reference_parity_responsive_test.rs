//! Compact viewport parity (RESP-60x20 / RESP-79x24 / RESP-80x24 / RESP-100x30 /
//! RESP-120x40 / RESP-120x50 / RESP-WIDE).
//!
//! Reference freeze (run1-resp-*-pinned-v1) shows real HOME idle shell at each
//! viewport: breadcrumb + empty transcript body + bordered composer (empty
//! prompt) + idle footer (Shift+Tab:mode | Ctrl+x:shortcuts). No welcome panel.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "integration parity tests use fail-fast asserts"
)]

use harness_tui::app::{AppState, LaunchMetadata};
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

fn idle_shell_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );
    app
}

fn render_at(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn count_char(rendered: &str, ch: char) -> usize {
    rendered.chars().filter(|c| *c == ch).count()
}

/// RESP-80x24 idle shell: breadcrumb + empty body + bordered composer + idle footer.
#[test]
fn resp_80x24_idle_shell_keeps_bordered_composer() {
    // arrange
    let app = idle_shell_app();

    // act
    let rendered = render_at(&app, 80, 24);

    // assert
    assert!(
        rendered.contains('❯'),
        "RESP-80x24: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-80x24: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(
        rendered.contains("Shift+Tab:mode") || rendered.contains("Ctrl+x:shortcuts"),
        "RESP-80x24: idle footer required\n{rendered}"
    );
    assert!(
        !rendered.contains("New worktree") && !rendered.contains("New session"),
        "RESP-80x24: welcome actions must not appear in idle shell\n{rendered}"
    );
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-80x24: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-79x24 idle shell: same anatomy as 80x24 at narrow boundary width.
#[test]
fn resp_79x24_idle_shell_keeps_bordered_composer() {
    // arrange
    let app = idle_shell_app();

    // act
    let rendered = render_at(&app, 79, 24);

    // assert
    assert!(
        rendered.contains('❯'),
        "RESP-79x24: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-79x24: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(
        rendered.contains("Shift+Tab:mode") || rendered.contains("Ctrl+x:shortcuts"),
        "RESP-79x24: idle footer required\n{rendered}"
    );
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-79x24: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-100x30 idle shell: breadcrumb + empty body + bordered composer + idle footer.
#[test]
fn resp_100x30_idle_shell_keeps_bordered_composer() {
    // arrange
    let app = idle_shell_app();

    // act
    let rendered = render_at(&app, 100, 30);

    // assert
    assert!(
        rendered.contains('❯'),
        "RESP-100x30: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-100x30: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(
        rendered.contains("Shift+Tab:mode") || rendered.contains("Ctrl+x:shortcuts"),
        "RESP-100x30: idle footer required\n{rendered}"
    );
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-100x30: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-120x40 idle shell: same anatomy with extra vertical gap.
#[test]
fn resp_120x40_idle_shell_keeps_bordered_composer() {
    // arrange
    let app = idle_shell_app();

    // act
    let rendered = render_at(&app, 120, 40);

    // assert
    assert!(
        rendered.contains('❯'),
        "RESP-120x40: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-120x40: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(
        rendered.contains("Shift+Tab:mode") || rendered.contains("Ctrl+x:shortcuts"),
        "RESP-120x40: idle footer required\n{rendered}"
    );
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-120x40: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-60x20 idle shell (freeze run1-resp-60x20-pinned-v1): compact idle shell.
#[test]
fn resp_60x20_idle_shell_keeps_bordered_composer() {
    // arrange
    let app = idle_shell_app();

    // act
    let rendered = render_at(&app, 60, 20);

    // assert
    assert!(
        rendered.contains('❯'),
        "RESP-60x20: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-60x20: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(
        rendered.contains("Shift+Tab:mode") || rendered.contains("Ctrl+x:shortcuts"),
        "RESP-60x20: idle footer required\n{rendered}"
    );
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-60x20: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-120x50 idle shell (freeze run1-resp-120x50-pinned-v1): tall idle shell.
#[test]
fn resp_120x50_idle_shell_keeps_bordered_composer() {
    // arrange
    let app = idle_shell_app();

    // act
    let rendered = render_at(&app, 120, 50);

    // assert
    assert!(
        rendered.contains('❯'),
        "RESP-120x50: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-120x50: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(
        rendered.contains("Shift+Tab:mode") || rendered.contains("Ctrl+x:shortcuts"),
        "RESP-120x50: idle footer required\n{rendered}"
    );
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-120x50: idle shell must not show draft footer\n{rendered}"
    );
}

/// RESP-WIDE idle shell (freeze run1-resp-140x40-pinned-v1): wide idle shell.
#[test]
fn resp_wide_140x40_idle_shell_keeps_bordered_composer() {
    // arrange
    let app = idle_shell_app();

    // act
    let rendered = render_at(&app, 140, 40);

    // assert
    assert!(
        rendered.contains('❯'),
        "RESP-WIDE: composer glyph required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-WIDE: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(
        rendered.contains("Shift+Tab:mode") || rendered.contains("Ctrl+x:shortcuts"),
        "RESP-WIDE: idle footer required\n{rendered}"
    );
    assert!(
        !rendered.contains("Enter:send"),
        "RESP-WIDE: idle shell must not show draft footer\n{rendered}"
    );
}
