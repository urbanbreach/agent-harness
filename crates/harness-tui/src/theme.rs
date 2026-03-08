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
pub struct EmptyStatePrompt {
    pub prompt: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyStateTokens {
    pub max_width: u16,
    pub value_prop: &'static str,
    pub example_prompts: [EmptyStatePrompt; 3],
    pub demo_mode_label: &'static str,
    pub mock_mode_label: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveShellLayout {
    pub target: ShellGeometryTarget,
    pub activity_drawer_width: u16,
    pub inspector_drawer_width: u16,
    pub transcript_min_width: u16,
    pub permission_modal_width: u16,
    pub centered_content_width: u16,
    pub content_margin_x: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveShellTokens {
    pub minimum: LiveShellLayout,
    pub primary: LiveShellLayout,
    pub heights: ShellHeights,
    pub rhythm: ShellRhythm,
    pub glyphs: StatusGlyphs,
    pub empty_state: EmptyStateTokens,
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
            centered_content_width: 78,
            content_margin_x: 1,
        },
        primary: LiveShellLayout {
            target: ShellGeometryTarget::Primary,
            activity_drawer_width: 24,
            inspector_drawer_width: 28,
            transcript_min_width: 40,
            permission_modal_width: 56,
            centered_content_width: 92,
            content_margin_x: 2,
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
            status_separator: 2,
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
        empty_state: EmptyStateTokens {
            max_width: 62,
            value_prop: "Inspect code, make edits, or explain failures.",
            example_prompts: [
                EmptyStatePrompt {
                    prompt: "inspect src/ui.rs",
                },
                EmptyStatePrompt {
                    prompt: "trace the failing test",
                },
                EmptyStatePrompt {
                    prompt: "review the current diff",
                },
            ],
            demo_mode_label: "Demo mode · mock provider",
            mock_mode_label: "Mock mode · mock provider",
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
                border: Color::Rgb(0x30, 0x39, 0x46),
                focus_border: Color::Rgb(0x78, 0x98, 0xC0),
                title: Color::Rgb(0x8E, 0x9C, 0xAE),
                header_bg: Color::Rgb(0x0F, 0x14, 0x1B),
                header_fg: Color::Rgb(0x8C, 0x99, 0xA9),
                footer_bg: Color::Rgb(0x0F, 0x14, 0x1B),
                footer_fg: Color::Rgb(0x73, 0x7F, 0x8E),
                modal_bg: Color::Rgb(0x14, 0x1A, 0x22),
                modal_border: Color::Rgb(0x78, 0x98, 0xC0),
            },
            text: TextColors {
                primary: Color::Rgb(0xE6, 0xE9, 0xEF),
                secondary: Color::Rgb(0x9A, 0xA5, 0xB3),
                accent: Color::Rgb(0x78, 0x98, 0xC0),
                selected_bg: Color::Rgb(0x18, 0x20, 0x29),
                selected_fg: Color::Rgb(0xFF, 0xFF, 0xFF),
            },
            status: StatusColors {
                success: Color::Rgb(0x55, 0xA4, 0x6A),
                error: Color::Rgb(0xE5, 0x53, 0x53),
                warning: Color::Rgb(0xC7, 0x97, 0x4D),
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
        let minimum = theme.live_shell_layout(80, 24);
        let primary = theme.live_shell_layout(100, 30);

        assert_eq!(primary.target, ShellGeometryTarget::Primary);
        assert_eq!(minimum.target, ShellGeometryTarget::Minimum);
        assert_eq!(minimum.centered_content_width, 78);
        assert_eq!(minimum.content_margin_x, 1);
        assert_eq!(primary.centered_content_width, 92);
        assert_eq!(primary.content_margin_x, 2);
        assert_eq!(theme.live_shell.rhythm.status_separator, 2);
        assert_eq!(theme.live_shell.heights.status, 1);

        assert!(minimum.centered_content_width + minimum.content_margin_x.saturating_mul(2) <= 80);
        assert!(primary.centered_content_width + primary.content_margin_x.saturating_mul(2) <= 100);
    }
}
