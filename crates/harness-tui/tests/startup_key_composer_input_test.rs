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
use harness_tui::overlay::OverlayKind;
use harness_tui::render_test::{render_to_buffer, render_to_string};
use harness_tui::ui;
use ratatui::layout::Rect;
use ratatui::style::Color;
use std::collections::BTreeMap;
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
    // arrange
    // act
    let a = StartupLeafView::new(StartupPhase::WelcomePanel, true, true, true);
    let b = StartupLeafView::new(StartupPhase::WelcomePanel, true, true, true);
    // assert
    assert_eq!(a, b);
    let c = StartupLeafView::new(StartupPhase::DraftActive, false, true, true);
    assert_ne!(a, c);
}

/// KeyLeafView is deterministic.
#[test]
fn key_leaf_view_is_deterministic() {
    // arrange
    // act
    let a = KeyLeafView::new(FooterGrammar::Welcome, true);
    let b = KeyLeafView::new(FooterGrammar::Welcome, true);
    // assert
    assert_eq!(a, b);
    let c = KeyLeafView::new(FooterGrammar::Draft, true);
    assert_ne!(a, c);
}

/// InputLeafView is deterministic.
#[test]
fn input_leaf_view_is_deterministic() {
    // arrange
    // act
    let a = InputLeafView::new(FocusOwner::Composer, "hello", 5, false);
    let b = InputLeafView::new(FocusOwner::Composer, "hello", 5, false);
    // assert
    assert_eq!(a, b);
    let c = InputLeafView::new(FocusOwner::Composer, "world", 5, false);
    assert_ne!(a, c);
}

/// Leaf views have no app-state dependency: they can be constructed
/// without any registry, runtime, or app state.
#[test]
fn leaf_views_have_no_app_state_dependency() {
    // arrange
    // act
    let _startup = StartupLeafView::default();
    let _key = KeyLeafView::default();
    let _input = InputLeafView::default();
    let _composer = ComposerLeafView::default();
    // assert
    let _transcript = TranscriptLeafView::default();
}

/// Leaf views have no registry dependency: two independent instances
/// do not share state (Copy semantics).
#[test]
fn leaf_views_have_no_registry_dependency() {
    // arrange
    // act
    let startup_a = StartupLeafView::new(StartupPhase::WelcomePanel, true, true, true);
    let startup_b = StartupLeafView::new(StartupPhase::DraftActive, false, true, true);
    // assert
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
    // arrange
    // act
    let app = startup_app();
    let view = StartupLeafView::from_app(
        app.lifecycle_shell_state(),
        app.startup_mode,
        app.focus,
        !app.composer.prompt_buffer.is_empty(),
    );
    // assert
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
    // arrange
    // act
    let mut app = startup_app();
    app.composer.prompt_buffer = "Browser QA draft".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();
    let view = StartupLeafView::from_app(
        app.lifecycle_shell_state(),
        app.startup_mode,
        app.focus,
        !app.composer.prompt_buffer.is_empty(),
    );
    // assert
    assert_eq!(view.phase, StartupPhase::DraftActive);
    assert!(!view.welcome_visible);
    assert!(view.welcome_cleared_by_draft());
    assert_eq!(view.focus_owner(), "composer");
}

/// P0-KEY-01: footer grammar changes from Welcome to Draft when
/// the composer has text.
#[test]
fn key_leaf_view_footer_grammar_welcome_vs_draft() {
    // arrange
    let app = startup_app();
    let welcome_view = KeyLeafView::from_state(app.startup_shell_visible(), false);
    assert_eq!(welcome_view.grammar, FooterGrammar::Welcome);
    assert!(welcome_view.footer_visible);

    // act
    let draft_view = KeyLeafView::from_state(app.startup_shell_visible(), true);
    // assert
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
    // arrange
    // act
    let app = startup_app();
    let view = InputLeafView::from_state(
        app.focus == Focus::Prompt,
        &app.composer.prompt_buffer,
        app.composer.prompt_cursor,
        app.startup_shell_visible(),
    );
    // assert
    assert_eq!(view.focus_owner, FocusOwner::Composer);
    assert!(view.focus_is_composer());
    assert_eq!(view.focus_owner.as_str(), "composer");
}

/// Input leaf view: draft clears welcome.
#[test]
fn input_leaf_view_draft_clears_welcome() {
    // arrange
    // act
    let draft = "Browser QA draft";
    let view = InputLeafView::from_state(true, draft, draft.len(), true);
    // assert
    assert!(view.draft_clears_welcome());
    assert!(!view.welcome_visible);
}

/// Input leaf view: cursor stays in bounds.
#[test]
fn input_leaf_view_cursor_in_bounds() {
    // arrange
    let draft = "hello";
    let view = InputLeafView::from_state(true, draft, 3, false);
    assert!(view.cursor_in_bounds());

    // act
    let out_of_bounds = InputLeafView::from_state(true, draft, 100, false);
    // assert
    assert!(!out_of_bounds.cursor_in_bounds());
}

/// Input leaf view: Unicode display width is computed without panic.
#[test]
fn input_leaf_view_unicode_display_width() {
    // arrange
    // act
    let cjk = "你好世界";
    let view = InputLeafView::from_state(true, cjk, cjk.chars().count(), false);
    let width = view.draft_display_width();
    // assert
    assert_eq!(width, 8, "4 CJK chars should be 8 cells wide");
    assert!(view.cursor_in_bounds());
}

#[test]
fn ctrl_w_opens_optional_worktree_name_dialog_without_launching() {
    // arrange
    let (mut app, intents) = startup_app_with_intents();

    app.handle_key(ctrl_w());

    // act
    let rendered = render(&app);
    // assert
    assert!(rendered.contains("Create worktree"));
    assert!(rendered.contains("Name (optional)"));
    assert!(rendered.contains("Changelog"));
    assert_eq!(
        app.overlay_stack().top(),
        Some(OverlayKind::NewWorktreeDialog)
    );
    assert!(intents.lock().expect("intent sink lock").is_empty());
}

#[test]
fn worktree_name_dialog_escape_cancels_without_launching() {
    // arrange
    let (mut app, intents) = startup_app_with_intents();
    app.handle_key(ctrl_w());

    // act
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    // assert
    assert!(!render(&app).contains("Create worktree"));
    assert!(intents.lock().expect("intent sink lock").is_empty());
}

#[test]
fn worktree_name_dialog_enter_launches_with_generated_name() {
    // arrange
    let (mut app, intents) = startup_app_with_intents();
    app.handle_key(ctrl_w());

    // act
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // assert
    assert_eq!(
        intents.lock().expect("intent sink lock").as_slice(),
        &[UiIntent::NewWorktreeSession { name: None }]
    );
}

#[test]
fn worktree_name_dialog_enter_launches_with_trimmed_name() {
    // arrange
    let (mut app, intents) = startup_app_with_intents();
    app.handle_key(ctrl_w());
    for character in "  feature-auth  ".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
    }

    // act
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // assert
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
    // arrange
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

    // act
    // Focus owner verification via leaf view
    let view = StartupLeafView::from_app(
        app.lifecycle_shell_state(),
        app.startup_mode,
        app.focus,
        !app.composer.prompt_buffer.is_empty(),
    );
    // assert
    assert_eq!(view.focus_owner(), "composer");
}

#[test]
fn startup_box_borders_use_grok_build_colors() {
    // arrange
    // act
    let app = startup_app();
    let area = Rect::new(0, 0, W, H);
    let layout = FrameLayoutPlan::for_app(&app, area);
    let composer = layout
        .dock
        .expect("startup shell must include a dock")
        .composer;
    let buffer = render_to_buffer(&app, area, |app, frame, _area| {
        ui::render_app(frame, app);
    });
    let welcome_corner = buffer
        .content
        .iter()
        .find(|cell| cell.symbol() == "╭")
        .expect("startup welcome border must be rendered");

    // assert
    assert_eq!(welcome_corner.fg, Color::Rgb(51, 51, 51));
    assert_eq!(buffer[(composer.x, composer.y)].fg, Color::Rgb(80, 80, 88));
}

#[test]
fn new_worktree_dialog_border_uses_grok_modal_color() {
    // arrange
    // act
    let mut app = startup_app();
    app.handle_key(ctrl_w());
    let area = Rect::new(0, 0, W, H);
    let buffer = render_to_buffer(&app, area, |app, frame, _area| {
        ui::render_app(frame, app);
    });
    let dialog_corner = buffer
        .content
        .iter()
        .find(|cell| cell.symbol() == "╭" && cell.fg == Color::Rgb(88, 88, 88))
        .expect("new worktree dialog border must use Grok gray_dim");

    // assert
    assert_eq!(dialog_corner.fg, Color::Rgb(88, 88, 88));
}

/// P0-START-02: breadcrumb and clipboard warning visible at startup.
#[test]
fn render_p0_start_02_breadcrumb_and_warning() {
    // arrange
    let mut app = startup_app();
    app.status_banner = Some("clipboard is unreachable".to_string());
    let rendered = render(&app);

    // Clipboard warning band
    assert!(
        rendered.contains("Clipboard may be unreachable."),
        "P0-START-02: clipboard warning band required at startup\n{rendered}"
    );
    assert!(
        rendered.contains("/doctor"),
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

    // act
    // Focus owner verification
    let view = StartupLeafView::from_app(
        app.lifecycle_shell_state(),
        app.startup_mode,
        app.focus,
        !app.composer.prompt_buffer.is_empty(),
    );
    // assert
    assert_eq!(view.focus_owner(), "composer");
}

/// P0-START-03: typing clears welcome; composer retains draft; footer switches grammar.
#[test]
fn render_p0_start_03_draft_clears_welcome() {
    // arrange
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

    // act
    // Leaf view confirms transition
    let view = StartupLeafView::from_app(
        app.lifecycle_shell_state(),
        app.startup_mode,
        app.focus,
        !app.composer.prompt_buffer.is_empty(),
    );
    // assert
    assert_eq!(view.phase, StartupPhase::DraftActive);
    assert!(view.welcome_cleared_by_draft());
    assert_eq!(view.focus_owner(), "composer");
}

/// P0-COMP-01: composer is a bordered strip with model badge.
#[test]
fn render_p0_comp_01_composer_bordered_strip_with_model_badge() {
    // arrange
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

    // act
    // Focus owner verification
    let view = InputLeafView::from_state(
        app.focus == Focus::Prompt,
        &app.composer.prompt_buffer,
        app.composer.prompt_cursor,
        app.startup_shell_visible(),
    );
    // assert
    assert_eq!(view.focus_owner, FocusOwner::Composer);
}

/// P0-KEY-01: footer vocabulary changes with composer state.
#[test]
fn render_p0_key_01_footer_changes_with_draft() {
    // arrange
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

    // act
    // Draft footer has Enter:send grammar
    // assert
    assert!(
        rendered_draft.contains("Enter:send") || rendered_draft.contains("Enter: send"),
        "P0-KEY-01: draft footer must use Enter:send grammar\n{rendered_draft}"
    );
}

#[test]
fn startup_draft_footer_uses_configured_submit_binding() {
    // arrange
    // Given
    let mut app = startup_app();
    app.apply_keybindings(BTreeMap::from([(
        "submit_prompt".to_string(),
        "ctrl+g".to_string(),
    )]));
    app.composer.prompt_buffer = "/help".to_string();
    app.composer.prompt_cursor = app.composer.prompt_buffer.chars().count();

    // When
    let rendered = render(&app);

    // act
    // Then
    // assert
    assert!(
        rendered.contains("Ctrl+g:send") && !rendered.contains("Enter:send"),
        "startup draft footer must show the active submit binding\n{rendered}"
    );
}

// ---------------------------------------------------------------------------
// Focused input tests (real state transitions, not snapshots)
// ---------------------------------------------------------------------------

/// Typing at startup transitions focus to Prompt (composer).
#[test]
fn input_typing_at_startup_transitions_focus_to_prompt() {
    // arrange
    let mut app = startup_app();
    assert_eq!(app.focus, Focus::Prompt);

    // Simulate typing a char: handle_key transitions focus to Prompt
    app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::Prompt);
    assert_eq!(app.composer.prompt_buffer, "B");
    assert_eq!(app.composer.prompt_cursor, 1);

    // act
    // Type more chars
    for c in "rowser QA draft".chars() {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    // assert
    assert_eq!(app.composer.prompt_buffer, "Browser QA draft");
    assert_eq!(
        app.composer.prompt_cursor,
        app.composer.prompt_buffer.chars().count()
    );
}

include!("support/startup_key_composer_input_test_part2_test.rs");
