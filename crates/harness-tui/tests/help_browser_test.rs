use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::app::{AppState, ReviewSurface};
use harness_tui::render_test::render_to_string;
use harness_tui::ui;
use ratatui::layout::Rect;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn modified_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn open_help(app: &mut AppState) {
    app.execute_slash_command("help", None);
}

fn render(app: &AppState) -> String {
    render_at(app, 120, 40)
}

fn render_at(app: &AppState, width: u16, height: u16) -> String {
    render_to_string(app, Rect::new(0, 0, width, height), |app, frame, _| {
        ui::render_app(frame, app)
    })
}

#[test]
fn help_browse_footer_advertises_only_working_grok_controls() {
    let mut app = AppState::new_live(None, false, None);

    open_help(&mut app);
    let rendered = render(&app);

    assert!(rendered.contains("/ to search"), "{rendered}");
    assert!(rendered.contains("f filter"), "{rendered}");
    assert!(rendered.contains("e/Space/→ expand"), "{rendered}");
    assert!(rendered.contains("Enter details"), "{rendered}");
    assert!(rendered.contains("Ctrl+./X close") || rendered.contains("Esc close"));
}

#[test]
fn help_search_escape_clears_then_closes() {
    let mut app = AppState::new_live(None, false, None);
    open_help(&mut app);

    app.handle_key(key(KeyCode::Char('/')));
    for character in "undo".chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }
    assert!(render(&app).contains("Undo"));

    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.review_surface(), Some(ReviewSurface::Help));
    assert!(render(&app).contains("Essentials"));

    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.review_surface(), None);
}

#[test]
fn help_detail_clears_search_and_supports_back_and_global_close() {
    let mut app = AppState::new_live(None, false, None);
    open_help(&mut app);
    app.handle_key(key(KeyCode::Char('/')));
    for character in "submit prompt".chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }

    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    let detail = render(&app);
    assert!(detail.contains("Esc back"), "{detail}");
    assert!(detail.contains("Ctrl+./X close"), "{detail}");
    assert_eq!(detail.matches("Submit prompt").count(), 1, "{detail}");

    for _ in 0..20 {
        app.handle_key(key(KeyCode::Down));
    }
    assert!(render(&app).contains("Submit prompt"));

    app.handle_key(key(KeyCode::Left));
    assert!(render(&app).contains("Essentials"));

    app.handle_key(modified_key(KeyCode::Char('.'), KeyModifiers::CONTROL));
    assert_eq!(app.review_surface(), None);
}

#[test]
fn help_keyboard_navigation_reaches_section_headers() {
    let mut app = AppState::new_live(None, false, None);
    open_help(&mut app);

    app.handle_key(key(KeyCode::Home));
    app.handle_key(key(KeyCode::Enter));
    let collapsed = render(&app);
    assert!(collapsed.contains("› Essentials (7)"), "{collapsed}");

    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Right));
    let expanded = render(&app);
    assert!(expanded.contains("◆ Input"), "{expanded}");
    assert!(expanded.contains("Insert newline"), "{expanded}");
}

#[test]
fn help_compact_footer_keeps_every_action_visible() {
    let mut app = AppState::new_live(None, false, None);
    open_help(&mut app);

    let rendered = render_at(&app, 80, 24);

    assert!(rendered.contains("← collapse"), "{rendered}");
    assert!(rendered.contains("Enter details"), "{rendered}");
    assert!(rendered.contains("Esc close"), "{rendered}");
}

#[test]
fn help_compact_detail_footer_keeps_close_hint_visible() {
    let mut app = AppState::new_live(None, false, None);
    open_help(&mut app);
    app.handle_key(key(KeyCode::Char('/')));
    for character in "submit prompt".chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));

    let rendered = render_at(&app, 60, 17);

    assert!(rendered.contains("Esc back | ↑/↓ scroll"), "{rendered}");
    assert!(rendered.contains("Ctrl+./X close"), "{rendered}");
}

#[test]
fn help_rows_follow_active_keymap_overrides() {
    let mut app = AppState::new_live(None, false, None);
    app.keymap.apply_overrides(&BTreeMap::from([(
        "submit_prompt".to_string(),
        "ctrl+g".to_string(),
    )]));

    open_help(&mut app);
    let rendered = render(&app);

    assert!(rendered.contains("Ctrl+g"), "{rendered}");
    assert!(!rendered.contains("Submit prompt  Enter"), "{rendered}");
}
