use ratatui::style::Color;

const fn rgb(red: u8, green: u8, blue: u8) -> Color {
    Color::Rgb(red, green, blue)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellGeometryTarget {
    Minimum,
    Split,
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

    pub const SPLIT: Self = Self {
        width: 90,
        height: 36,
    };

    pub const PRIMARY: Self = Self {
        width: 100,
        height: 30,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellBreakpoints {
    pub minimum: ShellGeometry,
    pub split: ShellGeometry,
    pub primary: ShellGeometry,
}

impl ShellBreakpoints {
    pub const DEFAULT: Self = Self {
        minimum: ShellGeometry::MINIMUM,
        split: ShellGeometry::SPLIT,
        primary: ShellGeometry::PRIMARY,
    };

    pub const fn target(self, width: u16, height: u16) -> ShellGeometryTarget {
        if width >= self.primary.width && height >= self.primary.height {
            ShellGeometryTarget::Primary
        } else if width >= self.split.width && height >= self.split.height {
            ShellGeometryTarget::Split
        } else {
            ShellGeometryTarget::Minimum
        }
    }
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
    pub composer_padding_x: u16,
    pub footer_prefix_gap: u16,
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
pub struct LifecycleCardTokens {
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleOverlayTokens {
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleSurfaceLayout {
    pub target: ShellGeometryTarget,
    pub startup_card: LifecycleCardTokens,
    pub post_run_card: LifecycleCardTokens,
    pub overlay: LifecycleOverlayTokens,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleSurfaceTokens {
    pub minimum: LifecycleSurfaceLayout,
    pub split: LifecycleSurfaceLayout,
    pub primary: LifecycleSurfaceLayout,
}

impl LifecycleSurfaceTokens {
    pub const fn select(
        self,
        breakpoints: ShellBreakpoints,
        width: u16,
        height: u16,
    ) -> LifecycleSurfaceLayout {
        match breakpoints.target(width, height) {
            ShellGeometryTarget::Minimum => self.minimum,
            ShellGeometryTarget::Split => self.split,
            ShellGeometryTarget::Primary => self.primary,
        }
    }
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
pub struct StartupLifecycleTokens {
    pub title: &'static str,
    pub subtitle: &'static str,
    pub new_session_purpose: &'static str,
    pub continue_session_purpose: &'static str,
    pub replay_session_purpose: &'static str,
    pub secondary_hint: &'static str,
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
pub struct LiveShellGeometryTokens {
    pub breakpoints: ShellBreakpoints,
    pub minimum: LiveShellLayout,
    pub split: LiveShellLayout,
    pub primary: LiveShellLayout,
    pub lifecycle: LifecycleSurfaceTokens,
}

impl LiveShellGeometryTokens {
    pub const fn select(self, width: u16, height: u16) -> LiveShellLayout {
        match self.breakpoints.target(width, height) {
            ShellGeometryTarget::Minimum => self.minimum,
            ShellGeometryTarget::Split => self.split,
            ShellGeometryTarget::Primary => self.primary,
        }
    }

    pub const fn target(self, width: u16, height: u16) -> ShellGeometryTarget {
        self.breakpoints.target(width, height)
    }

    pub const fn lifecycle_layout(self, width: u16, height: u16) -> LifecycleSurfaceLayout {
        self.lifecycle.select(self.breakpoints, width, height)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveShellSpacingTokens {
    pub heights: ShellHeights,
    pub rhythm: ShellRhythm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveShellGlyphCatalog {
    pub status: StatusGlyphs,
    pub transcript: TranscriptGlyphs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveShellGlyphTokens {
    pub preferred: LiveShellGlyphCatalog,
    pub ascii: LiveShellGlyphCatalog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveShellCopyTokens {
    pub startup: StartupLifecycleTokens,
    pub empty_state: EmptyStateTokens,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveShellTokenFamilies {
    pub geometry: LiveShellGeometryTokens,
    pub spacing: LiveShellSpacingTokens,
    pub glyphs: LiveShellGlyphTokens,
    pub copy: LiveShellCopyTokens,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveShellTokens {
    pub breakpoints: ShellBreakpoints,
    pub minimum: LiveShellLayout,
    pub split: LiveShellLayout,
    pub primary: LiveShellLayout,
    pub lifecycle: LifecycleSurfaceTokens,
    pub heights: ShellHeights,
    pub rhythm: ShellRhythm,
    pub glyphs: StatusGlyphs,
    pub transcript_glyphs: TranscriptGlyphs,
    pub ascii_glyphs: LiveShellGlyphCatalog,
    pub startup: StartupLifecycleTokens,
    pub empty_state: EmptyStateTokens,
}

impl LiveShellTokens {
    pub const fn families(self) -> LiveShellTokenFamilies {
        LiveShellTokenFamilies {
            geometry: LiveShellGeometryTokens {
                breakpoints: self.breakpoints,
                minimum: self.minimum,
                split: self.split,
                primary: self.primary,
                lifecycle: self.lifecycle,
            },
            spacing: LiveShellSpacingTokens {
                heights: self.heights,
                rhythm: self.rhythm,
            },
            glyphs: LiveShellGlyphTokens {
                preferred: LiveShellGlyphCatalog {
                    status: self.glyphs,
                    transcript: self.transcript_glyphs,
                },
                ascii: self.ascii_glyphs,
            },
            copy: LiveShellCopyTokens {
                startup: self.startup,
                empty_state: self.empty_state,
            },
        }
    }

    pub const fn select(self, width: u16, height: u16) -> LiveShellLayout {
        self.families().geometry.select(width, height)
    }

    pub const fn target(self, width: u16, height: u16) -> ShellGeometryTarget {
        self.families().geometry.target(width, height)
    }

    pub const fn lifecycle_layout(self, width: u16, height: u16) -> LifecycleSurfaceLayout {
        self.families().geometry.lifecycle_layout(width, height)
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
pub struct ThemePalette {
    pub surfaces: SurfaceColors,
    pub borders: BorderColors,
    pub text: TextColors,
    pub status: StatusColors,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeTokenFamilies {
    pub palette: ThemePalette,
    pub live_shell: LiveShellTokenFamilies,
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
        breakpoints: ShellBreakpoints::DEFAULT,
        minimum: LiveShellLayout {
            target: ShellGeometryTarget::Minimum,
            activity_drawer_width: 20,
            inspector_drawer_width: 20,
            details_sidebar_width: 34,
            transcript_min_width: 28,
            permission_modal_width: 58,
            centered_content_width: 78,
            content_margin_x: 1,
        },
        split: LiveShellLayout {
            target: ShellGeometryTarget::Split,
            activity_drawer_width: 18,
            inspector_drawer_width: 24,
            details_sidebar_width: 32,
            transcript_min_width: 32,
            permission_modal_width: 62,
            centered_content_width: 88,
            content_margin_x: 1,
        },
        primary: LiveShellLayout {
            target: ShellGeometryTarget::Primary,
            activity_drawer_width: 24,
            inspector_drawer_width: 28,
            details_sidebar_width: 40,
            transcript_min_width: 40,
            permission_modal_width: 66,
            centered_content_width: 92,
            content_margin_x: 2,
        },
        lifecycle: LifecycleSurfaceTokens {
            minimum: LifecycleSurfaceLayout {
                target: ShellGeometryTarget::Minimum,
                startup_card: LifecycleCardTokens {
                    width: 70,
                    height: 9,
                },
                post_run_card: LifecycleCardTokens {
                    width: 72,
                    height: 10,
                },
                overlay: LifecycleOverlayTokens {
                    width: 78,
                    height: 11,
                },
            },
            split: LifecycleSurfaceLayout {
                target: ShellGeometryTarget::Split,
                startup_card: LifecycleCardTokens {
                    width: 72,
                    height: 9,
                },
                post_run_card: LifecycleCardTokens {
                    width: 74,
                    height: 10,
                },
                overlay: LifecycleOverlayTokens {
                    width: 88,
                    height: 11,
                },
            },
            primary: LifecycleSurfaceLayout {
                target: ShellGeometryTarget::Primary,
                startup_card: LifecycleCardTokens {
                    width: 74,
                    height: 9,
                },
                post_run_card: LifecycleCardTokens {
                    width: 76,
                    height: 10,
                },
                overlay: LifecycleOverlayTokens {
                    width: 92,
                    height: 11,
                },
            },
        },
        heights: ShellHeights {
            header: 1,
            tabs: 3,
            status: 1,
            footer: 1,
            prompt_input: 3,
            permission_modal: 9,
        },
        rhythm: ShellRhythm {
            composer_padding_x: 1,
            footer_prefix_gap: 2,
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
        ascii_glyphs: LiveShellGlyphCatalog {
            status: StatusGlyphs {
                streaming: "o",
                done: "*",
                error: "x",
                pending_permission: "?",
                queued: ".",
                running: "o",
                succeeded: "*",
                failed: "x",
            },
            transcript: TranscriptGlyphs {
                user_marker: ">",
                card_top: "+-",
                card_mid: "|",
                card_bottom: "+-",
            },
        },
        startup: StartupLifecycleTokens {
            title: "Harness",
            subtitle: "Dispatch a new run, reopen live work, or inspect saved history.",
            new_session_purpose: "dispatch a fresh run from the draft below",
            continue_session_purpose: "reopen interactive work",
            replay_session_purpose: "inspect saved runs read-only",
            secondary_hint: "Type to quick-start a fresh run · Ctrl+P opens session tools",
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

    pub const fn token_families(self) -> ThemeTokenFamilies {
        ThemeTokenFamilies {
            palette: ThemePalette {
                surfaces: self.surface,
                borders: self.border,
                text: self.text,
                status: self.status,
            },
            live_shell: self.live_shell.families(),
        }
    }

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

    pub const fn lifecycle_surface_layout(self, width: u16, height: u16) -> LifecycleSurfaceLayout {
        self.live_shell.lifecycle_layout(width, height)
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
    fn semantic_theme_families_preserve_default_contracts() {
        let theme = Theme::default();
        let tokens = theme.token_families();

        assert_eq!(tokens.palette.surfaces, theme.surface);
        assert_eq!(tokens.palette.borders, theme.border);
        assert_eq!(tokens.palette.text, theme.text);
        assert_eq!(tokens.palette.status, theme.status);
        assert_eq!(
            tokens.live_shell.geometry.breakpoints,
            ShellBreakpoints::DEFAULT
        );
        assert_eq!(tokens.live_shell.geometry.minimum, theme.live_shell.minimum);
        assert_eq!(tokens.live_shell.geometry.primary, theme.live_shell.primary);
        assert_eq!(tokens.live_shell.spacing.heights, theme.live_shell.heights);
        assert_eq!(tokens.live_shell.spacing.rhythm, theme.live_shell.rhythm);
        assert_eq!(
            tokens.live_shell.glyphs.preferred.status,
            theme.live_shell.glyphs
        );
        assert_eq!(
            tokens.live_shell.glyphs.preferred.transcript,
            theme.live_shell.transcript_glyphs
        );
        assert_eq!(tokens.live_shell.copy.startup, theme.live_shell.startup);
        assert_eq!(
            tokens.live_shell.copy.empty_state,
            theme.live_shell.empty_state
        );
        assert_eq!(
            tokens.live_shell.glyphs.ascii.status.pending_permission,
            "?"
        );
        assert_eq!(tokens.live_shell.glyphs.ascii.transcript.user_marker, ">");
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
        let tokens = theme.token_families();
        let minimum = theme.live_shell_layout(80, 24);
        let split = theme.live_shell_layout(96, 40);
        let primary = theme.live_shell_layout(100, 30);
        let minimum_lifecycle = theme.lifecycle_surface_layout(80, 24);
        let split_lifecycle = theme.lifecycle_surface_layout(96, 40);
        let primary_lifecycle = theme.lifecycle_surface_layout(100, 30);

        assert_eq!(primary.target, ShellGeometryTarget::Primary);
        assert_eq!(split.target, ShellGeometryTarget::Split);
        assert_eq!(minimum.target, ShellGeometryTarget::Minimum);
        assert_eq!(primary_lifecycle.target, ShellGeometryTarget::Primary);
        assert_eq!(split_lifecycle.target, ShellGeometryTarget::Split);
        assert_eq!(minimum_lifecycle.target, ShellGeometryTarget::Minimum);
        assert_eq!(minimum.centered_content_width, 78);
        assert_eq!(minimum.content_margin_x, 1);
        assert_eq!(minimum.details_sidebar_width, 34);
        assert_eq!(minimum_lifecycle.startup_card.width, 70);
        assert_eq!(minimum_lifecycle.post_run_card.width, 72);
        assert_eq!(minimum_lifecycle.overlay.width, 78);
        assert_eq!(theme.live_shell.rhythm.composer_padding_x, 1);
        assert_eq!(theme.live_shell.rhythm.footer_prefix_gap, 2);
        assert_eq!(minimum.permission_modal_width, 58);
        assert_eq!(split.centered_content_width, 88);
        assert_eq!(split.content_margin_x, 1);
        assert_eq!(split.details_sidebar_width, 32);
        assert_eq!(split_lifecycle.startup_card.width, 72);
        assert_eq!(split_lifecycle.post_run_card.width, 74);
        assert_eq!(split_lifecycle.overlay.width, 88);
        assert_eq!(split.permission_modal_width, 62);
        assert_eq!(primary.centered_content_width, 92);
        assert_eq!(primary.content_margin_x, 2);
        assert_eq!(primary.details_sidebar_width, 40);
        assert_eq!(primary_lifecycle.startup_card.width, 74);
        assert_eq!(primary_lifecycle.post_run_card.width, 76);
        assert_eq!(primary_lifecycle.overlay.width, 92);
        assert_eq!(primary.permission_modal_width, 66);
        assert_eq!(theme.live_shell.rhythm.status_separator, 2);
        assert_eq!(theme.live_shell.heights.status, 1);
        assert_eq!(theme.live_shell.heights.permission_modal, 9);
        assert_eq!(theme.live_shell.transcript_glyphs.user_marker, "›");
        assert_eq!(theme.live_shell.transcript_glyphs.card_top, "╭─");
        assert_eq!(
            tokens.live_shell.geometry.target(100, 30),
            ShellGeometryTarget::Primary
        );
        assert_eq!(tokens.live_shell.glyphs.ascii.status.failed, "x");
        assert_eq!(tokens.live_shell.glyphs.ascii.transcript.card_top, "+-");

        assert!(minimum.centered_content_width + minimum.content_margin_x.saturating_mul(2) <= 80);
        assert!(split.centered_content_width + split.content_margin_x.saturating_mul(2) <= 96);
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

        let split =
            crate::layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 96, 40));
        assert_eq!(split.shell.width, 94);
    }
}
