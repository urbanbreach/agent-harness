//! Exact-clock production-renderer evidence, independent of runtime PTY timing.
use std::{fs, path::Path, time::Duration};

use harness_tui::{
    app::AppState,
    theme::{ColorLevel, GlyphMode},
    theme_family::{serialize_choice, ThemeChoice},
    ui::render_app,
    UnwrapOrAbort,
};
use ratatui::{
    backend::{CrosstermBackend, TestBackend},
    buffer::Buffer,
    layout::Rect,
    Terminal, TerminalOptions, Viewport,
};

const SIZES: [(u16, u16); 8] = [
    (80, 24),
    (120, 40),
    (160, 50),
    (89, 32),
    (90, 32),
    (90, 24),
    (120, 32),
    (200, 60),
];
const TIMES: [u64; 5] = [0, 100, 300, 1300, 4000];

#[test]
fn welcome_geometry_and_animation_use_immediate_controls_and_exact_clock() {
    for (profile, color, glyphs, choice) in [
        (
            "dark",
            ColorLevel::TrueColor,
            GlyphMode::Preferred,
            ThemeChoice::Dark,
        ),
        (
            "light",
            ColorLevel::TrueColor,
            GlyphMode::Preferred,
            ThemeChoice::Light,
        ),
        (
            "basic",
            ColorLevel::Basic,
            GlyphMode::Preferred,
            ThemeChoice::Dark,
        ),
        (
            "ascii",
            ColorLevel::None,
            GlyphMode::Ascii,
            ThemeChoice::Dark,
        ),
    ] {
        for (width, height) in SIZES {
            for reduced in [false, true] {
                verify_welcome_case(profile, color, glyphs, choice, width, height, reduced);
            }
        }
    }
}

fn verify_welcome_case(
    profile: &str,
    color: ColorLevel,
    glyphs: GlyphMode,
    choice: ThemeChoice,
    width: u16,
    height: u16,
    reduced: bool,
) {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.restore_theme_choice(&serialize_choice(choice).unwrap_or_abort())
        .unwrap_or_abort();
    app.set_startup_logo_capabilities_for_evidence(color, glyphs);
    app.set_reduced_motion_for_evidence(reduced);
    app.restart_motion_epoch_for_evidence();
    let first = render(&app, width, height);
    let first_text = text(&first);
    let mut changed = false;
    let mut previous_time = 0;
    for milliseconds in TIMES {
        app.advance_wall_clock_for_motion_evidence(Duration::from_millis(
            milliseconds - previous_time,
        ));
        previous_time = milliseconds;
        let frame = render(&app, width, height);
        let text = text(&frame);
        assert_eq!(
            text, first_text,
            "welcome content changed at {width}x{height}, t={milliseconds}"
        );
        for control in ["New worktree", "Resume session", "Changelog", "Quit"] {
            assert!(
                text.contains(control),
                "missing {control} at {width}x{height}: {text}"
            );
        }
        assert_eq!(
            text.matches(&format!("Harness {}", env!("CARGO_PKG_VERSION")))
                .count(),
            1,
            "identity duplication: {text}"
        );
        changed |= frame != first;
        if reduced {
            assert_eq!(frame, first, "reduced motion changed cells");
        }
        persist_if_requested(&app, width, height, milliseconds, reduced, profile);
    }
    if !reduced && color == ColorLevel::TrueColor && text(&first).contains('█') {
        assert!(changed, "visible H must shimmer at {width}x{height}");
    }
}

fn render(app: &AppState, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap_or_abort();
    terminal
        .draw(|frame| render_app(frame, app))
        .unwrap_or_abort();
    terminal.backend().buffer().clone()
}

fn text(buffer: &Buffer) -> String {
    buffer
        .content
        .chunks(usize::from(buffer.area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn persist_if_requested(
    app: &AppState,
    width: u16,
    height: u16,
    milliseconds: u64,
    reduced: bool,
    profile: &str,
) {
    let Some(directory) = std::env::var_os("HARNESS_PARITY_RENDER_ARTIFACT_DIR") else {
        return;
    };
    let directory = Path::new(&directory);
    fs::create_dir_all(directory).unwrap_or_abort();
    let name = format!(
        "welcome-{profile}-{width}x{height}-{}-{milliseconds}ms",
        if reduced { "reduced" } else { "motion" }
    );
    let mut bytes = Vec::new();
    {
        let backend = CrosstermBackend::new(&mut bytes);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, width, height)),
            },
        )
        .unwrap_or_abort();
        terminal
            .draw(|frame| render_app(frame, app))
            .unwrap_or_abort();
    }
    fs::write(directory.join(format!("{name}.ansi")), bytes).unwrap_or_abort();
    fs::write(
        directory.join(format!("{name}.txt")),
        text(&render(app, width, height)),
    )
    .unwrap_or_abort();
}
