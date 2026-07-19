//! Compact viewport parity (RESP-60x20 / RESP-79x24 / RESP-80x24 / RESP-100x30 /
//! RESP-120x40 / RESP-120x50 / RESP-WIDE).
//!
//! Contract: crates/harness-tui/DESIGN.md §3 + freeze captures under
//! artifacts/qa-evidence/20260717-tui-reference-parity/reference/freeze/.

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

fn startup_app() -> AppState {
    let mut app = AppState::new_startup(Vec::new(), None);
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

fn has_action_rows(rendered: &str) -> bool {
    let new = rendered.contains("New worktree") || rendered.contains("New session");
    let resume = rendered.contains("Resume session") || rendered.contains("Resume");
    let quit = rendered.contains("Quit") || rendered.contains("quit");
    new && resume && quit
}

fn has_changelog_body(rendered: &str) -> bool {
    rendered.contains("Changelog")
        && (rendered.contains('•')
            || rendered.contains("Event-sourced")
            || rendered.contains("changelog"))
}

/// RESP-80x24 startup: drop bordered welcome; keep unboxed actions + changelog + bordered composer.
#[test]
fn resp_80x24_startup_unboxed_welcome_keeps_composer_border() {
    // arrange
    let app = startup_app();

    // act
    let rendered = render_at(&app, 80, 24);

    // assert — breadcrumb / warning chrome
    let top: String = rendered.lines().take(8).collect::<Vec<_>>().join("\n");
    assert!(
        top.contains('')
            || top.contains("agent-harness")
            || rendered.contains("Clipboard may be unreachable."),
        "RESP-80x24: breadcrumb or clipboard warning required\n{rendered}"
    );
    assert!(
        rendered.contains("Clipboard may be unreachable."),
        "RESP-80x24: clipboard warning retained\n{rendered}"
    );

    // Unboxed welcome body (actions + changelog bullets)
    assert!(
        has_action_rows(&rendered),
        "RESP-80x24: unboxed action rows required\n{rendered}"
    );
    assert!(
        has_changelog_body(&rendered),
        "RESP-80x24: unboxed changelog list required\n{rendered}"
    );
    assert!(
        rendered.contains("ctrl+w") || rendered.contains("ctrl+q") || rendered.contains("ctrl+s"),
        "RESP-80x24: action shortcuts should remain visible\n{rendered}"
    );

    // Only composer is bordered (welcome box dropped) — single top-left rounded corner.
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-80x24: exactly one bordered box (composer); welcome must not be boxed\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "RESP-80x24: bordered composer glyph required\n{rendered}"
    );
    let lines: Vec<&str> = rendered.lines().collect();
    let glyph_idx = lines
        .iter()
        .rposition(|line| line.contains('│') && line.contains('❯'))
        .or_else(|| lines.iter().rposition(|line| line.contains('❯')))
        .expect("composer glyph row");
    let above = glyph_idx.checked_sub(1).map(|i| lines[i]).unwrap_or("");
    let below = lines.get(glyph_idx + 1).copied().unwrap_or("");
    assert!(
        above.contains('╭') || above.contains('─'),
        "RESP-80x24: composer top border retained\nabove={above:?}\n{rendered}"
    );
    assert!(
        below.contains('╰') || below.contains('─'),
        "RESP-80x24: composer bottom border retained\nbelow={below:?}\n{rendered}"
    );
}

/// RESP-80x24 draft: clear welcome/actions/changelog; keep breadcrumb + composer + Enter:send.
#[test]
fn resp_80x24_draft_clears_unboxed_welcome() {
    // arrange
    let mut app = startup_app();
    app.composer.prompt_buffer = "Browser QA draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();

    // act
    let rendered = render_at(&app, 80, 24);

    // assert
    assert!(
        rendered.contains("Browser QA draft"),
        "RESP-80x24 draft: composer retains draft\n{rendered}"
    );
    assert!(
        !rendered.contains("New worktree") && !rendered.contains("New session"),
        "RESP-80x24 draft: action rows must clear\n{rendered}"
    );
    assert!(
        !rendered.contains("Event-sourced agent harness"),
        "RESP-80x24 draft: changelog bullets must clear\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "RESP-80x24 draft: composer glyph retained\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-80x24 draft: only composer border\n{rendered}"
    );
    assert!(
        rendered.contains("Enter:send") || rendered.contains("Enter: send"),
        "RESP-80x24 draft: Enter:send footer grammar\n{rendered}"
    );
}

/// RESP-100x30 startup: retain bordered welcome + bordered composer (height gate must not drop panel).
#[test]
fn resp_100x30_startup_keeps_bordered_welcome() {
    // arrange
    let app = startup_app();

    // act
    let rendered = render_at(&app, 100, 30);

    // assert
    assert!(
        has_action_rows(&rendered),
        "RESP-100x30: welcome actions required\n{rendered}"
    );
    assert!(
        has_changelog_body(&rendered),
        "RESP-100x30: changelog section required\n{rendered}"
    );
    assert!(
        count_char(&rendered, '╭') >= 2,
        "RESP-100x30: bordered welcome + composer (got {} ╭)\n{rendered}",
        count_char(&rendered, '╭')
    );
    assert!(
        rendered.contains('❯'),
        "RESP-100x30: composer glyph required\n{rendered}"
    );
    // Logo/title mass present inside bordered panel (not compact-only unboxed actions).
    assert!(
        rendered.contains("██") || rendered.contains("Harness") || rendered.contains("Beta"),
        "RESP-100x30: bordered welcome should keep logo/title mass\n{rendered}"
    );
}

/// RESP-100x30 freeze ladder: clipboard at L4, welcome top at L7 (one blank less than 120x32).
/// Freezes: run1/run2/run3-startup-100x30 all agree; prior harness painted L5/L8 (+1 residual).
#[test]
fn resp_100x30_startup_matches_freeze_vertical_ladder() {
    // arrange
    let app = startup_app();

    // act
    let rendered = render_at(&app, 100, 30);
    let lines: Vec<&str> = rendered.lines().collect();

    // assert
    let clipboard_line = lines
        .iter()
        .position(|l| l.contains("Clipboard may be unreachable"))
        .map(|i| i + 1)
        .expect("clipboard warning required");
    let welcome_top = lines
        .iter()
        .position(|l| l.contains('╭') && (l.len() - l.trim_start_matches(' ').len()) >= 3)
        .map(|i| i + 1)
        .expect("bordered welcome top required");

    assert_eq!(
        clipboard_line, 4,
        "RESP-100x30 freeze ladder: clipboard at L4 (got L{clipboard_line})\n{rendered}"
    );
    assert_eq!(
        welcome_top, 7,
        "RESP-100x30 freeze ladder: welcome top at L7 (got L{welcome_top})\n{rendered}"
    );
    // 120x32 must stay at L5/L8 — regression guard for the 100x30-only adjustment.
    let rendered_32 = render_at(&app, 120, 32);
    let lines_32: Vec<&str> = rendered_32.lines().collect();
    let clipboard_32 = lines_32
        .iter()
        .position(|l| l.contains("Clipboard may be unreachable"))
        .map(|i| i + 1)
        .expect("120x32 clipboard");
    let welcome_32 = lines_32
        .iter()
        .position(|l| l.contains('╭') && (l.len() - l.trim_start_matches(' ').len()) >= 3)
        .map(|i| i + 1)
        .expect("120x32 welcome");
    assert_eq!(
        clipboard_32, 5,
        "120x32 freeze ladder must remain clipboard L5 (got L{clipboard_32})\n{rendered_32}"
    );
    assert_eq!(
        welcome_32, 8,
        "120x32 freeze ladder must remain welcome L8 (got L{welcome_32})\n{rendered_32}"
    );
}

/// RESP-120x40 startup: same bordered welcome+composer anatomy with extra vertical gap.
#[test]
fn resp_120x40_startup_keeps_bordered_welcome() {
    // arrange
    let app = startup_app();

    // act
    let rendered = render_at(&app, 120, 40);

    // assert
    assert!(
        has_action_rows(&rendered),
        "RESP-120x40: welcome actions required\n{rendered}"
    );
    assert!(
        count_char(&rendered, '╭') >= 2,
        "RESP-120x40: bordered welcome + composer (got {} ╭)\n{rendered}",
        count_char(&rendered, '╭')
    );
    assert!(
        rendered.contains('❯'),
        "RESP-120x40: composer glyph required\n{rendered}"
    );
    let lines: Vec<&str> = rendered.lines().collect();
    let welcome_top = lines
        .iter()
        .position(|line| line.contains('╭') && line.contains('─'))
        .expect("welcome top border");
    let composer_glyph = lines
        .iter()
        .rposition(|line| line.contains('│') && line.contains('❯'))
        .or_else(|| lines.iter().rposition(|line| line.contains('❯')))
        .expect("composer glyph");
    assert!(
        composer_glyph > welcome_top + 10,
        "RESP-120x40: welcome sits above composer with room for panel body\n{rendered}"
    );
}

/// RESP-60x20 startup (freeze run1-startup-60x20): unboxed actions + changelog title;
/// only composer is bordered.
#[test]
fn resp_60x20_startup_unboxed_welcome_keeps_composer_border() {
    // arrange
    let app = startup_app();

    // act
    let rendered = render_at(&app, 60, 20);

    // assert
    assert!(
        has_action_rows(&rendered),
        "RESP-60x20: unboxed action rows required\n{rendered}"
    );
    assert!(
        rendered.contains("Changelog") || rendered.contains("changelog"),
        "RESP-60x20: changelog section retained\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-60x20: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "RESP-60x20: bordered composer glyph required\n{rendered}"
    );
    assert!(
        rendered.contains("Clipboard may be unreachable.")
            || rendered.contains('')
            || rendered.contains("agent-harness"),
        "RESP-60x20: top chrome retained\n{rendered}"
    );
}

/// RESP-79x24 startup (freeze run1-startup-79x24): unboxed welcome + changelog bullets;
/// only composer is bordered (width still compact).
#[test]
fn resp_79x24_startup_unboxed_welcome_keeps_composer_border() {
    // arrange
    let app = startup_app();

    // act
    let rendered = render_at(&app, 79, 24);

    // assert
    assert!(
        has_action_rows(&rendered),
        "RESP-79x24: unboxed action rows required\n{rendered}"
    );
    assert!(
        has_changelog_body(&rendered),
        "RESP-79x24: unboxed changelog list required\n{rendered}"
    );
    assert_eq!(
        count_char(&rendered, '╭'),
        1,
        "RESP-79x24: exactly one bordered box (composer)\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "RESP-79x24: bordered composer glyph required\n{rendered}"
    );
    assert!(
        rendered.contains("ctrl+w") || rendered.contains("ctrl+q") || rendered.contains("ctrl+s"),
        "RESP-79x24: action shortcuts should remain visible\n{rendered}"
    );
}

/// RESP-120x50 startup (freeze run1-startup-120x50): bordered welcome + composer
/// with extra vertical gap above/below the panel.
#[test]
fn resp_120x50_startup_keeps_bordered_welcome() {
    // arrange
    let app = startup_app();

    // act
    let rendered = render_at(&app, 120, 50);

    // assert
    assert!(
        has_action_rows(&rendered),
        "RESP-120x50: welcome actions required\n{rendered}"
    );
    assert!(
        has_changelog_body(&rendered),
        "RESP-120x50: changelog section required\n{rendered}"
    );
    assert!(
        count_char(&rendered, '╭') >= 2,
        "RESP-120x50: bordered welcome + composer (got {} ╭)\n{rendered}",
        count_char(&rendered, '╭')
    );
    assert!(
        rendered.contains('❯'),
        "RESP-120x50: composer glyph required\n{rendered}"
    );
    let lines: Vec<&str> = rendered.lines().collect();
    let welcome_top = lines
        .iter()
        .position(|line| line.contains('╭') && line.contains('─'))
        .expect("welcome top border");
    let composer_glyph = lines
        .iter()
        .rposition(|line| line.contains('│') && line.contains('❯'))
        .or_else(|| lines.iter().rposition(|line| line.contains('❯')))
        .expect("composer glyph");
    assert!(
        composer_glyph > welcome_top + 12,
        "RESP-120x50: welcome sits above composer with tall panel body\n{rendered}"
    );
}

/// RESP-WIDE startup (freeze run1-startup-140x40): bordered welcome + composer on wide canvas.
#[test]
fn resp_wide_140x40_startup_keeps_bordered_welcome() {
    // arrange
    let app = startup_app();

    // act
    let rendered = render_at(&app, 140, 40);

    // assert
    assert!(
        has_action_rows(&rendered),
        "RESP-WIDE: welcome actions required\n{rendered}"
    );
    assert!(
        has_changelog_body(&rendered),
        "RESP-WIDE: changelog section required\n{rendered}"
    );
    assert!(
        count_char(&rendered, '╭') >= 2,
        "RESP-WIDE: bordered welcome + composer (got {} ╭)\n{rendered}",
        count_char(&rendered, '╭')
    );
    assert!(
        rendered.contains('❯'),
        "RESP-WIDE: composer glyph required\n{rendered}"
    );
    assert!(
        rendered.contains("██") || rendered.contains("Harness") || rendered.contains("Beta"),
        "RESP-WIDE: bordered welcome should keep logo/title mass\n{rendered}"
    );
}
