use crate::theme::Theme;

use super::bindings::SemanticThemeColors;
use super::focus::{BorderPalette, FocusPalette};
use super::glyphs::GlyphPalette;
use super::palette::Palette;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ThemeFamily {
    #[default]
    HarnessDark,
    HarnessLight,
    HighContrast,
    TerminalNative,
}

impl ThemeFamily {
    pub const ALL: [Self; 4] = [
        Self::HarnessDark,
        Self::HarnessLight,
        Self::HighContrast,
        Self::TerminalNative,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::HarnessDark => "harness-dark",
            Self::HarnessLight => "harness-light",
            Self::HighContrast => "high-contrast",
            Self::TerminalNative => "terminal-native",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        match label.trim().to_ascii_lowercase().as_str() {
            "default" | "harness-dark" | "harness_dark" | "dark" => Some(Self::HarnessDark),
            "harness-light" | "harness_light" | "light" => Some(Self::HarnessLight),
            "high-contrast" | "high_contrast" => Some(Self::HighContrast),
            "terminal-native" | "terminal_native" | "terminal" => Some(Self::TerminalNative),
            _ => None,
        }
    }

    pub const fn is_dark(self) -> bool {
        match self {
            Self::HarnessDark | Self::HighContrast | Self::TerminalNative => true,
            Self::HarnessLight => false,
        }
    }

    pub fn theme(self) -> Theme {
        match self {
            Self::HarnessDark => Theme::harness_dark(),
            Self::HarnessLight => Theme::harness_light(),
            Self::HighContrast => Theme::harness_high_contrast(),
            Self::TerminalNative => Theme::terminal_native(),
        }
    }

    pub fn palette(self) -> Palette {
        Palette::from_theme(&self.theme())
    }

    pub fn glyphs(self) -> GlyphPalette {
        GlyphPalette::from_theme(&self.theme())
    }

    pub fn borders(self) -> BorderPalette {
        BorderPalette::from_theme(&self.theme())
    }

    pub fn focus(self) -> FocusPalette {
        FocusPalette::from_theme(&self.theme())
    }

    pub fn semantic(self) -> SemanticThemeColors {
        SemanticThemeColors::from_theme(&self.theme())
    }
}
