use crate::theme::Theme;

pub use crate::theme::ColorLevel;

pub const FALLBACK_LADDER: [ColorLevel; 4] = [
    ColorLevel::TrueColor,
    ColorLevel::Ansi256,
    ColorLevel::Basic,
    ColorLevel::None,
];

pub fn fallback_theme(theme: Theme, level: ColorLevel) -> Theme {
    theme.for_color_level(level)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedTheme {
    pub requested: super::auto::ThemeChoice,
    pub family: super::family::ThemeFamily,
    pub color_level: ColorLevel,
    pub theme: Theme,
    pub palette: super::palette::Palette,
    pub glyphs: super::glyphs::GlyphPalette,
    pub borders: super::focus::BorderPalette,
    pub focus: super::focus::FocusPalette,
    pub bindings: super::bindings::SemanticThemeColors,
}
