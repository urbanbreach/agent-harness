use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::app::{AppState, Focus, LaunchMetadata};
use harness_tui::scheduling::{MotionCadence, MotionPlan};
use harness_tui::theme::{ColorLevel, GlyphMode};
use harness_tui::welcome_surface::WelcomeLayout;
use harness_tui::{ui, FrameLayoutPlan, UnwrapOrAbort};
use ratatui::style::Color;
use ratatui::{backend::TestBackend, layout::Rect, Terminal};

fn startup_text(app: &AppState, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, app))
        .unwrap_or_abort();
    terminal
        .backend()
        .buffer()
        .content
        .chunks(usize::from(width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn rounded_panel_rows(rendered: &str, inset: usize) -> Vec<usize> {
    rendered
        .lines()
        .enumerate()
        .filter_map(|(row, line)| {
            line.chars()
                .nth(inset)
                .is_some_and(|glyph| matches!(glyph, '╭' | '╰'))
                .then_some(row)
        })
        .collect()
}

fn startup_logo_colors(app: &AppState, width: u16, height: u16) -> Vec<Color> {
    let frame_area = Rect::new(0, 0, width, height);
    let transcript = FrameLayoutPlan::for_app(app, frame_area)
        .transcript
        .unwrap_or_abort();
    let layout = WelcomeLayout::for_area(
        (
            transcript.x,
            transcript.y,
            transcript.width,
            transcript.height,
        ),
        false,
    );
    let (x, y, logo_width, logo_height) = layout.logo_rect;
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, app))
        .unwrap_or_abort();
    let buffer = terminal.backend().buffer();
    let mut colors = Vec::new();
    for row in y..y.saturating_add(logo_height) {
        for column in x..x.saturating_add(logo_width) {
            let cell = &buffer[(column, row)];
            if cell.symbol() != " " && !colors.contains(&cell.fg) {
                colors.push(cell.fg);
            }
        }
    }
    colors
}

fn startup_logo_frame(
    app: &AppState,
    width: u16,
    height: u16,
) -> (Vec<(u16, u16, String)>, Vec<Color>) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap_or_abort();
    terminal
        .draw(|frame| ui::render_app(frame, app))
        .unwrap_or_abort();
    let buffer = terminal.backend().buffer();
    let mut cells = Vec::new();
    let mut colors = Vec::new();
    for row in 0..height {
        for column in 0..width {
            let cell = &buffer[(column, row)];
            if matches!(cell.symbol(), "█" | "╗" | "║" | "╔" | "═" | "╝") {
                cells.push((column, row, cell.symbol().to_string()));
                if !colors.contains(&cell.fg) {
                    colors.push(cell.fg);
                }
            }
        }
    }
    (cells, colors)
}

#[test]
fn stacked_startup_uses_the_h_that_fits_the_available_height() {
    let app = AppState::new_startup(Vec::new(), None);
    for (height, full) in [(24, false), (32, true), (40, true)] {
        let rendered = startup_text(&app, 80, height);
        assert_eq!(rendered.contains("██╗  ██╗"), full, "{rendered}");
        assert_eq!(rendered.contains("██"), full, "{rendered}");
    }
}

#[test]
fn compact_startup_composer_is_flush_while_live_composer_keeps_its_inset() {
    // arrange
    let startup = AppState::new_startup(Vec::new(), None);
    let live = AppState::new_live(None, false, None);

    // act
    let startup_dock = FrameLayoutPlan::for_app(&startup, Rect::new(0, 0, 60, 20))
        .dock
        .unwrap_or_abort();
    let live_dock = FrameLayoutPlan::for_app(&live, Rect::new(0, 0, 60, 20))
        .dock
        .unwrap_or_abort();

    // assert
    assert_eq!((startup_dock.shell.x, startup_dock.shell.width), (0, 60));
    assert_eq!((live_dock.shell.x, live_dock.shell.width), (1, 58));
}

#[test]
fn startup_identity_sits_on_composer_border_without_padding_field() {
    // arrange
    let metadata = LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo");
    let mut startup = AppState::new_startup(Vec::new(), None);
    startup.set_launch_metadata(metadata.clone());
    let mut live = AppState::new_live(None, false, None);
    live.set_launch_metadata(metadata);

    // act
    let startup = startup_text(&startup, 100, 30);
    let live = startup_text(&live, 100, 30);

    // assert
    let badge_row = startup
        .lines()
        .find(|line| line.contains("model-1 · Demo mode"))
        .expect("startup composer shows model identity on the bottom border")
        .trim();
    assert!(
        badge_row.starts_with('╰')
            && badge_row.contains(" model-1 · Demo mode ")
            && badge_row.ends_with('╯'),
        "model label sits on the bottom border with one blank cell of padding on each side\n{badge_row}"
    );
    assert!(startup.contains("Logged in with API key"));
    assert!(!live.contains("model-1 · Demo mode"));
    assert!(!live.contains("Logged in with API key"));
}

#[test]
fn startup_welcome_requests_slow_motion_only_until_first_input() {
    // arrange
    let mut app = AppState::new_startup(Vec::new(), None);

    // act
    let visible = app.motion_plan_for_evidence();
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    let dismissed = app.motion_plan_for_evidence();

    // assert
    assert_eq!(
        visible.cadence(),
        MotionCadence::Slow(Duration::from_millis(83))
    );
    assert_eq!(dismissed.cadence(), MotionCadence::None);
    assert_eq!(dismissed.until(), Some(Duration::from_millis(100)));
}

fn normalized_startup_snapshot(app: &AppState, width: u16, height: u16) -> String {
    let rendered = startup_text(app, width, height);
    rendered
        .lines()
        .map(|line| {
            if line.contains("git:") {
                "  <cwd-breadcrumb>"
            } else {
                line.trim_end()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn startup_content_snapshots_wide_and_stacked() {
    for (geometry, width, height) in [("wide", 120, 32), ("stacked", 80, 24)] {
        let mut app = AppState::new_startup(Vec::new(), None);
        app.set_reduced_motion_for_evidence(true);
        insta::assert_snapshot!(
            format!("startup_content_{geometry}"),
            normalized_startup_snapshot(&app, width, height)
        );
    }
}

#[test]
fn startup_input_is_never_blocked_by_the_reveal() {
    // arrange
    // Given: a welcome at its first frame.
    let mut app = AppState::new_startup(Vec::new(), None);
    assert_eq!(app.focus, Focus::Prompt);

    // act
    app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    let rendered = startup_text(&app, 120, 32);

    // assert
    assert_eq!(app.composer.prompt_buffer, "x");
    assert_eq!(app.composer.prompt_cursor, 1);
    assert_eq!(app.focus, Focus::Prompt);
    assert!(
        rendered.contains('x'),
        "typing on the welcome must reach the composer\n{rendered}"
    );
    assert!(
        !rendered.contains("New worktree"),
        "typing on the welcome must dismiss the welcome affordances\n{rendered}"
    );
}

#[test]
fn startup_logo_capabilities_preserve_geometry_and_bounded_motion() {
    for color_level in [
        ColorLevel::TrueColor,
        ColorLevel::Ansi256,
        ColorLevel::Basic,
        ColorLevel::None,
    ] {
        let mut app = AppState::new_startup(Vec::new(), None);
        app.set_startup_logo_capabilities_for_evidence(color_level, GlyphMode::Preferred);
        app.restart_motion_epoch_for_evidence();
        let (initial_cells, _) = startup_logo_frame(&app, 120, 32);
        app.advance_wall_clock_for_motion_evidence(Duration::from_millis(4300));
        let (later_cells, _) = startup_logo_frame(&app, 120, 32);
        assert!(!initial_cells.is_empty(), "{color_level}");
        assert_eq!(
            later_cells, initial_cells,
            "motion must never move artwork or controls"
        );
        assert_eq!(
            app.motion_plan_for_evidence().cadence(),
            MotionCadence::Slow(Duration::from_millis(83))
        );
        app.set_reduced_motion_for_evidence(true);
        assert!(app.motion_plan_for_evidence().is_none());
    }
}

#[test]
fn startup_logo_is_hidden_for_ascii_and_retained_at_stacked_widths() {
    // arrange
    let mut ascii = AppState::new_startup(Vec::new(), None);
    ascii.set_startup_logo_capabilities_for_evidence(ColorLevel::TrueColor, GlyphMode::Ascii);
    let (ascii_cells, _) = startup_logo_frame(&ascii, 120, 32);

    // act
    let compact = AppState::new_startup(Vec::new(), None);
    let (compact_cells, _) = startup_logo_frame(&compact, 80, 32);
    let wide = AppState::new_startup(Vec::new(), None);
    let (wide_cells, _) = startup_logo_frame(&wide, 100, 30);

    // assert
    assert!(ascii_cells.is_empty());
    assert_eq!(
        ascii.motion_plan_for_evidence().cadence(),
        MotionCadence::None
    );
    assert!(!compact_cells.is_empty());
    assert!(!wide_cells.is_empty());
}

#[test]
fn reduced_motion_keeps_startup_logo_on_one_resting_color() {
    // arrange
    let mut app = AppState::new_startup(Vec::new(), None);
    app.set_reduced_motion_for_evidence(true);
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(640));

    // act
    let colors = startup_logo_colors(&app, 120, 32);

    // assert
    assert_eq!(colors.len(), 1, "reduced-motion logo colors: {colors:?}");
    assert!(app.motion_plan_for_evidence().is_none());
}

#[test]
fn startup_composer_footer_rhythm_is_pinned_across_viewports_and_variants() {
    // arrange
    for (variant, reduced_motion, draft) in [
        ("welcome", false, false),
        ("reduced-motion welcome", true, false),
        ("draft", false, true),
        ("reduced-motion draft", true, true),
    ] {
        for (width, height) in [(60, 20), (80, 24), (100, 30), (120, 32), (140, 40)] {
            let mut app = AppState::new_startup(Vec::new(), None);
            app.set_reduced_motion_for_evidence(reduced_motion);
            if draft {
                app.composer.prompt_buffer = "x".to_string();
                app.composer.prompt_cursor = 1;
            }

            // act
            let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, width, height));
            let composer = plan.composer.unwrap_or_abort();
            let rendered = startup_text(&app, width, height);
            let spacer_row = rendered
                .lines()
                .nth(usize::from(height.saturating_sub(3)))
                .unwrap_or_abort();

            // assert
            assert_eq!(
                composer.y.saturating_add(composer.height),
                height.saturating_sub(3),
                "{variant} at {width}x{height}: one-line composer must end three rows above bottom"
            );
            assert_eq!(
                plan.footer.y,
                height.saturating_sub(2),
                "{variant} at {width}x{height}: footer must begin two rows above bottom"
            );
            assert_eq!(
                plan.footer.height, 2,
                "{variant} at {width}x{height}: footer must reserve two rows"
            );
            assert_eq!(
                plan.footer.y,
                composer.y.saturating_add(composer.height).saturating_add(1),
                "{variant} at {width}x{height}: reserve exactly one blank spacer row"
            );
            assert!(
                spacer_row.trim().is_empty(),
                "{variant} at {width}x{height}: startup spacer row must stay blank: {spacer_row:?}"
            );
        }
    }
}
