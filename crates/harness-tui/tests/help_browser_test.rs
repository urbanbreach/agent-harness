use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_tui::app::{AppState, Focus, ReviewSurface};
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
    // arrange
    let mut app = AppState::new_live(None, false, None);

    // act
    open_help(&mut app);
    let rendered = render(&app);

    // assert
    assert!(rendered.contains("/ to search"), "{rendered}");
    assert!(rendered.contains("f filter"), "{rendered}");
    assert!(rendered.contains("e/Space/→ expand"), "{rendered}");
    assert!(rendered.contains("Enter details"), "{rendered}");
    assert!(rendered.contains("Ctrl+./X close") || rendered.contains("Esc close"));
}

#[test]
fn help_search_escape_clears_then_closes() {
    // arrange
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

    // act
    app.handle_key(key(KeyCode::Esc));
    // assert
    assert_eq!(app.review_surface(), None);
}

#[test]
fn help_detail_clears_search_and_supports_back_and_global_close() {
    // arrange
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

    // act
    app.handle_key(modified_key(KeyCode::Char('.'), KeyModifiers::CONTROL));
    // assert
    assert_eq!(app.review_surface(), None);
}

#[test]
fn help_keyboard_navigation_reaches_section_headers() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    open_help(&mut app);

    app.handle_key(key(KeyCode::Home));
    app.handle_key(key(KeyCode::Enter));
    let collapsed = render(&app);
    assert!(collapsed.contains("› Essentials (7)"), "{collapsed}");

    // act
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Right));
    let expanded = render(&app);
    // assert
    assert!(expanded.contains("◆ Input"), "{expanded}");
    assert!(expanded.contains("Insert newline"), "{expanded}");
}

#[test]
fn help_compact_footer_keeps_every_action_visible() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    open_help(&mut app);

    // act
    let rendered = render_at(&app, 80, 24);

    // assert
    assert!(rendered.contains("← collapse"), "{rendered}");
    assert!(rendered.contains("Enter details"), "{rendered}");
    assert!(rendered.contains("Esc close"), "{rendered}");
}

#[test]
fn help_compact_detail_footer_keeps_close_hint_visible() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    open_help(&mut app);
    app.handle_key(key(KeyCode::Char('/')));
    for character in "submit prompt".chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));

    // act
    let rendered = render_at(&app, 60, 17);

    // assert
    assert!(rendered.contains("Esc back | ↑/↓ scroll"), "{rendered}");
    assert!(rendered.contains("Ctrl+./X close"), "{rendered}");
}

#[test]
fn help_rows_follow_active_keymap_overrides() {
    // arrange
    let mut app = AppState::new_live(None, false, None);
    app.keymap.apply_overrides(&BTreeMap::from([(
        "submit_prompt".to_string(),
        "ctrl+g".to_string(),
    )]));

    // act
    open_help(&mut app);
    let rendered = render(&app);

    // assert
    assert!(rendered.contains("Ctrl+g"), "{rendered}");
    assert!(!rendered.contains("Submit prompt  Enter"), "{rendered}");
}

#[test]
fn help_resize_preserves_search_selection_expansion_and_detail_scroll() {
    // arrange
    // Given: Help is searched, an inline row is expanded, and its detail is scrolled.
    let mut app = AppState::new_live(None, false, None);
    open_help(&mut app);
    app.handle_key(key(KeyCode::Char('/')));
    for character in "submit prompt".chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }
    app.handle_key(key(KeyCode::Down));
    // When: browse and detail are each rendered through both viewport sizes.
    let compact_browse = render_at(&app, 60, 20);
    let wide_browse = render_at(&app, 120, 40);
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Down));
    let compact_detail = render_at(&app, 60, 20);
    let wide_detail = render_at(&app, 120, 40);
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::Enter));
    let reopened_detail = render_at(&app, 120, 40);

    // act
    // Then: query, selection, detail scroll, and back navigation survive resize.
    // assert
    assert!(compact_browse.contains("Submit prompt"), "{compact_browse}");
    assert!(wide_browse.contains("submit prompt"), "{wide_browse}");
    assert!(compact_detail.contains("Submit prompt"), "{compact_detail}");
    assert!(wide_detail.contains("Submit prompt"), "{wide_detail}");
    assert!(
        reopened_detail.contains("Submit prompt"),
        "{reopened_detail}"
    );
}

#[test]
fn help_inline_expansion_survives_resize() {
    // arrange
    // Given: the initially selected shortcut is expanded in browse mode.
    let mut app = AppState::new_live(None, false, None);
    open_help(&mut app);
    app.handle_key(key(KeyCode::Char(' ')));

    // When: the expanded row renders compact and wide.
    let compact = render_at(&app, 60, 20);
    let wide = render_at(&app, 120, 40);

    // act
    // Then: the expanded row occupies an additional description line in both viewports.
    // assert
    assert!(compact.contains("Quit the application"), "{compact}");
    assert!(wide.contains("Quit the application"), "{wide}");
}

#[test]
fn help_empty_and_unicode_search_render_stable_results() {
    // arrange
    // Given: Help owns text input.
    let mut app = AppState::new_live(None, false, None);
    open_help(&mut app);
    app.handle_key(key(KeyCode::Char('/')));

    // When: an empty query and then a Unicode query are rendered.
    let empty = render_at(&app, 60, 20);
    for character in "入力".chars() {
        app.handle_key(key(KeyCode::Char(character)));
    }
    let unicode = render_at(&app, 60, 20);

    // act
    // Then: empty search keeps the catalog, while no-match Unicode remains intact.
    // assert
    assert!(empty.contains("Essentials"), "{empty}");
    assert!(
        unicode.contains('入') && unicode.contains('力'),
        "{unicode}"
    );
    assert!(unicode.contains("No shortcuts match"), "{unicode}");
}

#[test]
fn help_preempts_palette_and_close_restores_original_focus() {
    // arrange
    // Given: list focus and a command palette beneath Help.
    let mut app = AppState::new_live(None, false, None);
    app.focus = Focus::Prompt;
    app.palette_visible = true;

    // When: Help opens above the palette and then closes.
    open_help(&mut app);
    let help = render(&app);
    app.handle_key(modified_key(KeyCode::Char('x'), KeyModifiers::CONTROL));

    // act
    // Then: Help closes the lower palette and returns focus to Prompt.
    // assert
    assert!(help.contains("Keyboard Shortcuts"), "{help}");
    assert!(!help.contains("Command Palette"), "{help}");
    assert!(!app.palette_visible);
    assert_eq!(app.focus, Focus::Prompt);
}
