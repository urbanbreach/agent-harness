use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellGeometryTarget {
    Minimum,
    Primary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellGeometry {
    pub width: u16,
    pub height: u16,
}

impl ShellGeometry {
    pub const MINIMUM: Self = Self {
        width: 80,
        height: 24,
    };

    pub const PRIMARY: Self = Self {
        width: 100,
        height: 30,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellHeights {
    pub header: u16,
    pub tabs: u16,
    pub status: u16,
    pub footer: u16,
    pub prompt_input: u16,
    pub permission_modal: u16,
}

impl ShellHeights {
    pub const fn prompt_block(self) -> u16 {
        self.prompt_input + 2
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellRhythm {
    pub transcript_gutter_x: u16,
    pub transcript_gutter_y: u16,
    pub status_separator: u16,
    pub modal_margin: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusGlyphs {
    pub streaming: &'static str,
    pub done: &'static str,
    pub error: &'static str,
    pub pending_permission: &'static str,
    pub queued: &'static str,
    pub running: &'static str,
    pub succeeded: &'static str,
    pub failed: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveShellLayout {
    pub target: ShellGeometryTarget,
    pub activity_drawer_width: u16,
    pub inspector_drawer_width: u16,
    pub transcript_min_width: u16,
    pub permission_modal_width: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveShellTokens {
    pub minimum: LiveShellLayout,
    pub primary: LiveShellLayout,
    pub heights: ShellHeights,
    pub rhythm: ShellRhythm,
    pub glyphs: StatusGlyphs,
}

impl LiveShellTokens {
    pub const fn select(self, width: u16, height: u16) -> LiveShellLayout {
        if width >= ShellGeometry::PRIMARY.width && height >= ShellGeometry::PRIMARY.height {
            self.primary
        } else {
            self.minimum
        }
    }

    pub const fn target(self, width: u16, height: u16) -> ShellGeometryTarget {
        self.select(width, height).target
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeColors {
    pub canvas: Color,
    pub border: Color,
    pub focus_border: Color,
    pub title: Color,
    pub header_bg: Color,
    pub header_fg: Color,
    pub footer_bg: Color,
    pub footer_fg: Color,
    pub modal_bg: Color,
    pub modal_border: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextColors {
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub selected_bg: Color,
    pub selected_fg: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusColors {
    pub success: Color,
    pub error: Color,
    pub warning: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub chrome: ChromeColors,
    pub text: TextColors,
    pub status: StatusColors,
    pub live_shell: LiveShellTokens,
}

impl Theme {
    const OPENCODE_SHELL: LiveShellTokens = LiveShellTokens {
        minimum: LiveShellLayout {
            target: ShellGeometryTarget::Minimum,
            activity_drawer_width: 20,
            inspector_drawer_width: 20,
            transcript_min_width: 28,
            permission_modal_width: 48,
        },
        primary: LiveShellLayout {
            target: ShellGeometryTarget::Primary,
            activity_drawer_width: 24,
            inspector_drawer_width: 28,
            transcript_min_width: 40,
            permission_modal_width: 56,
        },
        heights: ShellHeights {
            header: 1,
            tabs: 3,
            status: 1,
            footer: 1,
            prompt_input: 3,
            permission_modal: 7,
        },
        rhythm: ShellRhythm {
            transcript_gutter_x: 1,
            transcript_gutter_y: 0,
            status_separator: 3,
            modal_margin: 2,
        },
        glyphs: StatusGlyphs {
            streaming: "◐",
            done: "●",
            error: "✗",
            pending_permission: "◷",
            queued: "◴",
            running: "◐",
            succeeded: "●",
            failed: "✗",
        },
    };

    pub fn mono() -> Self {
        Self {
            chrome: ChromeColors {
                canvas: Color::Black,
                border: Color::Rgb(0x88, 0x70, 0x30),
                focus_border: Color::Rgb(0xFF, 0xD0, 0x40),
                title: Color::Rgb(0xFF, 0xB0, 0x00),
                header_bg: Color::Rgb(0x22, 0x18, 0x00),
                header_fg: Color::Rgb(0xFF, 0xB0, 0x00),
                footer_bg: Color::Rgb(0x22, 0x18, 0x00),
                footer_fg: Color::Rgb(0xCC, 0x90, 0x00),
                modal_bg: Color::Rgb(0x22, 0x18, 0x00),
                modal_border: Color::Rgb(0xFF, 0xB0, 0x00),
            },
            text: TextColors {
                primary: Color::Rgb(0xFF, 0xB0, 0x00),
                secondary: Color::Rgb(0xCC, 0x90, 0x00),
                accent: Color::Rgb(0xFF, 0xD0, 0x40),
                selected_bg: Color::Rgb(0x33, 0x22, 0x00),
                selected_fg: Color::Rgb(0xFF, 0xD0, 0x40),
            },
            status: StatusColors {
                success: Color::Rgb(0x50, 0xD0, 0x50),
                error: Color::Rgb(0xD0, 0x50, 0x50),
                warning: Color::Rgb(0xD0, 0xA0, 0x30),
            },
            live_shell: Self::OPENCODE_SHELL,
        }
    }

    pub fn opencode_dark() -> Self {
        Self {
            chrome: ChromeColors {
                canvas: Color::Rgb(0x0D, 0x11, 0x17),
                border: Color::Rgb(0x2B, 0x35, 0x41),
                focus_border: Color::Rgb(0x58, 0x9B, 0xF9),
                title: Color::Rgb(0x9E, 0xB7, 0xD9),
                header_bg: Color::Rgb(0x11, 0x16, 0x1E),
                header_fg: Color::Rgb(0x98, 0xA4, 0xB3),
                footer_bg: Color::Rgb(0x11, 0x16, 0x1E),
                footer_fg: Color::Rgb(0x7E, 0x89, 0x99),
                modal_bg: Color::Rgb(0x16, 0x1C, 0x24),
                modal_border: Color::Rgb(0x58, 0x9B, 0xF9),
            },
            text: TextColors {
                primary: Color::Rgb(0xE6, 0xE9, 0xEF),
                secondary: Color::Rgb(0xA0, 0xAB, 0xB8),
                accent: Color::Rgb(0x58, 0x9B, 0xF9),
                selected_bg: Color::Rgb(0x1A, 0x24, 0x30),
                selected_fg: Color::Rgb(0xFF, 0xFF, 0xFF),
            },
            status: StatusColors {
                success: Color::Rgb(0x3B, 0xA2, 0x55),
                error: Color::Rgb(0xE5, 0x53, 0x53),
                warning: Color::Rgb(0xD2, 0x9B, 0x3A),
            },
            live_shell: Self::OPENCODE_SHELL,
        }
    }

    pub fn default_theme() -> Self {
        Self::opencode_dark()
    }

    pub const fn live_shell_layout(self, width: u16, height: u16) -> LiveShellLayout {
        self.live_shell.select(width, height)
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
        assert_eq!(theme.chrome.canvas, Color::Black);
        assert_eq!(theme.text.primary, Color::Rgb(0xFF, 0xB0, 0x00));
    }

    #[test]
    fn opencode_dark_theme_has_valid_colors() {
        let theme = Theme::opencode_dark();
        assert_eq!(theme.chrome.canvas, Color::Rgb(0x0D, 0x11, 0x17));
        assert_eq!(theme.text.primary, Color::Rgb(0xE6, 0xE9, 0xEF));
    }

    #[test]
    fn default_theme_is_opencode_dark() {
        let default = Theme::default();
        let opencode = Theme::opencode_dark();
        assert_eq!(default.chrome.canvas, opencode.chrome.canvas);
        assert_eq!(default.text.primary, opencode.text.primary);
    }

    #[test]
    fn live_shell_tokens_choose_primary_geometry_at_signoff_size() {
        let theme = Theme::default();
        assert_eq!(
            theme.live_shell_layout(100, 30).target,
            ShellGeometryTarget::Primary
        );
        assert_eq!(
            theme.live_shell_layout(80, 24).target,
            ShellGeometryTarget::Minimum
        );
        assert_eq!(theme.live_shell.heights.status, 1);
    }
}
