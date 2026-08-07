use super::*;

pub(super) fn default_app_uses_harness_chat_theme() {
    let app = AppState::default();

    assert_eq!(app.theme(), &Theme::harness_chat());
    assert_eq!(rendered_cell_bg(&app, 0, 0), Color::Rgb(20, 20, 20));
}

pub(super) fn explicit_harness_chat_selection_uses_harness_chat_theme() {
    let mut app = AppState::default();

    app.apply_theme_by_name("harness-chat");

    assert_eq!(app.theme(), &Theme::harness_chat());
    assert_eq!(app.theme_name, "harness-chat");
}

pub(super) fn explicit_harness_dark_selection_remains_available() {
    let mut app = AppState::default();

    app.apply_theme_by_name("harness-dark");

    assert_eq!(app.theme(), &Theme::harness_dark());
    assert_eq!(app.theme_name, "harness-dark");
}

pub(super) fn default_harness_chat_survives_color_level_changes() {
    let mut app = AppState::default();

    app.set_color_level(ColorLevel::Basic);

    assert_eq!(
        app.theme(),
        &Theme::harness_chat().for_color_level(ColorLevel::Basic)
    );
}
