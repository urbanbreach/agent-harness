use super::auto::{ThemeChoice, ThemeEnvironment};
use super::fallback::{fallback_theme, ResolvedTheme};
use super::family::ThemeFamily;

impl ThemeChoice {
    pub fn resolve(self, environment: &ThemeEnvironment) -> ResolvedTheme {
        let family = match self {
            Self::Explicit(family) => family,
            Self::Auto => match environment.system_appearance() {
                Some(super::auto::SystemAppearance::Light) => ThemeFamily::HarnessLight,
                Some(super::auto::SystemAppearance::Dark) | None => ThemeFamily::HarnessDark,
            },
        };
        let base = family.theme();
        let color_level = environment.color_level();
        let theme = match family {
            ThemeFamily::TerminalNative => base,
            ThemeFamily::HarnessDark | ThemeFamily::HarnessLight | ThemeFamily::HighContrast => {
                fallback_theme(base, color_level)
            }
        };
        ResolvedTheme {
            requested: self,
            family,
            color_level,
            palette: super::palette::Palette::from_theme(&theme),
            glyphs: super::glyphs::GlyphPalette::from_theme(&theme),
            borders: super::focus::BorderPalette::from_theme(&theme),
            focus: super::focus::FocusPalette::from_theme(&theme),
            bindings: super::bindings::SemanticThemeColors::from_theme(&theme),
            theme,
        }
    }
}
