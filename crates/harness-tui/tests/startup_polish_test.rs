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
fn compact_startup_reveals_mark_then_identity_before_affordances() {
    // arrange: compact width so the welcome renders without the wide panel.
    let mut app = AppState::new_startup(Vec::new(), None);

    // act: sample the compact reveal at each staged delay.
    let mark = startup_text(&app, 80, 24);

    // assert: Mark stage paints the wordmark, not the identity or affordances.
    assert!(
        mark.contains("Harness"),
        "compact mark stage must paint the wordmark\n{mark}"
    );
    assert!(
        !mark.contains(env!("CARGO_PKG_VERSION")),
        "compact mark stage must not paint the version\n{mark}"
    );
    assert!(
        !mark.contains("New worktree"),
        "compact mark stage must not paint affordances\n{mark}"
    );

    // act: advance to the Identity stage.
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(100));
    let identity = startup_text(&app, 80, 24);

    // assert: identity joins the wordmark, affordances stay hidden.
    assert!(
        identity.contains(env!("CARGO_PKG_VERSION"))
            && identity.contains("Thanks for trying Harness"),
        "compact identity stage must paint version and welcome copy\n{identity}"
    );
    assert!(
        !identity.contains("New worktree"),
        "compact identity stage must not paint affordances\n{identity}"
    );

    // act: advance to the Affordances stage.
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(100));
    let affordances = startup_text(&app, 80, 24);

    // assert: affordances join, changelog stays hidden.
    assert!(
        affordances.contains("New worktree") && affordances.contains("Resume session"),
        "compact affordance stage must paint the actions\n{affordances}"
    );
    assert!(
        !affordances.contains("Subagent spawning"),
        "compact affordance stage must not paint the changelog\n{affordances}"
    );

    // act: advance to the Complete stage.
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(100));
    let complete = startup_text(&app, 80, 24);

    // assert: the changelog settles the compact welcome within its row budget.
    assert!(
        complete.contains("Changelog") && complete.contains("Subagent spawning"),
        "compact complete stage must paint the changelog\n{complete}"
    );
    assert!(
        !complete.contains("██╗  ██╗"),
        "compact complete stage must stay logo-free\n{complete}"
    );
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
fn startup_welcome_reveals_mark_then_name_then_affordances_then_changelog() {
    // arrange
    // Given: a visible wide startup welcome at first paint.
    let mut app = AppState::new_startup(Vec::new(), None);

    // act
    // Mark stage at first paint — logo only, no identity, no affordances.
    let mark = startup_text(&app, 120, 32);
    // assert
    assert!(
        mark.contains("██╗"),
        "mark stage must paint the Harness mark\n{mark}"
    );
    assert!(
        !mark.contains("Thanks for trying Harness"),
        "mark stage must not paint welcome copy\n{mark}"
    );
    assert!(
        !mark.contains("New worktree"),
        "mark stage must not paint first-input affordances\n{mark}"
    );

    // Name stage — product identity and welcome copy join the mark.
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(100));
    let name = startup_text(&app, 120, 32);
    assert!(
        name.contains("Thanks for trying Harness"),
        "name stage must paint welcome copy\n{name}"
    );
    assert!(
        !name.contains("New worktree"),
        "name stage must not paint first-input affordances\n{name}"
    );

    // Affordances stage — first-input affordances join the identity.
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(100));
    let affordances = startup_text(&app, 120, 32);
    assert!(
        affordances.contains("New worktree") && affordances.contains("Resume session"),
        "affordance stage must paint first-input affordances\n{affordances}"
    );
    assert!(
        !affordances.contains("Subagent spawning"),
        "affordance stage must not paint the expanded changelog\n{affordances}"
    );

    // Complete stage — changelog expansion settles the welcome.
    app.advance_wall_clock_for_motion_evidence(Duration::from_millis(100));
    let complete = startup_text(&app, 120, 32);
    assert!(
        complete.contains("Changelog") && complete.contains("Subagent spawning"),
        "complete stage must paint the expanded changelog\n{complete}"
    );
    assert!(
        app.motion_plan_for_evidence().is_none(),
        "settled reveal must not keep a decorative deadline armed"
    );
}

#[test]
fn reduced_motion_freezes_startup_reveal_on_the_final_frame() {
    // arrange
    // Given: reduced motion active before the first startup paint.
    let mut app = AppState::new_startup(Vec::new(), None);
    app.set_reduced_motion_for_evidence(true);

    // act
    let frozen = startup_text(&app, 120, 32);

    // assert
    assert!(
        frozen.contains("██╗"),
        "reduced motion must still paint the Harness mark\n{frozen}"
    );
    assert!(
        frozen.contains("Thanks for trying Harness") || frozen.contains("Changelog"),
        "reduced motion must freeze on the final reveal frame\n{frozen}"
    );
    assert!(
        frozen.contains("New worktree"),
        "reduced motion must show first-input affordances immediately\n{frozen}"
    );
    assert!(
        frozen.contains("Changelog") && frozen.contains("Subagent spawning"),
        "reduced motion must freeze on the expanded final frame\n{frozen}"
    );
    assert!(
        app.motion_plan_for_evidence().is_none(),
        "reduced motion must not arm a reveal deadline"
    );
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
fn startup_reveal_frame_sequence_snapshots_wide_and_compact() {
    // arrange: the staged reveal sampled at each stage boundary in both geometries.
    let geometries: [(&str, u16, u16); 2] = [("wide", 120, 32), ("compact", 80, 24)];

    for (geometry_name, width, height) in geometries {
        let mut app = AppState::new_startup(Vec::new(), None);
        // Pin the motion epoch to construction time so real scheduling delay
        // between app creation and the staged advances cannot leak into the
        // sampled reveal stage under a loaded parallel test run.
        app.restart_motion_epoch_for_evidence();
        for (stage_index, stage_name) in ["mark", "identity", "affordances", "complete"]
            .into_iter()
            .enumerate()
        {
            // act
            if stage_index > 0 {
                app.advance_wall_clock_for_motion_evidence(Duration::from_millis(100));
            }
            let snapshot = normalized_startup_snapshot(&app, width, height);

            // assert
            insta::assert_snapshot!(
                format!("startup_reveal_{geometry_name}_{stage_name}"),
                snapshot
            );
        }
    }
}

#[test]
fn reduced_motion_startup_snapshots_freeze_the_complete_frame() {
    // arrange: reduced motion active before the first startup paint in both geometries.
    let geometries: [(&str, u16, u16); 2] = [("wide", 120, 32), ("compact", 80, 24)];

    for (geometry_name, width, height) in geometries {
        let mut app = AppState::new_startup(Vec::new(), None);
        app.set_reduced_motion_for_evidence(true);

        // act
        let frozen = normalized_startup_snapshot(&app, width, height);

        // assert
        insta::assert_snapshot!(format!("startup_reduced_motion_{geometry_name}"), frozen);
    }
}

#[test]
fn reduced_motion_freeze_equals_the_full_motion_complete_frame() {
    // arrange: reduced motion freezes instantly while full motion settles after the cadence.
    let mut reduced = AppState::new_startup(Vec::new(), None);
    reduced.set_reduced_motion_for_evidence(true);
    let mut full = AppState::new_startup(Vec::new(), None);
    full.advance_wall_clock_for_motion_evidence(Duration::from_millis(300));

    for (width, height) in [(120u16, 32u16), (80, 24)] {
        // act
        let frozen = normalized_startup_snapshot(&reduced, width, height);
        let settled = normalized_startup_snapshot(&full, width, height);

        // assert
        assert_eq!(
            frozen, settled,
            "reduced motion must freeze on the exact complete frame at {width}x{height}"
        );
    }
}

#[test]
fn startup_input_is_never_blocked_by_the_reveal() {
    // arrange
    // Given: a welcome still in its Mark reveal stage.
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
        "typing during the reveal must reach the composer\n{rendered}"
    );
    assert!(
        !rendered.contains("New worktree"),
        "typing during the reveal must dismiss the welcome affordances\n{rendered}"
    );
}

#[test]
fn startup_welcome_settles_and_parks_after_expansion() {
    // arrange
    // Given: a visible startup welcome before its baseline expansion tick.
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
