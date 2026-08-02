use harness_tui::theme::Theme;
use ratatui::style::Color;

#[test]
fn harness_chat_when_defaulted_is_the_user_visible_chat_theme() {
    // Given: the default theme resolution path.
    // When: the default value and name are resolved.
    let from_default = Theme::default();
    let from_name = Theme::by_name("default");

    // Then: both paths select the Harness-owned chat configuration.
    assert_eq!(from_default, Theme::harness_chat());
    assert_eq!(from_name, Some(Theme::harness_chat()));
}

#[test]
fn harness_theme_names_resolve_without_replacing_harness_dark() {
    let chat = Theme::by_name("harness-chat");
    let compatibility = Theme::by_name("harness-dark");

    assert_eq!(chat, Some(Theme::harness_chat()));
    assert_eq!(compatibility, Some(Theme::harness_dark()));
}

#[test]
fn harness_chat_surfaces_when_constructed_match_observed_pixels() {
    // Given: the Harness chat constructor.
    // When: its surface tokens are read.
    let theme = Theme::harness_chat();

    // Then: the surface ramp matches the observation receipt.
    assert_eq!(theme.surface.canvas, Color::Rgb(20, 20, 20));
    assert_eq!(theme.surface.shell, Color::Rgb(20, 20, 20));
    assert_eq!(theme.surface.panel, Color::Rgb(20, 20, 20));
    assert_eq!(theme.surface.panel_elevated, Color::Rgb(28, 28, 28));
    assert_eq!(theme.surface.overlay, Color::Rgb(20, 20, 20));
    assert_eq!(theme.surface.card, Color::Rgb(85, 87, 83));
    assert_eq!(theme.surface.selected_card, Color::Rgb(85, 87, 83));
}

#[test]
fn harness_chat_measured_shell_colors_are_observation_locked() {
    let theme = Theme::harness_chat();

    assert_eq!(theme.border.subtle, Color::Rgb(58, 61, 67));
    assert_eq!(theme.border.strong, Color::Rgb(72, 75, 82));
    assert_eq!(theme.border.focus, Color::Rgb(96, 99, 106));
    assert_eq!(theme.text.primary, Color::Rgb(238, 238, 236));
    assert_eq!(theme.text.secondary, Color::Rgb(136, 139, 145));
    assert_eq!(theme.text.tertiary, Color::Rgb(136, 139, 145));
    assert_eq!(theme.status.disabled, Color::Rgb(128, 128, 128));
}

#[test]
fn harness_chat_content_tokens_inherit_harness_dark_and_measured_scrollbar() {
    let theme = Theme::harness_chat();
    let base = Theme::harness_dark();

    assert_eq!(theme.markdown, base.markdown);
    assert_eq!(theme.agents, base.agents);
    assert_eq!(theme.scrollbar.track, Color::Rgb(20, 20, 20));
    assert_eq!(theme.scrollbar.thumb, Color::Rgb(36, 36, 36));
    assert_eq!(theme.scrollbar.thumb_active, base.scrollbar.thumb_active);
}
