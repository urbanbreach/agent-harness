//! First-slice structural parity (P0-START / P0-COMP / P0-KEY) against measured reference.
//!
//! Contract: docs/grok-build-tui-implementation-prompt.md + crates/harness-tui/DESIGN.md.
//! Identity glyphs/text may differ; geometry and choreography must match freeze anatomy.
//! Pixel dual-binary proof is a later gate; this suite locks the text-grid shell.

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

const W: u16 = 120;
const H: u16 = 32;
const DRAFT_TEXT: &str = "Browser QA draft";
const DRAFT_FOOTER: &str = "Enter:send  │  Shift+Tab:mode  │  Ctrl+x:shortcuts";

fn startup_app() -> AppState {
    let mut app = AppState::new_startup(Vec::new(), None);
    app.set_launch_metadata(
        LaunchMetadata::from_model_ref("worker", "mock:model-1").with_mode_label("Demo"),
    );
    app
}

fn render(app: &AppState) -> String {
    render_to_string(app, Rect::new(0, 0, W, H), |app, frame, _area| {
        ui::render_app(frame, app)
    })
}

fn line_with(rendered: &str, needle: &str) -> Option<String> {
    rendered
        .lines()
        .find(|line| line.contains(needle))
        .map(str::to_string)
}

/// P0-START-01 / P0-COMP-01: bordered welcome + bordered composer at startup.
#[test]
fn p0_start_01_startup_has_bordered_welcome_and_composer() {
    // arrange
    let app = startup_app();

    // act
    let rendered = render(&app);

    // assert — welcome panel uses rounded box-drawing (not centered full-width block logo alone)
    assert!(
        rendered.contains('╭') && rendered.contains('╰'),
        "P0-START-01: must paint rounded borders\n{rendered}"
    );
    assert!(
        rendered.contains("New worktree")
            || rendered.contains("New session")
            || rendered.contains("Resume session")
            || rendered.contains("Resume"),
        "P0-START-01: welcome must expose action rows\n{rendered}"
    );
    assert!(
        rendered.contains("Changelog") || rendered.contains("changelog"),
        "P0-START-01: welcome must expose changelog section or action\n{rendered}"
    );
    assert!(
        rendered.contains("Quit") || rendered.contains("quit"),
        "P0-START-01: welcome must expose quit action\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "P0-COMP-01: composer must use ❯ glyph\n{rendered}"
    );
    // Model badge / identity on composer bottom border region (not unboxed metadata-only)
    let has_model_signal = rendered.contains("mock")
        || rendered.contains("model")
        || rendered.contains("worker")
        || rendered.contains("Demo");
    assert!(
        has_model_signal,
        "P0-COMP-01: composer chrome should surface model/session identity\n{rendered}"
    );
    // Must not keep invalid DIV-004-only surface: huge centered HARNESS block without welcome box
    let welcome_border_rows = rendered
        .lines()
        .filter(|line| line.contains('╭') || line.contains('╰'))
        .count();
    assert!(
        welcome_border_rows >= 2,
        "P0-START-01: need at least welcome+composer border pairs, got {welcome_border_rows}\n{rendered}"
    );
    assert!(
        !rendered.to_ascii_lowercase().contains("grok"),
        "identity: must not render the external product name\n{rendered}"
    );

    let action_line = rendered
        .lines()
        .find(|line| line.contains("New worktree") || line.contains("New session"))
        .expect("welcome action row");
    let after_border = action_line
        .split_once('│')
        .map(|(_, rest)| rest)
        .expect("welcome action row must sit inside bordered panel");
    let leading = after_border.len() - after_border.trim_start_matches(' ').len();
    assert_eq!(
        leading, 19,
        "P0-START-01: freeze run1-startup action indent is 19 spaces after border (got {leading})\n{action_line}"
    );

    let bare = render(&AppState::new_startup(Vec::new(), None));
    let bare_bottom = bare
        .lines()
        .rfind(|line| line.contains('╰') && line.contains('╯'))
        .expect("composer bottom border");
    assert!(
        !bare_bottom.contains("unknown")
            && !bare_bottom.contains("model")
            && !bare_bottom.contains("mock")
            && !bare_bottom.contains("Demo"),
        "P0-START-01: freeze startup composer badge is empty (title_bottom \"  ─\")\n{bare_bottom}"
    );
}

/// P0-START-03: typing clears welcome; composer retains draft; footer switches grammar.
#[test]
fn p0_start_03_draft_clears_welcome_and_keeps_composer() {
    // arrange
    let mut app = startup_app();
    app.composer.prompt_buffer = DRAFT_TEXT.to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();

    // act
    let rendered = render(&app);

    // assert — draft retained
    assert!(
        rendered.contains("Browser QA draft"),
        "P0-START-03: composer must retain draft\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "P0-START-03: composer glyph retained\n{rendered}"
    );
    // Welcome mass cleared: no action rows / no big logo block
    assert!(
        !rendered.contains("New worktree") && !rendered.contains("New session"),
        "P0-START-03: welcome actions must clear when draft non-empty\n{rendered}"
    );
    assert!(
        !rendered.contains("██╗  ██╗"),
        "P0-START-03: centered HARNESS block logo must not remain while typing\n{rendered}"
    );
    let draft_bottom = rendered
        .lines()
        .rfind(|line| line.contains('╰') && line.contains('╯'))
        .expect("composer bottom border");
    assert!(
        draft_bottom.contains("unknown")
            || draft_bottom.contains("model")
            || draft_bottom.contains("mock")
            || draft_bottom.contains("Demo")
            || draft_bottom.contains("worker"),
        "P0-START-03: freeze draft composer shows model badge (not empty startup badge)\n{draft_bottom}"
    );

    // Footer shortcut grammar (reference measured)
    let footerish = rendered
        .lines()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        footerish.contains("Enter:send")
            || footerish.contains("Enter: send")
            || rendered.contains("Enter:send"),
        "P0-KEY-01: draft footer must use Enter:send grammar\n{rendered}"
    );
}

/// P0-COMP-01: single-line composer is a 3-row bordered strip (top/content/bottom).
#[test]
fn p0_comp_01_composer_is_three_row_bordered_strip() {
    // arrange
    let app = startup_app();

    // act
    let rendered = render(&app);
    let lines: Vec<&str> = rendered.lines().collect();
    let glyph_idx = lines
        .iter()
        .rposition(|line| line.contains('│') && line.contains('❯'))
        .or_else(|| lines.iter().rposition(|line| line.contains('❯')))
        .expect("composer glyph row");

    // assert — rounded borders immediately above/below content row when possible
    let above = glyph_idx.checked_sub(1).map(|i| lines[i]).unwrap_or("");
    let below = lines.get(glyph_idx + 1).copied().unwrap_or("");
    assert!(
        above.contains('╭') || above.contains('─'),
        "P0-COMP-01: row above ❯ should be top border\nabove={above:?}\n{rendered}"
    );
    assert!(
        below.contains('╰') || below.contains('─'),
        "P0-COMP-01: row below ❯ should be bottom border\nbelow={below:?}\n{rendered}"
    );
}

/// Breadcrumb + reference-shaped top spacing (P0-START-02).
#[test]
fn p0_start_02_top_chrome_not_only_bottom_path_bar() {
    // arrange
    // act
    // assert
    let app = startup_app();
    let rendered = render(&app);
    let top: String = rendered.lines().take(8).collect::<Vec<_>>().join("\n");
    let has_top_context = top.contains('')
        || top.contains("ui-ux")
        || top.contains("agent-harness")
        || top.contains("Projects");
    assert!(
        has_top_context || rendered.contains('╭'),
        "P0-START-02: top breadcrumb or welcome chrome required\n{rendered}"
    );
    assert!(
        !rendered.contains("Clipboard may be unreachable.") && !rendered.contains("terminal-setup"),
        "P0-START-02: startup chrome must not insert non-reference warning rows\n{rendered}"
    );
    let lines: Vec<&str> = rendered.lines().collect();
    let welcome_top = lines
        .iter()
        .position(|line| line.contains('╭') && line.contains('─'))
        .expect("welcome top border");
    assert_eq!(
        welcome_top, 4,
        "P0-START-02: welcome panel top must match the 120x32 reference\n{rendered}"
    );
    let _ = line_with(&rendered, "❯");
}

/// P0-KEY-01: the draft footer keeps the current Grok shortcut grammar.
#[test]
fn p0_key_01_draft_footer_matches_reference_grammar() {
    // arrange
    let mut app = startup_app();
    app.composer.prompt_buffer = DRAFT_TEXT.to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();

    // act
    let rendered = render(&app);

    // assert
    assert!(
        rendered.contains(DRAFT_FOOTER),
        "P0-KEY-01: draft footer must retain the current shortcut grammar\n{rendered}"
    );
}
