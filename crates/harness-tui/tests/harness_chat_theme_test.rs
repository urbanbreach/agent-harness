use harness_tui::theme::Theme;
use harness_tui::theme_system::{
    auto::{ThemeChoice, ThemeEnvironment},
    fallback::ColorLevel,
    family::ThemeFamily,
    roles::PaletteRole,
};
use ratatui::style::Color;

#[test]
fn harness_chat_maps_every_visible_palette_role_to_groknight_truecolor() {
    let resolved = ThemeChoice::explicit(ThemeFamily::HarnessChat)
        .resolve(&ThemeEnvironment::with_color_level(ColorLevel::TrueColor));
    let expected = [
        Color::Rgb(20, 20, 20),
        Color::Rgb(20, 20, 20),
        Color::Rgb(20, 20, 20),
        Color::Rgb(28, 28, 28),
        Color::Rgb(20, 20, 20),
        Color::Rgb(36, 36, 36),
        Color::Rgb(85, 87, 83),
        Color::Rgb(50, 50, 55),
        Color::Rgb(60, 60, 65),
        Color::Rgb(80, 80, 88),
        Color::Rgb(225, 225, 225),
        Color::Rgb(108, 108, 108),
        Color::Rgb(88, 88, 88),
        Color::Rgb(187, 154, 247),
        Color::Rgb(20, 20, 20),
        Color::Rgb(36, 36, 36),
        Color::Rgb(54, 54, 54),
        Color::Rgb(225, 225, 225),
        Color::Rgb(200, 200, 200),
        Color::Rgb(108, 108, 108),
        Color::Rgb(158, 206, 106),
        Color::Rgb(224, 175, 104),
        Color::Rgb(247, 118, 142),
        Color::Rgb(125, 207, 255),
        Color::Rgb(88, 88, 88),
        Color::Rgb(26, 188, 156),
        Color::Rgb(122, 162, 247),
        Color::Rgb(157, 124, 216),
        Color::Rgb(120, 120, 120),
        Color::Rgb(108, 108, 108),
        Color::Rgb(90, 90, 90),
        Color::Rgb(122, 162, 247),
        Color::Rgb(122, 166, 218),
        Color::Rgb(58, 149, 171),
        Color::Rgb(158, 206, 106),
        Color::Rgb(200, 200, 200),
        Color::Rgb(108, 108, 108),
        Color::Rgb(28, 28, 28),
        Color::Rgb(200, 200, 200),
        Color::Rgb(200, 200, 200),
        Color::Rgb(200, 200, 200),
        Color::Rgb(108, 108, 108),
        Color::Rgb(108, 108, 108),
        Color::Rgb(108, 108, 108),
        Color::Rgb(108, 108, 108),
        Color::Rgb(122, 162, 247),
        Color::Rgb(187, 154, 247),
        Color::Rgb(224, 175, 104),
        Color::Rgb(125, 207, 255),
        Color::Rgb(17, 17, 17),
        Color::Rgb(36, 36, 36),
        Color::Rgb(80, 80, 88),
    ];

    assert_eq!(PaletteRole::ALL.len(), expected.len());
    for (role, expected_color) in PaletteRole::ALL.into_iter().zip(expected) {
        assert_eq!(
            resolved.palette.color(role),
            expected_color,
            "HarnessChat role {} must use its GrokNight mapping",
            role.label()
        );
    }
}

#[test]
fn harness_chat_maps_complete_groknight_markdown_role_set() {
    let markdown = Theme::harness_chat().markdown;

    assert_eq!(
        [
            markdown.heading_h1,
            markdown.heading_h2,
            markdown.heading_h3,
            markdown.heading_h4,
            markdown.heading_h5,
            markdown.heading_h6,
            markdown.code,
            markdown.task_checked,
            markdown.task_unchecked,
            markdown.muted,
            markdown.code_background,
            markdown.text,
            markdown.link_text,
        ],
        [
            Color::Rgb(26, 188, 156),
            Color::Rgb(122, 162, 247),
            Color::Rgb(157, 124, 216),
            Color::Rgb(120, 120, 120),
            Color::Rgb(108, 108, 108),
            Color::Rgb(90, 90, 90),
            Color::Rgb(58, 149, 171),
            Color::Rgb(158, 206, 106),
            Color::Rgb(200, 200, 200),
            Color::Rgb(108, 108, 108),
            Color::Rgb(28, 28, 28),
            Color::Rgb(200, 200, 200),
            Color::Rgb(122, 166, 218),
        ]
    );
}

#[test]
fn harness_chat_maps_terminal_and_diff_roles_to_groknight_truecolor() {
    let colors = Theme::harness_chat().reference_terminal;
    assert_eq!(
        [
            colors.canvas,
            colors.primary,
            colors.secondary,
            colors.muted,
            colors.prompt_border,
            colors.prompt_border_active,
            colors.prompt_accent,
            colors.active_prompt_surface,
            colors.error,
            colors.palette_section,
            colors.fork_accent,
            colors.assistant_error,
            colors.diff_added,
            colors.diff_removed,
            colors.diff_added_gutter,
            colors.diff_removed_gutter,
            colors.diff_added_highlight,
            colors.diff_removed_highlight,
            colors.diff_hunk_header,
        ],
        [
            Color::Rgb(20, 20, 20),
            Color::Rgb(225, 225, 225),
            Color::Rgb(108, 108, 108),
            Color::Rgb(88, 88, 88),
            Color::Rgb(50, 50, 55),
            Color::Rgb(80, 80, 88),
            Color::Rgb(200, 200, 200),
            Color::Rgb(38, 38, 38),
            Color::Rgb(247, 118, 142),
            Color::Rgb(187, 154, 247),
            Color::Rgb(255, 158, 100),
            Color::Rgb(108, 108, 108),
            Color::Rgb(6, 56, 6),
            Color::Rgb(66, 14, 20),
            Color::Rgb(6, 56, 6),
            Color::Rgb(66, 14, 20),
            Color::Rgb(158, 206, 106),
            Color::Rgb(247, 118, 142),
            Color::Rgb(122, 162, 247),
        ]
    );
}

#[test]
fn harness_chat_maps_custom_agent_palette_to_groknight_accents() {
    assert_eq!(
        Theme::harness_chat().agents.palette,
        [
            Color::Rgb(122, 162, 247),
            Color::Rgb(187, 154, 247),
            Color::Rgb(158, 206, 106),
            Color::Rgb(224, 175, 104),
            Color::Rgb(255, 158, 100),
            Color::Rgb(247, 118, 142),
            Color::Rgb(125, 207, 255),
        ]
    );
}

#[test]
fn dark_family_truecolor_resolves_to_harness_chat() {
    assert_eq!(
        Theme::from_family(
            harness_tui::theme_family::ThemeFamily::Dark,
            ColorLevel::TrueColor
        ),
        Theme::harness_chat()
    );
}
