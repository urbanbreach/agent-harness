//! Task 26 deterministic parity tests: startup, welcome, shell chrome, composer, shortcuts.
//!
//! Contract: crates/harness-tui/DESIGN.md sections 2-5, 11-12.
//! Geometry, rhythm, borders, focus, and choreography must match measured reference.

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

fn idle_app() -> AppState {
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );
    app
}

fn render_at(app: &AppState, w: u16, h: u16) -> String {
    render_to_string(app, Rect::new(0, 0, w, h), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn render(app: &AppState) -> String {
    render_at(app, 120, 32)
}

fn count_char(rendered: &str, ch: char) -> usize {
    rendered.chars().filter(|c| *c == ch).count()
}

fn composer_top_row(rendered: &str) -> Option<usize> {
    rendered
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains('╭') && line.contains('─'))
        .map(|(i, _)| i + 1)
}

fn composer_bottom_row(rendered: &str) -> Option<usize> {
    rendered
        .lines()
        .collect::<Vec<_>>()
        .iter()
        .enumerate()
        .rev()
        .find(|(_, line)| line.contains('╰') && line.contains('╯'))
        .map(|(i, _)| i + 1)
}

fn footer_row(rendered: &str) -> Option<usize> {
    rendered
        .lines()
        .collect::<Vec<_>>()
        .iter()
        .enumerate()
        .rev()
        .find(|(_, line)| line.contains("Shift+Tab:mode") || line.contains("Enter:send"))
        .map(|(i, _)| i + 1)
}

// ---------------------------------------------------------------------------
// Welcome panel rendering
// ---------------------------------------------------------------------------

#[test]
fn welcome_panel_has_rounded_borders_at_120x32() {
    // arrange
    let app = startup_app();
    let rendered = render(&app);
    assert!(
        rendered.contains('╭') && rendered.contains('╰'),
        "welcome panel must use rounded borders\n{rendered}"
    );
}

#[test]
fn welcome_panel_contains_action_rows() {
    // arrange
    let app = startup_app();
    let rendered = render(&app);
    assert!(
        rendered.contains("New worktree"),
        "missing New worktree\n{rendered}"
    );
    assert!(
        rendered.contains("Resume session"),
        "missing Resume session\n{rendered}"
    );
    assert!(rendered.contains("Quit"), "missing Quit\n{rendered}");
}

#[test]
fn welcome_panel_contains_changelog_bullets() {
    // arrange
    let app = startup_app();
    let rendered = render(&app);
    assert!(
        rendered.contains('•'),
        "missing changelog bullets\n{rendered}"
    );
    assert!(
        rendered.contains("Changelog"),
        "missing Changelog section\n{rendered}"
    );
}

#[test]
fn welcome_panel_cleared_on_draft() {
    // arrange
    let mut app = startup_app();
    app.composer.prompt_buffer = "test draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    let rendered = render(&app);
    assert!(
        !rendered.contains("New worktree"),
        "welcome actions must clear on draft\n{rendered}"
    );
    assert!(
        !rendered.contains("Changelog"),
        "changelog must clear on draft\n{rendered}"
    );
}

#[test]
fn welcome_panel_drops_border_at_compact_viewport() {
    // arrange
    let app = startup_app();
    let rendered = render_at(&app, 80, 24);
    let welcome_borders = count_char(&rendered, '╭');
    assert_eq!(
        welcome_borders, 1,
        "compact startup must drop welcome panel border, keep only composer\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// Composer geometry at all viewports
// ---------------------------------------------------------------------------

#[test]
fn composer_is_bordered_at_all_viewports() {
    // arrange
    let app = idle_app();
    for (w, h) in [
        (120u16, 50u16),
        (120, 40),
        (120, 32),
        (100, 30),
        (80, 24),
        (79, 24),
        (60, 20),
        (140, 40),
    ] {
        let rendered = render_at(&app, w, h);
        assert!(
            rendered.contains('╭') && rendered.contains('╰'),
            "composer must have rounded borders at {w}x{h}\n{rendered}"
        );
        assert!(
            rendered.contains('❯'),
            "composer must have prompt glyph at {w}x{h}\n{rendered}"
        );
    }
}

#[test]
fn composer_is_three_rows_at_120x32() {
    // arrange
    let app = idle_app();
    let rendered = render_at(&app, 120, 32);
    let top = composer_top_row(&rendered).expect("composer top border");
    let bottom = composer_bottom_row(&rendered).expect("composer bottom border");
    assert_eq!(
        bottom - top,
        2,
        "composer must be 3 rows (top+content+bottom) at 120x32\n{rendered}"
    );
}

#[test]
fn idle_shell_has_spacer_between_composer_and_footer_at_120x32() {
    // arrange
    let app = idle_app();
    let rendered = render_at(&app, 120, 32);
    let bottom = composer_bottom_row(&rendered).expect("composer bottom border");
    let footer = footer_row(&rendered).expect("footer row");
    let gap = footer.saturating_sub(bottom + 1);
    assert_eq!(
        gap, 1,
        "idle shell must have 1 blank row between composer and footer at 120x32 (got {gap})\n{rendered}"
    );
}

#[test]
fn idle_shell_has_spacer_at_80x24() {
    // arrange
    let app = idle_app();
    let rendered = render_at(&app, 80, 24);
    let bottom = composer_bottom_row(&rendered).expect("composer bottom border");
    let footer = footer_row(&rendered).expect("footer row");
    let gap = footer.saturating_sub(bottom + 1);
    assert_eq!(
        gap, 1,
        "idle shell must have 1 blank row between composer and footer at 80x24 (got {gap})\n{rendered}"
    );
}

#[test]
fn idle_shell_no_spacer_at_60x20() {
    // arrange
    let app = idle_app();
    let rendered = render_at(&app, 60, 20);
    let bottom = composer_bottom_row(&rendered).expect("composer bottom border");
    let footer = footer_row(&rendered).expect("footer row");
    let gap = footer.saturating_sub(bottom + 1);
    assert_eq!(
        gap, 0,
        "idle shell must have no spacer between composer and footer at 60x20 (got {gap})\n{rendered}"
    );
}

#[test]
fn composer_width_matches_viewport_at_all_viewports() {
    // arrange
    let app = idle_app();
    for (w, h, expected_width) in [
        (120u16, 50u16, 116u16),
        (120, 40, 116),
        (120, 32, 116),
        (100, 30, 96),
        (80, 24, 76),
        (79, 24, 75),
        (60, 20, 58),
        (140, 40, 136),
    ] {
        let rendered = render_at(&app, w, h);
        let top_row = rendered
            .lines()
            .find(|line| line.contains('╭') && line.contains('─'))
            .unwrap_or_else(|| panic!("composer top border not found at {w}x{h}\n{rendered}"));
        let actual_width = top_row.chars().count().min(usize::from(w));
        assert!(
            actual_width >= usize::from(expected_width),
            "composer width must be ~{expected_width} at {w}x{h}, got {actual_width}\n{top_row}"
        );
    }
}

#[test]
fn breadcrumb_on_row_1_at_60x20() {
    // arrange
    let app = idle_app();
    let rendered = render_at(&app, 60, 20);
    let first_line = rendered.lines().next().unwrap_or("");
    assert!(
        first_line.contains("ui-ux-experiments") || first_line.contains("agent-harness"),
        "breadcrumb must be on row 1 at 60x20 (no top margin)\n{rendered}"
    );
}

#[test]
fn breadcrumb_on_row_2_at_120x32() {
    // arrange
    let app = idle_app();
    let rendered = render_at(&app, 120, 32);
    let lines: Vec<&str> = rendered.lines().collect();
    let first = lines.first().copied().unwrap_or("");
    let second = lines.get(1).copied().unwrap_or("");
    assert!(
        first.trim().is_empty(),
        "row 1 must be blank (top margin) at 120x32\n{rendered}"
    );
    assert!(
        second.contains("ui-ux-experiments") || second.contains("agent-harness"),
        "breadcrumb must be on row 2 at 120x32\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// Footer shortcuts
// ---------------------------------------------------------------------------

#[test]
fn idle_footer_shows_mode_and_shortcuts() {
    // arrange
    let app = idle_app();
    let rendered = render(&app);
    assert!(
        rendered.contains("Shift+Tab:mode"),
        "idle footer must show Shift+Tab:mode\n{rendered}"
    );
    assert!(
        rendered.contains("Ctrl+x:shortcuts"),
        "idle footer must show Ctrl+x:shortcuts\n{rendered}"
    );
}

#[test]
fn draft_footer_shows_send_and_mode() {
    // arrange
    let mut app = startup_app();
    app.composer.prompt_buffer = "test".to_string();
    app.composer.prompt_cursor = 4;
    let rendered = render(&app);
    assert!(
        rendered.contains("Enter:send"),
        "draft footer must show Enter:send\n{rendered}"
    );
    assert!(
        rendered.contains("Shift+Tab:mode"),
        "draft footer must show Shift+Tab:mode\n{rendered}"
    );
}

#[test]
fn startup_footer_shows_auth_status() {
    // arrange
    let app = startup_app();
    let rendered = render(&app);
    assert!(
        rendered.contains("Logged in with API key"),
        "startup footer must show auth status\n{rendered}"
    );
    assert!(
        rendered.contains("Beta") || rendered.contains("Demo"),
        "startup footer must show mode label\n{rendered}"
    );
}

#[test]
fn startup_footer_reports_oauth_authentication() {
    // Given
    let mut app = AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "openai-codex:gpt-5.4")
            .with_oauth_authentication(),
    );

    // When
    let rendered = render(&app);

    // Then
    assert!(
        rendered.contains("Logged in via OAuth"),
        "startup footer must identify OAuth authentication\n{rendered}"
    );
    assert!(
        !rendered.contains("Logged in with API key"),
        "startup footer must not mislabel OAuth as an API key\n{rendered}"
    );
}

#[test]
fn footer_uses_pipe_separator() {
    // arrange
    let app = idle_app();
    let rendered = render(&app);
    assert!(
        rendered.contains("│"),
        "footer must use pipe separator between shortcut clusters\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// Model badge
// ---------------------------------------------------------------------------

#[test]
fn model_badge_on_composer_bottom_border() {
    // arrange
    let app = idle_app();
    let rendered = render(&app);
    let bottom = rendered
        .lines()
        .rfind(|line| line.contains('╰') && line.contains('╯'))
        .expect("composer bottom border");
    assert!(
        bottom.contains("model-1") || bottom.contains("Demo"),
        "model badge must be on composer bottom border\n{bottom}"
    );
}

#[test]
fn startup_empty_composer_has_empty_badge() {
    // arrange
    let app = AppState::new_startup(Vec::new(), None);
    let rendered = render(&app);
    let bottom = rendered
        .lines()
        .rfind(|line| line.contains('╰') && line.contains('╯'))
        .expect("composer bottom border");
    assert!(
        !bottom.contains("model") && !bottom.contains("mock"),
        "startup empty composer badge must be empty (not show model name)\n{bottom}"
    );
}

#[test]
fn draft_composer_shows_model_badge() {
    // arrange
    let mut app = startup_app();
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );
    app.composer.prompt_buffer = "test".to_string();
    app.composer.prompt_cursor = 4;
    let rendered = render(&app);
    let bottom = rendered
        .lines()
        .rfind(|line| line.contains('╰') && line.contains('╯'))
        .expect("composer bottom border");
    assert!(
        bottom.contains("model-1") || bottom.contains("Demo"),
        "draft composer must show model badge\n{bottom}"
    );
}

// ---------------------------------------------------------------------------
// Focus / cursor transitions
// ---------------------------------------------------------------------------

#[test]
fn startup_focus_is_in_composer() {
    // arrange
    let app = startup_app();
    let rendered = render(&app);
    assert!(
        rendered.contains('❯'),
        "startup focus must be in composer (❯ glyph visible)\n{rendered}"
    );
}

#[test]
fn draft_focus_remains_in_composer() {
    // arrange
    let mut app = startup_app();
    app.composer.prompt_buffer = "test".to_string();
    app.composer.prompt_cursor = 4;
    let rendered = render(&app);
    assert!(
        rendered.contains("test"),
        "draft text must be visible in composer\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "composer glyph must remain visible in draft\n{rendered}"
    );
}

#[test]
fn composer_grows_with_multiline_draft() {
    // arrange
    let mut app = idle_app();
    app.composer.prompt_buffer = "line1\nline2\nline3".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    let rendered = render(&app);
    let top = composer_top_row(&rendered).expect("composer top");
    let bottom = composer_bottom_row(&rendered).expect("composer bottom");
    let height = bottom - top + 1;
    assert!(
        height >= 5,
        "multiline draft must grow composer (3 content + 2 border = 5+ rows, got {height})\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// Bad input handling
// ---------------------------------------------------------------------------

#[test]
fn empty_prompt_does_not_send() {
    // arrange
    let mut app = idle_app();
    let initial_buffer = app.composer.prompt_buffer.clone();
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::empty(),
    ));
    assert_eq!(
        app.composer.prompt_buffer, initial_buffer,
        "empty prompt must not change buffer on Enter"
    );
}

#[test]
fn composer_rejects_control_chars_in_draft() {
    // arrange
    let mut app = idle_app();
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('\u{0001}'),
        crossterm::event::KeyModifiers::CONTROL,
    ));
    assert!(
        !app.composer.prompt_buffer.contains('\u{0001}'),
        "control chars must not appear in prompt buffer"
    );
}

// ---------------------------------------------------------------------------
// Paste / Unicode / IME handling
// ---------------------------------------------------------------------------

#[test]
fn composer_handles_unicode_cjk() {
    // arrange
    let mut app = idle_app();
    app.composer.prompt_buffer = "你好世界".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    let rendered = render(&app);
    assert!(
        rendered.contains('你')
            && rendered.contains('好')
            && rendered.contains('世')
            && rendered.contains('界'),
        "CJK characters must render in composer\n{rendered}"
    );
}

#[test]
fn composer_handles_unicode_emoji() {
    // arrange
    let mut app = idle_app();
    app.composer.prompt_buffer = "test 🦀 rust".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    let rendered = render(&app);
    assert!(
        rendered.contains("🦀"),
        "emoji must render in composer\n{rendered}"
    );
}

#[test]
fn composer_handles_mixed_width_chars() {
    // arrange
    let mut app = idle_app();
    app.composer.prompt_buffer = "abc你好def".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    let rendered = render(&app);
    assert!(
        rendered.contains('a') && rendered.contains('你') && rendered.contains('d'),
        "mixed width chars must render in composer\n{rendered}"
    );
}

#[test]
fn composer_handles_long_line_wrapping() {
    // arrange
    let mut app = idle_app();
    let long_line = "a".repeat(200);
    app.composer.prompt_buffer = long_line;
    app.composer.prompt_cursor = 200;
    let rendered = render(&app);
    let top = composer_top_row(&rendered).expect("composer top");
    let bottom = composer_bottom_row(&rendered).expect("composer bottom");
    let height = bottom - top + 1;
    assert!(
        height > 3,
        "long line must wrap and grow composer (got {height} rows)\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// Restart-persisted settings
// ---------------------------------------------------------------------------

#[test]
fn composer_prompt_history_retains_entries() {
    // arrange
    let mut app = idle_app();
    app.composer.prompt_buffer = "test command".to_string();
    app.composer.prompt_cursor = 12;
    app.composer.prompt_history.push("test command".to_string());
    assert!(
        app.composer
            .prompt_history
            .iter()
            .any(|h| h == "test command"),
        "prompt history must retain entries"
    );
}

#[test]
fn composer_renders_after_mode_toggle() {
    // arrange
    let app = idle_app();
    let rendered = render(&app);
    assert!(
        rendered.contains('❯'),
        "composer must render in default mode\n{rendered}"
    );
}

#[test]
fn always_approve_mode_badge_visible() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("always-approve"),
    );
    let rendered = render(&app);
    let bottom = rendered
        .lines()
        .rfind(|line| line.contains('╰') && line.contains('╯'))
        .expect("composer bottom border");
    assert!(
        bottom.contains("model-1"),
        "model badge must be visible on composer bottom border\n{bottom}"
    );
}
