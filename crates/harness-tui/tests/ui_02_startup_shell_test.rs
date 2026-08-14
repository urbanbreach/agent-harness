#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "rendered shell contract tests use fail-fast assertions"
)]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::app::{AppState, LaunchMetadata};
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use harness_tui::welcome_surface::WelcomeLayout;
use harness_tui::FrameLayoutPlan;
use ratatui::{backend::TestBackend, layout::Rect, style::Modifier, Terminal};

fn startup_app() -> AppState {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );
    app
}

fn startup_app_with_clipboard_warning() -> AppState {
    let mut app = startup_app();
    app.status_banner = Some("clipboard is unreachable".to_string());
    app
}

fn render(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn render_terminal(app: &AppState, width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("startup test terminal");
    terminal
        .draw(|frame| ui::render_app(frame, app))
        .expect("startup render");
    terminal
}

#[test]
fn primary_startup_uses_measured_vertical_order_at_120x32() {
    let rendered = render(&startup_app_with_clipboard_warning(), 120, 32);
    let lines = rendered.lines().collect::<Vec<_>>();

    let breadcrumb = lines.iter().position(|line| line.contains(''));
    let warning = lines
        .iter()
        .position(|line| line.contains("Clipboard may be unreachable."));
    let welcome_top = lines
        .iter()
        .position(|line| line.contains('╭') && line.starts_with("   "));
    let composer = lines.iter().rposition(|line| line.contains("│ ❯"));
    let footer = lines
        .iter()
        .position(|line| line.contains("Provider configured"));

    assert_eq!(
        (breadcrumb, warning, welcome_top, composer, footer),
        (Some(1), Some(4), Some(8), Some(29), Some(31)),
        "startup rows must follow breadcrumb, warning, welcome, composer, footer\n{rendered}"
    );
}

#[test]
fn home_top_bar_and_hero_topology_hold_at_80_90_100_and_120_columns() {
    for (width, height, welcome_boxes) in [(80, 24, 1), (90, 24, 2), (100, 30, 2), (120, 32, 2)] {
        let rendered = render(&startup_app(), width, height);
        let lines = rendered.lines().collect::<Vec<_>>();
        let breadcrumb = lines.iter().position(|line| line.contains(''));

        assert_eq!(
            breadcrumb,
            Some(1),
            "the one-row startup top bar must stay on row 2 at {width}x{height}\n{rendered}"
        );
        assert_eq!(
            rendered.matches('╭').count(),
            welcome_boxes,
            "80 columns keeps only the composer box; 90+ keeps welcome plus composer at {width}x{height}\n{rendered}"
        );
        assert!(
            rendered.contains("New worktree") && rendered.contains("Changelog"),
            "startup actions and changelog must remain reachable at {width}x{height}\n{rendered}"
        );
        assert_eq!(
            rendered.contains("██╗"),
            width >= 90,
            "the two-column logo panel starts at 90 columns at {width}x{height}\n{rendered}"
        );
    }
}

#[test]
fn focused_home_action_uses_the_full_semantic_selection_row_at_all_breakpoints() {
    for (width, height) in [(80, 24), (90, 24), (100, 30), (120, 32)] {
        let mut app = startup_app();
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let frame_area = Rect::new(0, 0, width, height);
        let startup_area = FrameLayoutPlan::for_app(&app, frame_area)
            .transcript
            .expect("startup transcript area");
        let layout = WelcomeLayout::for_area(
            (
                startup_area.x,
                startup_area.y,
                startup_area.width,
                startup_area.height,
            ),
            false,
        );

        let terminal = render_terminal(&app, width, height);
        let action = layout.action_rects[0];
        for column in action.0..action.0.saturating_add(action.2) {
            assert_eq!(
                terminal.backend().buffer()[(column, action.1)].bg,
                app.theme().surface.selected_card,
                "focused action row must use the semantic selected surface at {width}x{height}, column {column}"
            );
        }
    }
}

#[test]
fn primary_startup_keeps_harness_identity_in_reference_region_without_dead_action() {
    let rendered = render(&startup_app_with_clipboard_warning(), 120, 32);
    let title = rendered
        .lines()
        .find(|line| line.contains("Harness"))
        .expect("Harness title row");
    let title_column = title.chars().position(|character| character == 'H');

    assert_eq!(
        (title_column, rendered.matches("Changelog").count()),
        (Some(23), 1),
        "Harness identity must use the measured title column and Changelog must remain a section, not a dead action\n{rendered}"
    );
}

#[test]
fn primary_startup_renders_changelog_as_a_bold_section_label() {
    let app = startup_app_with_clipboard_warning();
    let terminal = render_terminal(&app, 120, 32);
    let rendered = render(&app, 120, 32);
    let (row, column) = rendered
        .lines()
        .enumerate()
        .find_map(|(row, line)| {
            line.contains("Changelog").then(|| {
                let column = line
                    .chars()
                    .position(|character| character == 'C')
                    .expect("Changelog starts with C");
                (row, column)
            })
        })
        .expect("Changelog section label");
    let row = u16::try_from(row).expect("render row fits terminal coordinates");
    let column = u16::try_from(column).expect("render column fits terminal coordinates");

    assert!(
        terminal.backend().buffer()[(column, row)]
            .modifier
            .contains(Modifier::BOLD),
        "Changelog must use the measured bold section-label hierarchy\n{rendered}"
    );
}

#[test]
fn compact_startup_collapses_welcome_chrome_but_keeps_bordered_composer() {
    let rendered = render(&startup_app(), 80, 24);
    let lines = rendered.lines().collect::<Vec<_>>();
    let composer_row = lines
        .iter()
        .position(|line| line.contains("│ ❯"))
        .expect("compact composer row");

    assert!(
        lines
            .get(composer_row.saturating_sub(1))
            .is_some_and(|line| line.contains('╭'))
            && lines
                .get(composer_row.saturating_add(1))
                .is_some_and(|line| line.contains('╰'))
            && !rendered.contains("██╗"),
        "compact startup must unbox welcome content without unboxing the composer\n{rendered}"
    );
}

#[test]
fn disconnected_startup_keeps_recovery_status_in_footer_without_panel_notice() {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.maybe_set_no_provider_banner();

    let rendered = render(&app, 80, 24);
    assert!(
        !rendered.contains("No provider connected. Use /connect.")
            && !rendered.contains("Notices:")
            && rendered.contains("Provider not connected")
            && !rendered.contains("Logged in with API key"),
        "disconnected startup must reserve welcome content for the measured anatomy and keep status truthful in the footer\n{rendered}"
    );
}

#[test]
fn first_typed_grapheme_clears_only_welcome_content() {
    let mut app = startup_app_with_clipboard_warning();

    app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));

    let rendered = render(&app, 120, 32);
    assert!(
        rendered.contains('')
            && rendered.contains("│ ❯ B")
            && rendered.contains("Enter:send")
            && !rendered.contains("Clipboard may be unreachable.")
            && !rendered.contains("New worktree")
            && !rendered.contains("██╗"),
        "typing must retain breadcrumb, composer, draft, and footer while clearing welcome-only content\n{rendered}"
    );
}

#[test]
fn erased_first_grapheme_keeps_welcome_dismissed() {
    let mut app = startup_app_with_clipboard_warning();
    app.handle_key(KeyEvent::new(KeyCode::Char('é'), KeyModifiers::NONE));

    app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));

    let rendered = render(&app, 120, 32);
    assert!(
        rendered.contains("│ ❯")
            && rendered.contains("Enter:send")
            && !rendered.contains("New worktree")
            && !rendered.contains("Clipboard may be unreachable."),
        "clearing the first draft must not resurrect welcome-only content\n{rendered}"
    );
}

#[test]
fn startup_composer_is_focused_and_has_no_placeholder() {
    let rendered = render(&startup_app(), 120, 32);

    assert!(
        rendered.contains("│ ❯") && !rendered.contains("Build anything"),
        "startup must render the focused empty composer rather than inactive placeholder copy\n{rendered}"
    );
}

#[test]
fn new_worktree_dialog_preserves_welcome_underlay() {
    let mut app = startup_app_with_clipboard_warning();
    app.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));

    let rendered = render(&app, 120, 32);
    assert!(
        rendered.contains("Create worktree")
            && rendered.contains("Harness")
            && rendered.contains("Changelog"),
        "the worktree dialog must overlay, not remove, the startup shell\n{rendered}"
    );
}

#[test]
fn startup_responsive_matrix_stays_inside_every_contract_viewport() {
    for (width, height) in [
        (120, 50),
        (120, 40),
        (100, 30),
        (80, 24),
        (79, 24),
        (60, 20),
        (140, 40),
    ] {
        let rendered = render(&startup_app(), width, height);
        let lines = rendered.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), usize::from(height));
        assert!(lines
            .iter()
            .all(|line| line.chars().count() == usize::from(width)));
        assert!(rendered.contains('❯'));
        assert!(rendered.contains("New worktree"));
    }
}

#[test]
fn keyboard_focus_marks_the_selected_startup_action() {
    let mut app = startup_app();
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert!(render(&app, 120, 32).contains("›New worktree"));

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(render(&app, 120, 32).contains("›Resume session"));
}
