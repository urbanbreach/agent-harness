//! Theme leaf types for the TUI responsive/terminal shard.
//!
//! These are plain value objects with no shared registry or app-state
//! dependency. They capture the named theme catalog, auto mode, and
//! reduced-capability theme selection that the manifest theme rows require.

/// Named theme identifiers matching the TUI theme catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum NamedTheme {
    /// Harness chat (default GrokNight role system).
    #[default]
    HarnessChat,
    /// Legacy Harness dark.
    HarnessDark,
    /// Harness light.
    HarnessLight,
    /// High contrast.
    HighContrast,
    /// Terminal-native: uses `Color::Reset` + named ANSI accents.
    /// Matches the reference binary's terminal-native mode where the
    /// core shell defers to the terminal's own fg/bg.
    TerminalNative,
}

impl NamedTheme {
    pub const ALL: [Self; 5] = [
        Self::HarnessChat,
        Self::HarnessDark,
        Self::HarnessLight,
        Self::HighContrast,
        Self::TerminalNative,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::HarnessChat => "harness-chat",
            Self::HarnessDark => "harness-dark",
            Self::HarnessLight => "harness-light",
            Self::HighContrast => "high-contrast",
            Self::TerminalNative => "terminal-native",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        match label.trim().to_ascii_lowercase().as_str() {
            "default" | "harness-chat" | "harness_chat" | "chat" => Some(Self::HarnessChat),
            "harness-dark" | "dark" => Some(Self::HarnessDark),
            "harness-light" | "light" => Some(Self::HarnessLight),
            "high-contrast" | "high_contrast" => Some(Self::HighContrast),
            "terminal-native" | "terminal_native" => Some(Self::TerminalNative),
            _ => None,
        }
    }

    /// Whether this theme uses only `Color::Reset` and named ANSI colors
    /// (no RGB or indexed). Used to determine if color quantization is
    /// needed.
    pub const fn is_terminal_native(self) -> bool {
        matches!(self, Self::TerminalNative)
    }
}

/// Theme auto-detection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeAutoMode {
    /// Theme is explicitly selected (no auto detection).
    #[default]
    Explicit,
    /// Theme is auto-detected from terminal environment.
    Auto,
}

/// Theme leaf — a pure value type for theme selection and auto mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeLeaf {
    pub theme: NamedTheme,
    pub auto_mode: ThemeAutoMode,
    /// True when the theme was selected due to reduced color capability.
    pub reduced_capability: bool,
}

impl ThemeLeaf {
    /// Default theme: harness-chat, explicit, no reduced capability.
    pub const fn default_theme() -> Self {
        Self {
            theme: NamedTheme::HarnessChat,
            auto_mode: ThemeAutoMode::Explicit,
            reduced_capability: false,
        }
    }

    /// Auto-detect theme from terminal environment (pure; no I/O).
    ///
    /// When the terminal advertises truecolor, selects HarnessChat with
    /// full color capability. When the terminal is dumb or has no color,
    /// selects HighContrast with reduced capability. Otherwise selects
    /// HarnessChat with reduced capability (256-color or less).
    pub fn auto_from_env(colorterm: Option<&str>, term: Option<&str>) -> Self {
        let lower_term = term.unwrap_or("").to_ascii_lowercase();
        let is_dumb = lower_term == "dumb";
        let has_truecolor = colorterm
            .map(|ct| {
                let lower = ct.to_ascii_lowercase();
                lower.contains("truecolor") || lower.contains("24bit")
            })
            .unwrap_or(false);

        Self {
            theme: if is_dumb {
                NamedTheme::HighContrast
            } else {
                NamedTheme::HarnessChat
            },
            auto_mode: ThemeAutoMode::Auto,
            reduced_capability: is_dumb || !has_truecolor,
        }
    }

    /// Select a theme explicitly, clearing auto mode.
    pub const fn explicit(theme: NamedTheme) -> Self {
        Self {
            theme,
            auto_mode: ThemeAutoMode::Explicit,
            reduced_capability: false,
        }
    }

    /// Select a reduced-capability theme (high contrast) for legacy terminals.
    pub const fn reduced() -> Self {
        Self {
            theme: NamedTheme::HighContrast,
            auto_mode: ThemeAutoMode::Explicit,
            reduced_capability: true,
        }
    }
}

impl Default for ThemeLeaf {
    fn default() -> Self {
        Self::default_theme()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_themes_have_unique_labels() {
        // arrange
        // act
        let labels: Vec<&str> = NamedTheme::ALL.iter().map(|t| t.label()).collect();

        // assert
        assert_eq!(labels.len(), 5);
        assert_eq!(
            labels
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            5
        );
    }

    #[test]
    fn from_label_resolves_known_themes() {
        // arrange
        // act
        // assert
        assert_eq!(
            NamedTheme::from_label("default"),
            Some(NamedTheme::HarnessChat)
        );
        assert_eq!(
            NamedTheme::from_label("harness-chat"),
            Some(NamedTheme::HarnessChat)
        );
        assert_eq!(
            NamedTheme::from_label("harness-dark"),
            Some(NamedTheme::HarnessDark)
        );
        assert_eq!(
            NamedTheme::from_label("dark"),
            Some(NamedTheme::HarnessDark)
        );
        assert_eq!(
            NamedTheme::from_label("HARNESS-LIGHT"),
            Some(NamedTheme::HarnessLight)
        );
        assert_eq!(
            NamedTheme::from_label("high-contrast"),
            Some(NamedTheme::HighContrast)
        );
        assert_eq!(
            NamedTheme::from_label("terminal-native"),
            Some(NamedTheme::TerminalNative)
        );
        assert_eq!(
            NamedTheme::from_label("terminal_native"),
            Some(NamedTheme::TerminalNative)
        );
        assert_eq!(NamedTheme::from_label("unknown"), None);
    }

    #[test]
    fn terminal_native_is_terminal_native() {
        // arrange
        // act
        // assert
        assert!(NamedTheme::TerminalNative.is_terminal_native());
        assert!(!NamedTheme::HarnessDark.is_terminal_native());
    }

    #[test]
    fn default_theme_is_harness_chat_explicit() {
        // arrange
        // act
        let leaf = ThemeLeaf::default_theme();

        // assert
        assert_eq!(leaf.theme, NamedTheme::HarnessChat);
        assert_eq!(leaf.auto_mode, ThemeAutoMode::Explicit);
        assert!(!leaf.reduced_capability);
    }

    #[test]
    fn auto_from_env_detects_truecolor() {
        // arrange
        // act
        let leaf = ThemeLeaf::auto_from_env(Some("truecolor"), Some("xterm-256color"));

        // assert
        assert_eq!(leaf.theme, NamedTheme::HarnessChat);
        assert_eq!(leaf.auto_mode, ThemeAutoMode::Auto);
        assert!(!leaf.reduced_capability);
    }

    #[test]
    fn auto_from_env_marks_reduced_for_dumb_terminal() {
        // arrange
        // act
        let leaf = ThemeLeaf::auto_from_env(None, Some("dumb"));

        // assert
        assert_eq!(leaf.theme, NamedTheme::HighContrast);
        assert!(leaf.reduced_capability);
    }

    #[test]
    fn auto_from_env_marks_reduced_without_truecolor() {
        // arrange
        // act
        let leaf = ThemeLeaf::auto_from_env(None, Some("xterm-256color"));

        // assert
        assert!(leaf.reduced_capability);
    }

    #[test]
    fn explicit_theme_clears_auto_mode() {
        // arrange
        // act
        let leaf = ThemeLeaf::explicit(NamedTheme::HarnessLight);

        // assert
        assert_eq!(leaf.theme, NamedTheme::HarnessLight);
        assert_eq!(leaf.auto_mode, ThemeAutoMode::Explicit);
        assert!(!leaf.reduced_capability);
    }

    #[test]
    fn reduced_theme_is_high_contrast() {
        // arrange
        // act
        let leaf = ThemeLeaf::reduced();

        // assert
        assert_eq!(leaf.theme, NamedTheme::HighContrast);
        assert!(leaf.reduced_capability);
    }
}
