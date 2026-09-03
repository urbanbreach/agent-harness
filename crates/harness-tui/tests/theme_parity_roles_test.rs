#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast assertions for contract fixtures"
)]

use harness_tui::theme_system::{
    auto::{ThemeChoice, ThemeEnvironment},
    fallback::ColorLevel,
    roles::PaletteRole,
};
use ratatui::style::Color;

fn rgb_luminance(color: Color) -> u32 {
    match color {
        Color::Rgb(red, green, blue) => {
            u32::from(red) * 2126 + u32::from(green) * 7152 + u32::from(blue) * 722
        }
        other => panic!("expected truecolor default theme role, got {other:?}"),
    }
}

#[test]
fn default_resolved_theme_role_table_preserves_contrast_and_elevation() {
    // arrange
    let resolved =
        ThemeChoice::default().resolve(&ThemeEnvironment::with_color_level(ColorLevel::TrueColor));
    let roles = [
        PaletteRole::SurfaceCanvas,
        PaletteRole::SurfacePanelElevated,
        PaletteRole::SurfaceCard,
        PaletteRole::BorderSubtle,
        PaletteRole::BorderStrong,
        PaletteRole::BorderFocus,
        PaletteRole::TextSecondary,
        PaletteRole::TextAccent,
    ];

    // act
    let role_table = roles
        .map(|role| format!("{} = {:?}", role.label(), resolved.palette.color(role)))
        .join("\n");
    let background = resolved.palette.color(PaletteRole::SurfaceCanvas);
    let surface = resolved.palette.color(PaletteRole::SurfacePanelElevated);
    let raised = resolved.palette.color(PaletteRole::SurfaceCard);
    let surfaces = [background, surface, raised];
    let borders = [
        resolved.palette.color(PaletteRole::BorderSubtle),
        resolved.palette.color(PaletteRole::BorderStrong),
        resolved.palette.color(PaletteRole::BorderFocus),
    ];
    let muted_text = resolved.palette.color(PaletteRole::TextSecondary);
    let accent = resolved.palette.color(PaletteRole::TextAccent);

    // assert
    insta::assert_snapshot!(role_table, @r###"
    surface.canvas = Rgb(11, 14, 20)
    surface.panel_elevated = Rgb(18, 22, 30)
    surface.card = Rgb(85, 87, 83)
    border.subtle = Rgb(58, 61, 67)
    border.strong = Rgb(72, 75, 82)
    border.focus = Rgb(96, 99, 106)
    text.secondary = Rgb(136, 139, 145)
    text.accent = Rgb(217, 132, 217)
    "###);
    assert!(surfaces.iter().all(|color| !borders.contains(color)));
    assert!(borders.iter().all(|color| *color != muted_text));
    assert!(rgb_luminance(background) < rgb_luminance(surface));
    assert!(rgb_luminance(surface) < rgb_luminance(raised));
    assert!(!surfaces.contains(&accent));
    assert!(!borders.contains(&accent));
    assert_ne!(accent, muted_text);
}
