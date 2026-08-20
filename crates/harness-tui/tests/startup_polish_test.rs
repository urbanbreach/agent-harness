use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::app::{AppState, LaunchMetadata};
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
fn compact_startup_omits_the_wide_identity_logo_at_every_height() {
    // arrange
    // act
    let app = AppState::new_startup(Vec::new(), None);
    let short = startup_text(&app, 80, 24);
    let medium = startup_text(&app, 80, 32);
    let tall = startup_text(&app, 80, 40);

    // assert
    assert!(!short.contains("██╗  ██╗"), "{short}");
    assert!(!medium.contains("██╗  ██╗"), "{medium}");
    assert!(!tall.contains("██╗  ██╗"), "{tall}");
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
fn startup_identity_field_geometry_does_not_change_live_composer_or_footer_copy() {
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
    assert!(startup.contains("                             model-1 · Demo mode"));
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
    assert_eq!(dismissed, MotionPlan::none());
}

#[test]
fn startup_welcome_matches_reference_rest_and_expanded_geometry() {
    // arrange
    let mut app = AppState::new_startup(Vec::new(), None);

    // act
    let rest = startup_text(&app, 100, 30);
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(300));
    let expanded = startup_text(&app, 100, 30);

    // assert
    assert_eq!(rounded_panel_rows(&rest, 3), vec![5, 15], "{rest}");
    assert_eq!(rounded_panel_rows(&expanded, 3), vec![4, 19], "{expanded}");
    assert_eq!(
        rest.lines()
            .filter(|line| {
                ["New worktree", "Resume session", "Changelog", "Quit"]
                    .iter()
                    .any(|label| line.contains(label))
            })
            .count(),
        4,
        "rest menu rows:\n{rest}"
    );
    assert_eq!(
        expanded
            .lines()
            .filter(|line| {
                ["New worktree", "Resume session", "Quit"]
                    .iter()
                    .any(|label| line.contains(label))
            })
            .count(),
        3,
        "expanded action rows:\n{expanded}"
    );
}

#[test]
fn startup_logo_stays_on_one_color_during_welcome_expansion() {
    // arrange
    let mut app = AppState::new_startup(Vec::new(), None);
    let initial = startup_logo_colors(&app, 120, 32);
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(300));

    // act
    let colors = startup_logo_colors(&app, 120, 32);

    // assert
    assert!(initial.contains(&colors[0]), "rest colors: {initial:?}");
    assert_eq!(colors.len(), 1, "startup identity colors: {colors:?}");
}

#[test]
fn startup_welcome_settles_and_parks_after_expansion() {
    // arrange
    // Given: a visible startup welcome before its reference expansion tick.
    let mut app = AppState::new_startup(Vec::new(), None);

    // When: the motion clock reaches the first expanded frame.
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(300));

    // act
    // Then: the renderer has no decorative redraw deadline left to serve.
    // assert
    assert!(app.motion_plan_for_evidence().is_none());
}

#[test]
fn startup_logo_capability_matrix_preserves_static_identity_semantics() {
    // arrange
    for color_level in [
        ColorLevel::TrueColor,
        ColorLevel::Ansi256,
        ColorLevel::Basic,
        ColorLevel::None,
    ] {
        let mut app = AppState::new_startup(Vec::new(), None);
        app.set_startup_logo_capabilities_for_evidence(color_level, GlyphMode::Preferred);

        // act
        let (initial_cells, initial_colors) = startup_logo_frame(&app, 120, 32);
        let initial_motion = app.motion_plan_for_evidence();
        app.advance_wall_clock_for_motion_evidence(Duration::from_millis(300));
        let (middle_cells, middle_colors) = startup_logo_frame(&app, 120, 32);

        // assert
        assert!(
            !initial_cells.is_empty(),
            "{color_level}: preferred logo missing"
        );
        assert_eq!(middle_colors, initial_colors, "{color_level}: color drift");
        assert_eq!(
            initial_motion.cadence(),
            MotionCadence::Slow(Duration::from_millis(83)),
            "{color_level}: visible welcome expansion must request the slow cadence"
        );
        assert_ne!(
            middle_cells, initial_cells,
            "{color_level}: expanded logo did not move with its panel"
        );
        assert!(
            app.motion_plan_for_evidence().is_none(),
            "{color_level}: settled sweep still armed"
        );
    }
}

#[test]
fn startup_logo_ascii_and_compact_widths_do_not_paint_logo_cells() {
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
        MotionCadence::Slow(Duration::from_millis(83))
    );
    assert!(compact_cells.is_empty());
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
