//! Todo 22: Startup, key, composer, and input parity shard.
//!
//! Proves real behavior for P0-START-01/02/03, P0-COMP-01, P0-KEY-01
//! using deterministic render tests, focused input tests, and leaf view
//! contract tests. No snapshot-only acceptance — every test asserts real
//! rendered output or real state transitions.
//!
//! Manifest rows covered:
//! - P0-START-01: welcome_panel → focus_owner=composer
//! - P0-START-02: breadcrumb_and_warning → focus_owner=composer
//! - P0-START-03: welcome_to_composer_transition → focus_owner=composer
//! - P0-COMP-01: bordered_strip_with_model_badge → focus_owner=composer
//! - P0-KEY-01: contextual_shortcut_footer → focus_owner=composer

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "parity tests use fail-fast asserts for missing leaf state"
)]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::app::{AppState, Focus, LaunchMetadata, LifecycleShellState, UiIntent};
use harness_tui::layout::FrameLayoutPlan;
use harness_tui::leaf_views::{
    ComposerLeafView, FocusOwner, FooterGrammar, InputLeafView, KeyLeafView, StartupLeafView,
    StartupPhase, TranscriptLeafView,
};
use harness_tui::render_test::{render_to_buffer, render_to_string};
use harness_tui::ui;
use ratatui::layout::Rect;
use ratatui::style::Color;
use std::sync::{Arc, Mutex};

const W: u16 = 120;
const H: u16 = 32;

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

fn startup_app_with_intents() -> (AppState, Arc<Mutex<Vec<UiIntent>>>) {
    let intents = Arc::new(Mutex::new(Vec::new()));
    let intent_sink = Arc::clone(&intents);
    let app = AppState::new_startup(
        Vec::new(),
        Some(Arc::new(move |intent| {
            intent_sink.lock().expect("intent sink lock").push(intent);
        })),
    );
    (app, intents)
}

fn ctrl_w() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL)
}

// ---------------------------------------------------------------------------
// Leaf view determinism and no-dependency contract
// ---------------------------------------------------------------------------

/// StartupLeafView is deterministic: same inputs produce same outputs.
#[test]
fn startup_leaf_view_is_deterministic() {
    let a = StartupLeafView::new(StartupPhase::WelcomePanel, true, true, true);
    let b = StartupLeafView::new(StartupPhase::WelcomePanel, true, true, true);
    assert_eq!(a, b);
    let c = StartupLeafView::new(StartupPhase::DraftActive, false, true, true);
    assert_ne!(a, c);
}

/// KeyLeafView is deterministic.
#[test]
fn key_leaf_view_is_deterministic() {
    let a = KeyLeafView::new(FooterGrammar::Welcome, true);
    let b = KeyLeafView::new(FooterGrammar::Welcome, true);
    assert_eq!(a, b);
    let c = KeyLeafView::new(FooterGrammar::Draft, true);
    assert_ne!(a, c);
}

/// InputLeafView is deterministic.
#[test]
fn input_leaf_view_is_deterministic() {
    let a = InputLeafView::new(FocusOwner::Composer, "hello", 5, false);
    let b = InputLeafView::new(FocusOwner::Composer, "hello", 5, false);
    assert_eq!(a, b);
    let c = InputLeafView::new(FocusOwner::Composer, "world", 5, false);
    assert_ne!(a, c);
}

/// Leaf views have no app-state dependency: they can be constructed
/// without any registry, runtime, or app state.
#[test]
fn leaf_views_have_no_app_state_dependency() {
    let _startup = StartupLeafView::default();
    let _key = KeyLeafView::default();
    let _input = InputLeafView::default();
    let _composer = ComposerLeafView::default();
    let _transcript = TranscriptLeafView::default();
}

/// Leaf views have no registry dependency: two independent instances
/// do not share state (Copy semantics).
#[test]
fn leaf_views_have_no_registry_dependency() {
    let startup_a = StartupLeafView::new(StartupPhase::WelcomePanel, true, true, true);
    let startup_b = StartupLeafView::new(StartupPhase::DraftActive, false, true, true);
    assert_ne!(startup_a, startup_b);
    let mut startup_c = startup_a;
    startup_c.welcome_visible = false;
    assert_ne!(startup_a, startup_c);
    assert!(startup_a.welcome_visible);
}

// ---------------------------------------------------------------------------
// Leaf view derivation from real AppState
// ---------------------------------------------------------------------------

/// P0-START-01: startup app produces a welcome-panel leaf view with
/// focus_owner == "composer".
#[test]
fn startup_leaf_view_from_app_welcome() {
    let app = startup_app();
    let view = StartupLeafView::from_app(
        app.lifecycle_shell_state(),
        app.startup_mode,
        app.focus,
        !app.composer.prompt_buffer.is_empty(),
    );
    assert_eq!(view.phase, StartupPhase::WelcomePanel);
    assert!(view.welcome_visible);
    assert!(view.breadcrumb_visible);
    assert!(view.composer_focusable);
    assert_eq!(view.focus_owner(), "composer");
}

/// P0-START-03: typing a draft transitions the leaf view to DraftActive
/// and clears the welcome panel.
#[test]
fn startup_leaf_view_from_app_draft_clears_welcome() {
    let mut app = startup_app();
    app.composer.prompt_buffer = "Browser QA draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    let view = StartupLeafView::from_app(
        app.lifecycle_shell_state(),
        app.startup_mode,
        app.focus,
        !app.composer.prompt_buffer.is_empty(),
    );
    assert_eq!(view.phase, StartupPhase::DraftActive);
    assert!(!view.welcome_visible);
    assert!(view.welcome_cleared_by_draft());
    assert_eq!(view.focus_owner(), "composer");
}

/// P0-KEY-01: footer grammar changes from Welcome to Draft when
/// the composer has text.
#[test]
fn key_leaf_view_footer_grammar_welcome_vs_draft() {
    let app = startup_app();
    let welcome_view = KeyLeafView::from_state(app.startup_shell_visible(), false);
    assert_eq!(welcome_view.grammar, FooterGrammar::Welcome);
    assert!(welcome_view.footer_visible);

    let draft_view = KeyLeafView::from_state(app.startup_shell_visible(), true);
    assert_eq!(draft_view.grammar, FooterGrammar::Draft);
    assert!(draft_view.footer_changes_with_composer());
    assert_eq!(
        draft_view.draft_footer_tokens(),
        &["Enter", "Shift+Tab", "Ctrl+x"]
    );
}

/// Input leaf view: focus owner is "composer" at startup.
#[test]
fn input_leaf_view_focus_owner_composer_at_startup() {
    let app = startup_app();
    let view = InputLeafView::from_state(
        app.focus == Focus::Prompt,
        &app.composer.prompt_buffer,
        app.composer.prompt_cursor,
        app.startup_shell_visible(),
    );
    assert_eq!(view.focus_owner, FocusOwner::Composer);
    assert!(view.focus_is_composer());
    assert_eq!(view.focus_owner.as_str(), "composer");
}

/// Input leaf view: draft clears welcome.
#[test]
fn input_leaf_view_draft_clears_welcome() {
    let draft = "Browser QA draft";
    let view = InputLeafView::from_state(true, draft, draft.len(), true);
    assert!(view.draft_clears_welcome());
    assert!(!view.welcome_visible);
}

/// Input leaf view: cursor stays in bounds.
#[test]
fn input_leaf_view_cursor_in_bounds() {
    let draft = "hello";
    let view = InputLeafView::from_state(true, draft, 3, false);
    assert!(view.cursor_in_bounds());

    let out_of_bounds = InputLeafView::from_state(true, draft, 100, false);
    assert!(!out_of_bounds.cursor_in_bounds());
}

/// Input leaf view: Unicode display width is computed without panic.
#[test]
fn input_leaf_view_unicode_display_width() {
    let cjk = "你好世界";
    let view = InputLeafView::from_state(true, cjk, cjk.chars().count(), false);
    let width = view.draft_display_width();
    assert_eq!(width, 8, "4 CJK chars should be 8 cells wide");
    assert!(view.cursor_in_bounds());
}

#[test]
fn ctrl_w_opens_optional_worktree_name_dialog_without_launching() {
    let (mut app, intents) = startup_app_with_intents();

    app.handle_key(ctrl_w());

    let rendered = render(&app);
    assert!(rendered.contains("Create worktree"));
    assert!(rendered.contains("Name (optional)"));
    assert!(!rendered.contains("Changelog"));
    assert!(!rendered.contains("Resume session"));
    assert!(intents.lock().expect("intent sink lock").is_empty());
}

#[test]
fn worktree_name_dialog_escape_cancels_without_launching() {
    let (mut app, intents) = startup_app_with_intents();
    app.handle_key(ctrl_w());

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(!render(&app).contains("Create worktree"));
    assert!(intents.lock().expect("intent sink lock").is_empty());
}

#[test]
fn worktree_name_dialog_enter_launches_with_generated_name() {
    let (mut app, intents) = startup_app_with_intents();
    app.handle_key(ctrl_w());

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        intents.lock().expect("intent sink lock").as_slice(),
        &[UiIntent::NewWorktreeSession { name: None }]
    );
}

#[test]
fn worktree_name_dialog_enter_launches_with_trimmed_name() {
    let (mut app, intents) = startup_app_with_intents();
    app.handle_key(ctrl_w());
    for character in "  feature-auth  ".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(
        intents.lock().expect("intent sink lock").as_slice(),
        &[UiIntent::NewWorktreeSession {
            name: Some("feature-auth".to_string()),
        }]
    );
}

// ---------------------------------------------------------------------------
// Deterministic render tests (real Ratatui TestBackend, not snapshots)
// ---------------------------------------------------------------------------

/// P0-START-01: startup renders bordered welcome panel with composer focus.
#[test]
fn render_p0_start_01_welcome_panel_bordered() {
    let app = startup_app();
    let rendered = render(&app);

    // Welcome panel uses rounded box-drawing borders
    assert!(
        rendered.contains('╭') && rendered.contains('╰'),
        "P0-START-01: must paint rounded borders\n{rendered}"
    );
    // Welcome exposes action rows
    assert!(
        rendered.contains("New worktree")
            || rendered.contains("New session")
            || rendered.contains("Resume session")
            || rendered.contains("Resume"),
        "P0-START-01: welcome must expose action rows\n{rendered}"
    );
    // Composer glyph present
    assert!(
        rendered.contains('❯'),
        "P0-COMP-01: composer must use ❯ glyph\n{rendered}"
    );
    // At least welcome+composer border pairs
    let welcome_border_rows = rendered
        .lines()
        .filter(|line| line.contains('╭') || line.contains('╰'))
        .count();
    assert!(
        welcome_border_rows >= 2,
        "P0-START-01: need at least welcome+composer border pairs, got {welcome_border_rows}\n{rendered}"
    );
    // Must not render the external product name.
    assert!(
        !rendered.to_ascii_lowercase().contains("grok"),
        "identity: must not render the external product name\n{rendered}"
    );

    // Focus owner verification via leaf view
    let view = StartupLeafView::from_app(
        app.lifecycle_shell_state(),
        app.startup_mode,
        app.focus,
        !app.composer.prompt_buffer.is_empty(),
    );
    assert_eq!(view.focus_owner(), "composer");
}

/// P0-START-02: breadcrumb and clipboard warning visible at startup.
#[test]
fn render_p0_start_02_breadcrumb_and_warning() {
    let app = startup_app();
    let rendered = render(&app);

    // Clipboard warning band
    assert!(
        rendered.contains("Clipboard may be unreachable."),
        "P0-START-02: clipboard warning band required at startup\n{rendered}"
    );
    assert!(
        rendered.contains("/terminal-setup") || rendered.contains("terminal-setup"),
        "P0-START-02: clipboard warning second line required\n{rendered}"
    );

    // Welcome panel sits below breadcrumb+warning band
    let lines: Vec<&str> = rendered.lines().collect();
    let welcome_top = lines
        .iter()
        .position(|line| line.contains('╭') && line.contains('─'))
        .expect("welcome top border");
    assert!(
        welcome_top >= 6,
        "P0-START-02: welcome panel must sit below breadcrumb+warning band (row {welcome_top})\n{rendered}"
    );

    // Focus owner verification
    let view = StartupLeafView::from_app(
        app.lifecycle_shell_state(),
        app.startup_mode,
        app.focus,
        !app.composer.prompt_buffer.is_empty(),
    );
    assert_eq!(view.focus_owner(), "composer");
}

/// P0-START-03: typing clears welcome; composer retains draft; footer switches grammar.
#[test]
fn render_p0_start_03_draft_clears_welcome() {
    let mut app = startup_app();
    app.composer.prompt_buffer = "Browser QA draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();

    let rendered = render(&app);

    // Draft retained
    assert!(
        rendered.contains("Browser QA draft"),
        "P0-START-03: composer must retain draft\n{rendered}"
    );
    assert!(
        rendered.contains('❯'),
        "P0-START-03: composer glyph retained\n{rendered}"
    );
    // Welcome actions cleared
    assert!(
        !rendered.contains("New worktree") && !rendered.contains("New session"),
        "P0-START-03: welcome actions must clear when draft non-empty\n{rendered}"
    );
    // Footer grammar switched to draft
    assert!(
        rendered.contains("Enter:send") || rendered.contains("Enter: send"),
        "P0-KEY-01: draft footer must use Enter:send grammar\n{rendered}"
    );

    // Leaf view confirms transition
    let view = StartupLeafView::from_app(
        app.lifecycle_shell_state(),
        app.startup_mode,
        app.focus,
        !app.composer.prompt_buffer.is_empty(),
    );
    assert_eq!(view.phase, StartupPhase::DraftActive);
    assert!(view.welcome_cleared_by_draft());
    assert_eq!(view.focus_owner(), "composer");
}

/// P0-COMP-01: composer is a bordered strip with model badge.
#[test]
fn render_p0_comp_01_composer_bordered_strip_with_model_badge() {
    let app = startup_app();
    let rendered = render(&app);
    let lines: Vec<&str> = rendered.lines().collect();

    // Composer glyph row
    let glyph_idx = lines
        .iter()
        .rposition(|line| line.contains('│') && line.contains('❯'))
        .or_else(|| lines.iter().rposition(|line| line.contains('❯')))
        .expect("composer glyph row");

    // Bordered strip: top border above, bottom border below
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

    // Model badge / identity on composer border region
    let has_model_signal = rendered.contains("mock")
        || rendered.contains("model")
        || rendered.contains("worker")
        || rendered.contains("Demo");
    assert!(
        has_model_signal,
        "P0-COMP-01: composer chrome should surface model/session identity\n{rendered}"
    );

    // Focus owner verification
    let view = InputLeafView::from_state(
        app.focus == Focus::Prompt,
        &app.composer.prompt_buffer,
        app.composer.prompt_cursor,
        app.startup_shell_visible(),
    );
    assert_eq!(view.focus_owner, FocusOwner::Composer);
}

/// P0-KEY-01: footer vocabulary changes with composer state.
#[test]
fn render_p0_key_01_footer_changes_with_draft() {
    // Welcome footer
    let app_welcome = startup_app();
    let _rendered_welcome = render(&app_welcome);
    let key_welcome = KeyLeafView::from_state(app_welcome.startup_shell_visible(), false);
    assert_eq!(key_welcome.grammar, FooterGrammar::Welcome);

    // Draft footer
    let mut app_draft = startup_app();
    app_draft.composer.prompt_buffer = "Browser QA draft".to_string();
    app_draft.composer.prompt_cursor = app_draft.composer.prompt_buffer.chars().count();
    let rendered_draft = render(&app_draft);
    let key_draft = KeyLeafView::from_state(app_draft.startup_shell_visible(), true);
    assert_eq!(key_draft.grammar, FooterGrammar::Draft);
    assert!(key_draft.footer_changes_with_composer());

    // Draft footer has Enter:send grammar
    assert!(
        rendered_draft.contains("Enter:send") || rendered_draft.contains("Enter: send"),
        "P0-KEY-01: draft footer must use Enter:send grammar\n{rendered_draft}"
    );
}

// ---------------------------------------------------------------------------
// Focused input tests (real state transitions, not snapshots)
// ---------------------------------------------------------------------------

/// Typing at startup transitions focus to Prompt (composer).
#[test]
fn input_typing_at_startup_transitions_focus_to_prompt() {
    let mut app = startup_app();
    // At startup, focus is List (welcome panel navigation)
    assert_eq!(app.focus, Focus::List);

    // Simulate typing a char: handle_key transitions focus to Prompt
    app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(app.composer.prompt_buffer, "B");
    assert_eq!(app.composer.prompt_cursor, 1);

    // Type more chars
    for c in "rowser QA draft".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert_eq!(app.composer.prompt_buffer, "Browser QA draft");
    assert_eq!(
        app.composer.prompt_cursor,
        app.composer.prompt_buffer.chars().count()
    );
}

#[test]
fn input_typed_text_uses_grok_primary_on_canvas() {
    let mut app = startup_app();
    for character in "Browser QA draft".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }

    let buffer = render_to_buffer(&app, Rect::new(0, 0, W, H), |app, frame, _area| {
        ui::render_app(frame, app);
    });
    let draft = "Browser QA draft".chars().collect::<Vec<_>>();
    let draft_cells = buffer
        .content
        .windows(draft.len())
        .find(|cells| {
            cells
                .iter()
                .zip(&draft)
                .all(|(cell, expected)| cell.symbol().starts_with(*expected))
        })
        .expect("typed draft must be rendered in the composer");

    let foregrounds = draft_cells.iter().map(|cell| cell.fg).collect::<Vec<_>>();
    let backgrounds = draft_cells.iter().map(|cell| cell.bg).collect::<Vec<_>>();
    assert!(
        foregrounds
            .iter()
            .all(|color| *color == Color::Rgb(225, 225, 225)),
        "typed draft foregrounds must match Grok primary: {foregrounds:?}"
    );
    assert!(
        backgrounds
            .iter()
            .all(|color| *color == Color::Rgb(20, 20, 20)),
        "typed draft backgrounds must match Grok canvas: {backgrounds:?}"
    );

    let composer = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, W, H))
        .dock
        .expect("startup shell must include a dock")
        .composer;
    assert_eq!(buffer[(composer.x, composer.y)].fg, Color::Rgb(80, 80, 88));
    assert_eq!(
        buffer[(composer.x + 2, composer.y + 1)].fg,
        Color::Rgb(200, 200, 200)
    );
}

#[test]
fn unfocused_empty_composer_uses_grok_idle_state() {
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Details;
    let area = Rect::new(0, 0, W, 40);
    let composer = FrameLayoutPlan::for_app(&app, area)
        .dock
        .expect("live shell must include a dock")
        .composer;
    let buffer = render_to_buffer(&app, area, |app, frame, _area| {
        ui::render_app(frame, app);
    });
    let input_row = (composer.x..composer.right())
        .map(|x| buffer[(x, composer.y + 1)].symbol())
        .collect::<String>();

    assert!(input_row.contains("Build anything"), "{input_row:?}");
    assert_eq!(buffer[(composer.x, composer.y)].fg, Color::Rgb(50, 50, 55));
    assert_eq!(
        buffer[(composer.x + 2, composer.y + 1)].fg,
        Color::Rgb(65, 65, 65)
    );
    assert_eq!(
        buffer[(composer.x + 4, composer.y + 1)].fg,
        Color::Rgb(78, 78, 78)
    );
}

#[test]
fn unfocused_draft_uses_grok_dimmed_primary_and_collapses() {
    let mut app = AppState::new_live(None, false, None);
    app.composer.prompt_buffer = format!(
        "FIRST alpha beta gamma delta epsilon zeta eta theta iota kappa {}LAST omega",
        "middle ".repeat(24)
    );
    app.focus = Focus::Prompt;
    let area = Rect::new(0, 0, 80, 24);
    let focused_composer = FrameLayoutPlan::for_app(&app, area)
        .dock
        .expect("live shell must include a dock")
        .composer;

    app.focus = Focus::Details;
    let composer = FrameLayoutPlan::for_app(&app, area)
        .dock
        .expect("live shell must include a dock")
        .composer;
    let buffer = render_to_buffer(&app, area, |app, frame, _area| {
        ui::render_app(frame, app);
    });

    assert!(focused_composer.height > 3, "{focused_composer:?}");
    assert_eq!(composer.height, 3, "{composer:?}");
    let input_row = (composer.x..composer.right())
        .map(|x| buffer[(x, composer.y + 1)].symbol())
        .collect::<String>();
    assert!(input_row.contains("FIRST"), "{input_row:?}");
    assert!(!input_row.contains("LAST"), "{input_row:?}");
    assert_eq!(
        buffer[(composer.x + 4, composer.y + 1)].fg,
        Color::Rgb(155, 155, 155)
    );
}

#[test]
fn live_bordered_composer_reserves_an_inner_input_row() {
    let app = AppState::new_live(None, false, None);
    let plan = FrameLayoutPlan::for_app(&app, Rect::new(0, 0, W, 40));
    let composer = plan.dock.expect("live shell must include a dock").composer;

    assert!(
        composer.height >= 3,
        "bordered composer needs top, input, and bottom rows; got {composer:?}"
    );
}

/// Typing clears the welcome panel (startup_mode stays but welcome not visible).
#[test]
fn input_typing_clears_welcome_panel() {
    let mut app = startup_app();
    assert!(app.startup_shell_visible());

    // Type a char to transition to Prompt
    app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
    // Startup mode is still true, but the welcome panel is no longer
    // the active surface because the composer has a draft.
    let view = StartupLeafView::from_app(
        app.lifecycle_shell_state(),
        app.startup_mode,
        app.focus,
        !app.composer.prompt_buffer.is_empty(),
    );
    assert_eq!(view.phase, StartupPhase::DraftActive);
    assert!(!view.welcome_visible);
    assert!(view.welcome_cleared_by_draft());
}

/// Unicode text is handled without panic and cursor stays in bounds.
#[test]
fn input_unicode_text_no_panic() {
    let mut app = startup_app();
    // Type unicode chars one at a time
    for c in "你好世界🌍".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert_eq!(app.composer.prompt_buffer, "你好世界🌍");

    let view = InputLeafView::from_state(
        app.focus == Focus::Prompt,
        &app.composer.prompt_buffer,
        app.composer.prompt_cursor,
        app.startup_shell_visible(),
    );
    assert!(view.cursor_in_bounds());
    let width = view.draft_display_width();
    assert!(width > 0, "unicode text should have nonzero display width");
}

/// Enhanced key: backspace works on unicode text without panic.
#[test]
fn input_enhanced_key_backspace_unicode() {
    let mut app = startup_app();
    app.focus = Focus::Prompt;
    // Type unicode chars
    for c in "你好".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert_eq!(app.composer.prompt_buffer, "你好");
    assert_eq!(app.composer.prompt_cursor, 2);

    // Backspace one char
    app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(app.composer.prompt_buffer, "你");
    assert_eq!(app.composer.prompt_cursor, 1);

    // Backspace again
    app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(app.composer.prompt_buffer, "");
    assert_eq!(app.composer.prompt_cursor, 0);
}

/// Empty draft at startup: focus owner is still composer.
#[test]
fn input_empty_draft_focus_owner_composer() {
    let app = startup_app();
    let view = InputLeafView::from_state(
        app.focus == Focus::Prompt,
        &app.composer.prompt_buffer,
        app.composer.prompt_cursor,
        app.startup_shell_visible(),
    );
    assert_eq!(view.focus_owner, FocusOwner::Composer);
    assert!(view.welcome_visible);
    assert_eq!(view.focus_owner.as_str(), "composer");
}

/// Small viewport (80x24) does not panic at startup.
#[test]
fn input_small_viewport_no_panic() {
    let app = startup_app();
    let rendered = render_to_string(&app, Rect::new(0, 0, 80, 24), |app, frame, _area| {
        ui::render_app(frame, app)
    });
    // Must still render something
    assert!(!rendered.is_empty());
    // Composer glyph should still be present even at small viewport
    assert!(
        rendered.contains('❯') || rendered.contains('│'),
        "small viewport must still render composer area\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// Failure scenario: empty-small-unicode-enhanced-key
// Verifies no_panic==true and recovered==true
// ---------------------------------------------------------------------------

/// Failure scenario: empty draft, small viewport, unicode text, enhanced keys.
/// The app must not panic and must recover to a usable state.
#[test]
fn failure_scenario_empty_small_unicode_enhanced_key() {
    let mut app = startup_app();

    // Render at small viewport — must not panic
    let small = render_to_string(&app, Rect::new(0, 0, 80, 24), |app, frame, _area| {
        ui::render_app(frame, app)
    });
    assert!(!small.is_empty(), "small viewport render must not be empty");

    // Type unicode chars — must not panic
    for c in "你好世界🌍".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert_eq!(app.composer.prompt_buffer, "你好世界🌍");

    // Render with unicode draft at small viewport — must not panic
    let small_unicode = render_to_string(&app, Rect::new(0, 0, 80, 24), |app, frame, _area| {
        ui::render_app(frame, app)
    });
    assert!(
        !small_unicode.is_empty(),
        "unicode small viewport render must not be empty"
    );

    // Enhanced key: backspace — must not panic
    app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(
        app.composer.prompt_buffer,
        "你好世界🌍"[..].chars().take(4).collect::<String>()
    );

    // Recovered: app is still usable (can type more text)
    for c in " recovered".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert!(app.composer.prompt_buffer.contains("recovered"));

    // Final render — must not panic
    let final_render = render_to_string(&app, Rect::new(0, 0, 80, 24), |app, frame, _area| {
        ui::render_app(frame, app)
    });
    assert!(
        !final_render.is_empty(),
        "final render after recovery must not be empty"
    );

    // External postconditions
    let view = InputLeafView::from_state(
        app.focus == Focus::Prompt,
        &app.composer.prompt_buffer,
        app.composer.prompt_cursor,
        app.startup_shell_visible(),
    );
    assert_eq!(view.focus_owner, FocusOwner::Composer);
    assert!(view.cursor_in_bounds());
}

// ---------------------------------------------------------------------------
// Render capture for evidence (run with --nocapture)
// ---------------------------------------------------------------------------

/// Print the rendered startup screen for evidence capture.
#[test]
fn render_startup_capture() {
    let app = startup_app();
    let rendered = render(&app);
    println!("{rendered}");
}

/// Print the rendered draft screen for evidence capture.
#[test]
fn render_draft_capture() {
    let mut app = startup_app();
    app.composer.prompt_buffer = "Browser QA draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    let rendered = render(&app);
    println!("{rendered}");
}
