use ratatui::style::Color;

const fn rgb(red: u8, green: u8, blue: u8) -> Color {
    Color::Rgb(red, green, blue)
}

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
    pub surface_margin_x: u16,
    pub surface_margin_y: u16,
    pub surface_gap: u16,
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
pub struct TranscriptGlyphs {
    pub user_marker: &'static str,
    pub card_top: &'static str,
    pub card_mid: &'static str,
    pub card_bottom: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyStatePrompt {
    pub prompt: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyStateTokens {
    pub max_width: u16,
    pub title: &'static str,
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
    pub details_sidebar_width: u16,
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
    pub transcript_glyphs: TranscriptGlyphs,
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
pub struct SurfaceColors {
    pub canvas: Color,
    pub shell: Color,
    pub panel: Color,
    pub panel_elevated: Color,
    pub overlay: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BorderColors {
    pub subtle: Color,
    pub strong: Color,
    pub focus: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextColors {
    pub primary: Color,
    pub secondary: Color,
    pub tertiary: Color,
    pub accent: Color,
    pub inverse: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusColors {
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,
    pub disabled: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub surface: SurfaceColors,
    pub border: BorderColors,
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
            details_sidebar_width: 34,
            transcript_min_width: 28,
            permission_modal_width: 48,
            centered_content_width: 78,
            content_margin_x: 1,
        },
        primary: LiveShellLayout {
            target: ShellGeometryTarget::Primary,
            activity_drawer_width: 24,
            inspector_drawer_width: 28,
            details_sidebar_width: 40,
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
            surface_margin_x: 2,
            surface_margin_y: 1,
            surface_gap: 1,
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
        transcript_glyphs: TranscriptGlyphs {
            user_marker: "›",
            card_top: "╭─",
            card_mid: "│",
            card_bottom: "╰─",
        },
        empty_state: EmptyStateTokens {
            max_width: 62,
            title: "Harness",
            value_prop: "Start a conversation to begin",
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
            surface: SurfaceColors {
                canvas: Color::Black,
                shell: rgb(0x14, 0x10, 0x02),
                panel: rgb(0x18, 0x12, 0x04),
                panel_elevated: rgb(0x21, 0x18, 0x05),
                overlay: rgb(0x2B, 0x1F, 0x07),
            },
            border: BorderColors {
                subtle: rgb(0x88, 0x70, 0x30),
                strong: rgb(0xCC, 0x90, 0x00),
                focus: rgb(0xFF, 0xD0, 0x40),
            },
            text: TextColors {
                primary: rgb(0xFF, 0xB0, 0x00),
                secondary: rgb(0xCC, 0x90, 0x00),
                tertiary: rgb(0x88, 0x70, 0x30),
                accent: rgb(0xFF, 0xD0, 0x40),
                inverse: Color::Black,
            },
            status: StatusColors {
                success: rgb(0x50, 0xD0, 0x50),
                warning: rgb(0xD0, 0xA0, 0x30),
                error: rgb(0xD0, 0x50, 0x50),
                info: rgb(0x80, 0xB8, 0xFF),
                disabled: rgb(0x88, 0x70, 0x30),
            },
            live_shell: Self::OPENCODE_SHELL,
        }
    }

    pub fn harness_app_dark() -> Self {
        Self {
            surface: SurfaceColors {
                canvas: rgb(0x07, 0x0B, 0x12),
                shell: rgb(0x0D, 0x15, 0x22),
                panel: rgb(0x10, 0x1A, 0x29),
                panel_elevated: rgb(0x15, 0x22, 0x35),
                overlay: rgb(0x18, 0x27, 0x3B),
            },
            border: BorderColors {
                subtle: rgb(0x24, 0x35, 0x4B),
                strong: rgb(0x31, 0x47, 0x60),
                focus: rgb(0x6E, 0xA8, 0xFE),
            },
            text: TextColors {
                primary: rgb(0xE7, 0xEE, 0xF7),
                secondary: rgb(0xA3, 0xB1, 0xC2),
                tertiary: rgb(0x72, 0x83, 0x99),
                accent: rgb(0x6E, 0xA8, 0xFE),
                inverse: rgb(0x07, 0x10, 0x1A),
            },
            status: StatusColors {
                success: rgb(0x5A, 0xC0, 0x8E),
                warning: rgb(0xD6, 0xA5, 0x5A),
                error: rgb(0xE3, 0x6D, 0x6D),
                info: rgb(0x7C, 0xB7, 0xFF),
                disabled: rgb(0x5F, 0x70, 0x85),
            },
            live_shell: Self::OPENCODE_SHELL,
        }
    }

    pub fn default_theme() -> Self {
        Self::harness_app_dark()
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
        assert_eq!(theme.surface.canvas, Color::Black);
        assert_eq!(theme.text.primary, Color::Rgb(0xFF, 0xB0, 0x00));
    }

    #[test]
    fn harness_app_dark_theme_has_exact_palette() {
        let theme = Theme::harness_app_dark();
        assert_eq!(theme.surface.canvas, rgb(0x07, 0x0B, 0x12));
        assert_eq!(theme.surface.shell, rgb(0x0D, 0x15, 0x22));
        assert_eq!(theme.surface.panel, rgb(0x10, 0x1A, 0x29));
        assert_eq!(theme.surface.panel_elevated, rgb(0x15, 0x22, 0x35));
        assert_eq!(theme.surface.overlay, rgb(0x18, 0x27, 0x3B));
        assert_eq!(theme.border.subtle, rgb(0x24, 0x35, 0x4B));
        assert_eq!(theme.border.strong, rgb(0x31, 0x47, 0x60));
        assert_eq!(theme.border.focus, rgb(0x6E, 0xA8, 0xFE));
        assert_eq!(theme.text.primary, rgb(0xE7, 0xEE, 0xF7));
        assert_eq!(theme.text.secondary, rgb(0xA3, 0xB1, 0xC2));
        assert_eq!(theme.text.tertiary, rgb(0x72, 0x83, 0x99));
        assert_eq!(theme.text.accent, rgb(0x6E, 0xA8, 0xFE));
        assert_eq!(theme.text.inverse, rgb(0x07, 0x10, 0x1A));
        assert_eq!(theme.status.success, rgb(0x5A, 0xC0, 0x8E));
        assert_eq!(theme.status.warning, rgb(0xD6, 0xA5, 0x5A));
        assert_eq!(theme.status.error, rgb(0xE3, 0x6D, 0x6D));
        assert_eq!(theme.status.info, rgb(0x7C, 0xB7, 0xFF));
        assert_eq!(theme.status.disabled, rgb(0x5F, 0x70, 0x85));
    }

    #[test]
    fn default_theme_is_harness_app_dark() {
        let default = Theme::default();
        let harness = Theme::harness_app_dark();
        assert_eq!(default.surface.canvas, harness.surface.canvas);
        assert_eq!(default.text.primary, harness.text.primary);
        assert_eq!(default.status.info, harness.status.info);
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
        assert_eq!(minimum.details_sidebar_width, 34);
        assert_eq!(primary.centered_content_width, 92);
        assert_eq!(primary.content_margin_x, 2);
        assert_eq!(primary.details_sidebar_width, 40);
        assert_eq!(theme.live_shell.rhythm.status_separator, 2);
        assert_eq!(theme.live_shell.heights.status, 1);
        assert_eq!(theme.live_shell.transcript_glyphs.user_marker, "›");
        assert_eq!(theme.live_shell.transcript_glyphs.card_top, "╭─");

        assert!(minimum.centered_content_width + minimum.content_margin_x.saturating_mul(2) <= 80);
        assert!(primary.centered_content_width + primary.content_margin_x.saturating_mul(2) <= 100);
    }

    #[test]
    fn layout_plan_shell_width_tracks_theme_contracts() {
        let mut app = crate::app::AppState::new_live(None, false, None);
        app.active_tab = crate::app::Tab::Run;

        let minimum = crate::layout::FrameLayoutPlan::for_app(
            &app,
            ratatui::layout::Rect::new(
                0,
                0,
                ShellGeometry::MINIMUM.width,
                ShellGeometry::MINIMUM.height,
            ),
        );
        assert_eq!(minimum.shell.width, 78);

        let primary = crate::layout::FrameLayoutPlan::for_app(
            &app,
            ratatui::layout::Rect::new(
                0,
                0,
                ShellGeometry::PRIMARY.width,
                ShellGeometry::PRIMARY.height,
            ),
        );
        assert_eq!(primary.shell.width, 96);
    }
}
