//! Theme system for the TUI with multiple color palettes.
//!
//! Provides two palettes:
//! - `mono`: Pi-like monochrome theme (amber/green on black)
//! - `opencode_dark`: High-contrast dark theme

use ratatui::style::Color;

/// A color palette for the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Background color for the main interface
    pub bg: Color,
    /// Foreground/text color
    pub fg: Color,
    /// Background for selected items
    pub selected_bg: Color,
    /// Foreground for selected items
    pub selected_fg: Color,
    /// Border color
    pub border: Color,
    /// Title/border text color
    pub title: Color,
    /// Header background
    pub header_bg: Color,
    /// Header foreground
    pub header_fg: Color,
    /// Footer/status bar background
    pub footer_bg: Color,
    /// Footer/status bar foreground
    pub footer_fg: Color,
    /// Accent color for highlights
    pub accent: Color,
    /// Success/allow color
    pub success: Color,
    /// Error/deny color
    pub error: Color,
    /// Warning color
    pub warning: Color,
    /// Modal/dialog background
    pub modal_bg: Color,
    /// Modal border color
    pub modal_border: Color,
}

impl Theme {
    /// Monochrome Pi-like theme (amber on black)
    pub fn mono() -> Self {
        Self {
            bg: Color::Black,
            fg: Color::Rgb(0xFF, 0xB0, 0x00), // Amber
            selected_bg: Color::Rgb(0x33, 0x22, 0x00),
            selected_fg: Color::Rgb(0xFF, 0xD0, 0x40),
            border: Color::Rgb(0x88, 0x70, 0x30),
            title: Color::Rgb(0xFF, 0xB0, 0x00),
            header_bg: Color::Rgb(0x22, 0x18, 0x00),
            header_fg: Color::Rgb(0xFF, 0xB0, 0x00),
            footer_bg: Color::Rgb(0x22, 0x18, 0x00),
            footer_fg: Color::Rgb(0xCC, 0x90, 0x00),
            accent: Color::Rgb(0xFF, 0xD0, 0x40),
            success: Color::Rgb(0x50, 0xD0, 0x50),
            error: Color::Rgb(0xD0, 0x50, 0x50),
            warning: Color::Rgb(0xD0, 0xA0, 0x30),
            modal_bg: Color::Rgb(0x22, 0x18, 0x00),
            modal_border: Color::Rgb(0xFF, 0xB0, 0x00),
        }
    }

    /// OpenCode-inspired high-contrast dark theme
    pub fn opencode_dark() -> Self {
        Self {
            bg: Color::Rgb(0x0D, 0x11, 0x17), // Deep slate
            fg: Color::Rgb(0xE6, 0xE9, 0xEF), // Light gray-white
            selected_bg: Color::Rgb(0x1F, 0x29, 0x36),
            selected_fg: Color::Rgb(0xFF, 0xFF, 0xFF),
            border: Color::Rgb(0x4A, 0x55, 0x66),
            title: Color::Rgb(0x7D, 0xB0, 0xFF), // Soft blue
            header_bg: Color::Rgb(0x16, 0x1B, 0x22),
            header_fg: Color::Rgb(0xA0, 0xB0, 0xC0),
            footer_bg: Color::Rgb(0x16, 0x1B, 0x22),
            footer_fg: Color::Rgb(0x88, 0x96, 0xA8),
            accent: Color::Rgb(0x58, 0x9B, 0xF9), // Bright blue
            success: Color::Rgb(0x3B, 0xA2, 0x55),
            error: Color::Rgb(0xE5, 0x53, 0x53),
            warning: Color::Rgb(0xD2, 0x9B, 0x3A),
            modal_bg: Color::Rgb(0x1A, 0x20, 0x28),
            modal_border: Color::Rgb(0x58, 0x9B, 0xF9),
        }
    }

    /// Get the default theme
    pub fn default_theme() -> Self {
        Self::opencode_dark()
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::default_theme()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_theme_has_valid_colors() {
        let theme = Theme::mono();
        assert_eq!(theme.bg, Color::Black);
        assert_eq!(theme.fg, Color::Rgb(0xFF, 0xB0, 0x00));
    }

    #[test]
    fn opencode_dark_theme_has_valid_colors() {
        let theme = Theme::opencode_dark();
        assert_eq!(theme.bg, Color::Rgb(0x0D, 0x11, 0x17));
        assert_eq!(theme.fg, Color::Rgb(0xE6, 0xE9, 0xEF));
    }

    #[test]
    fn default_theme_is_opencode_dark() {
        let default = Theme::default();
        let opencode = Theme::opencode_dark();
        assert_eq!(default.bg, opencode.bg);
        assert_eq!(default.fg, opencode.fg);
    }
}
