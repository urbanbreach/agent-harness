use ratatui::style::Color;

pub const DIFF_SIDE_BY_SIDE_MIN_WIDTH: u16 = 96;

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
}

impl ShellHeights {
    pub const fn prompt_block(self) -> u16 {
        self.prompt_input + 2
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellRhythm {
    pub composer_padding_x: u16,
    pub sidebar_padding_x: u16,
    pub sidebar_padding_y: u16,
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
pub enum ChromeMode {
    Chromeless,
    Divided,
    Card,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DividerIntensity {
    None,
    Subtle,
    Strong,
    Focus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpacingDensity {
    Compact,
    Standard,
    Roomy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DividerTokens {
    pub intensity: DividerIntensity,
    pub color: Option<Color>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeTokens {
    pub mode: ChromeMode,
    pub surface: Color,
    pub border: DividerTokens,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChromeTokenFamilies {
    pub chromeless: ChromeTokens,
    pub divided: ChromeTokens,
    pub card: ChromeTokens,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DividerTokenFamilies {
    pub none: DividerTokens,
    pub subtle: DividerTokens,
    pub strong: DividerTokens,
    pub focus: DividerTokens,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DensitySpacingTokens {
    pub target: ShellGeometryTarget,
    pub density: SpacingDensity,
    pub content_margin_x: u16,
    pub heights: ShellHeights,
    pub rhythm: ShellRhythm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DensitySpacingTokenFamilies {
    pub minimum: DensitySpacingTokens,
    pub split: DensitySpacingTokens,
    pub primary: DensitySpacingTokens,
}

impl DensitySpacingTokenFamilies {
    pub const fn select(self, target: ShellGeometryTarget) -> DensitySpacingTokens {
        match target {
            ShellGeometryTarget::Minimum => self.minimum,
            ShellGeometryTarget::Split => self.split,
            ShellGeometryTarget::Primary => self.primary,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposerPresentationTokens {
    pub target: ShellGeometryTarget,
    pub chrome: ChromeMode,
    pub divider: DividerIntensity,
    pub density: SpacingDensity,
    pub surface: Color,
    pub border: Option<Color>,
    pub padding_x: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComposerTokenFamilies {
    pub minimum: ComposerPresentationTokens,
    pub split: ComposerPresentationTokens,
    pub primary: ComposerPresentationTokens,
}

impl ComposerTokenFamilies {
    pub const fn select(self, target: ShellGeometryTarget) -> ComposerPresentationTokens {
        match target {
            ShellGeometryTarget::Minimum => self.minimum,
            ShellGeometryTarget::Split => self.split,
            ShellGeometryTarget::Primary => self.primary,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticThemeTokenFamilies {
    pub chrome: ChromeTokenFamilies,
    pub dividers: DividerTokenFamilies,
    pub density: DensitySpacingTokenFamilies,
    pub composer: ComposerTokenFamilies,
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
pub struct QuestionPromptColors {
    pub accent: Color,
    pub secondary: Color,
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
pub struct ScrollbarColors {
    pub track: Color,
    pub thumb: Color,
    pub thumb_active: Color,
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
    pub semantic: SemanticThemeTokenFamilies,
    pub palette: ThemePalette,
    pub live_shell: LiveShellTokenFamilies,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub surface: SurfaceColors,
    pub border: BorderColors,
    pub text: TextColors,
    pub question_prompt: QuestionPromptColors,
    pub status: StatusColors,
    pub scrollbar: ScrollbarColors,
    pub live_shell: LiveShellTokens,
}

impl Theme {
    const HARNESS_DARK_SHELL: LiveShellTokens = LiveShellTokens {
        breakpoints: ShellBreakpoints::DEFAULT,
        minimum: LiveShellLayout {
            target: ShellGeometryTarget::Minimum,
            activity_drawer_width: 20,
            inspector_drawer_width: 20,
            details_sidebar_width: 42,
            transcript_min_width: 28,
            centered_content_width: 76,
            content_margin_x: 1,
        },
        split: LiveShellLayout {
            target: ShellGeometryTarget::Split,
            activity_drawer_width: 18,
            inspector_drawer_width: 24,
            details_sidebar_width: 42,
            transcript_min_width: 32,
            centered_content_width: 86,
            content_margin_x: 0,
        },
        primary: LiveShellLayout {
            target: ShellGeometryTarget::Primary,
            activity_drawer_width: 24,
            inspector_drawer_width: 28,
            details_sidebar_width: 42,
            transcript_min_width: 40,
            centered_content_width: 90,
            content_margin_x: 0,
        },
        lifecycle: LifecycleSurfaceTokens {
            minimum: LifecycleSurfaceLayout {
                target: ShellGeometryTarget::Minimum,
                startup_card: LifecycleCardTokens {
                    width: 70,
                    height: 12,
                },
                post_run_card: LifecycleCardTokens {
                    width: 72,
                    height: 12,
                },
                overlay: LifecycleOverlayTokens {
                    width: 76,
                    height: 11,
                },
            },
            split: LifecycleSurfaceLayout {
                target: ShellGeometryTarget::Split,
                startup_card: LifecycleCardTokens {
                    width: 92,
                    height: 13,
                },
                post_run_card: LifecycleCardTokens {
                    width: 76,
                    height: 12,
                },
                overlay: LifecycleOverlayTokens {
                    width: 86,
                    height: 11,
                },
            },
            primary: LifecycleSurfaceLayout {
                target: ShellGeometryTarget::Primary,
                startup_card: LifecycleCardTokens {
                    width: 82,
                    height: 12,
                },
                post_run_card: LifecycleCardTokens {
                    width: 78,
                    height: 12,
                },
                overlay: LifecycleOverlayTokens {
                    width: 90,
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
        },
        rhythm: ShellRhythm {
            composer_padding_x: 2,
            sidebar_padding_x: 2,
            sidebar_padding_y: 1,
            footer_prefix_gap: 2,
            transcript_gutter_x: 2,
            transcript_gutter_y: 1,
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
            new_session_purpose: "open a fresh session in this directory",
            continue_session_purpose: "reopen interactive work",
            replay_session_purpose: "inspect saved runs read-only",
            secondary_hint: "Type to start immediately · Ctrl+P opens saved sessions",
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
                    prompt: "review the latest edit",
                },
            ],
            demo_mode_label: "Demo mode · mock provider",
            mock_mode_label: "Mock mode · mock provider",
        },
    };

    pub const fn token_families(self) -> ThemeTokenFamilies {
        ThemeTokenFamilies {
            semantic: SemanticThemeTokenFamilies {
                chrome: ChromeTokenFamilies {
                    chromeless: ChromeTokens {
                        mode: ChromeMode::Chromeless,
                        surface: self.surface.shell,
                        border: DividerTokens {
                            intensity: DividerIntensity::None,
                            color: None,
                        },
                    },
                    divided: ChromeTokens {
                        mode: ChromeMode::Divided,
                        surface: self.surface.panel,
                        border: DividerTokens {
                            intensity: DividerIntensity::Subtle,
                            color: Some(self.border.subtle),
                        },
                    },
                    card: ChromeTokens {
                        mode: ChromeMode::Card,
                        surface: self.surface.overlay,
                        border: DividerTokens {
                            intensity: DividerIntensity::Subtle,
                            color: Some(self.border.subtle),
                        },
                    },
                },
                dividers: DividerTokenFamilies {
                    none: DividerTokens {
                        intensity: DividerIntensity::None,
                        color: None,
                    },
                    subtle: DividerTokens {
                        intensity: DividerIntensity::Subtle,
                        color: Some(self.border.subtle),
                    },
                    strong: DividerTokens {
                        intensity: DividerIntensity::Strong,
                        color: Some(self.border.strong),
                    },
                    focus: DividerTokens {
                        intensity: DividerIntensity::Focus,
                        color: Some(self.border.focus),
                    },
                },
                density: DensitySpacingTokenFamilies {
                    minimum: DensitySpacingTokens {
                        target: ShellGeometryTarget::Minimum,
                        density: SpacingDensity::Compact,
                        content_margin_x: self.live_shell.minimum.content_margin_x,
                        heights: self.live_shell.heights,
                        rhythm: self.live_shell.rhythm,
                    },
                    split: DensitySpacingTokens {
                        target: ShellGeometryTarget::Split,
                        density: SpacingDensity::Standard,
                        content_margin_x: self.live_shell.split.content_margin_x,
                        heights: self.live_shell.heights,
                        rhythm: self.live_shell.rhythm,
                    },
                    primary: DensitySpacingTokens {
                        target: ShellGeometryTarget::Primary,
                        density: SpacingDensity::Roomy,
                        content_margin_x: self.live_shell.primary.content_margin_x,
                        heights: self.live_shell.heights,
                        rhythm: self.live_shell.rhythm,
                    },
                },
                composer: ComposerTokenFamilies {
                    minimum: ComposerPresentationTokens {
                        target: ShellGeometryTarget::Minimum,
                        chrome: ChromeMode::Card,
                        divider: DividerIntensity::Subtle,
                        density: SpacingDensity::Compact,
                        surface: self.surface.panel_elevated,
                        border: Some(self.border.subtle),
                        padding_x: self.live_shell.rhythm.composer_padding_x,
                    },
                    split: ComposerPresentationTokens {
                        target: ShellGeometryTarget::Split,
                        chrome: ChromeMode::Divided,
                        divider: DividerIntensity::Subtle,
                        density: SpacingDensity::Standard,
                        surface: self.surface.panel_elevated,
                        border: Some(self.border.subtle),
                        padding_x: self.live_shell.rhythm.composer_padding_x,
                    },
                    primary: ComposerPresentationTokens {
                        target: ShellGeometryTarget::Primary,
                        chrome: ChromeMode::Divided,
                        divider: DividerIntensity::Subtle,
                        density: SpacingDensity::Roomy,
                        surface: self.surface.panel_elevated,
                        border: Some(self.border.subtle),
                        padding_x: self.live_shell.rhythm.composer_padding_x,
                    },
                },
            },
            palette: ThemePalette {
                surfaces: self.surface,
                borders: self.border,
                text: self.text,
                status: self.status,
            },
            live_shell: self.live_shell.families(),
        }
    }

    pub fn harness_dark() -> Self {
        Self {
            surface: SurfaceColors {
                canvas: rgb(0x0A, 0x0A, 0x0A),
                shell: rgb(0x0A, 0x0A, 0x0A),
                panel: rgb(0x14, 0x14, 0x14),
                panel_elevated: rgb(0x1E, 0x1E, 0x1E),
                overlay: rgb(0x14, 0x14, 0x14),
            },
            border: BorderColors {
                subtle: rgb(0x3C, 0x3C, 0x3C),
                strong: rgb(0x48, 0x48, 0x48),
                focus: rgb(0x60, 0x60, 0x60),
            },
            text: TextColors {
                primary: rgb(0xEE, 0xEE, 0xEE),
                secondary: rgb(0x80, 0x80, 0x80),
                tertiary: rgb(0x80, 0x80, 0x80),
                accent: rgb(0xFA, 0xB2, 0x83),
                inverse: rgb(0x0A, 0x0A, 0x0A),
            },
            question_prompt: QuestionPromptColors {
                accent: rgb(0x9D, 0x7C, 0xD8),
                secondary: rgb(0x5C, 0x9C, 0xF5),
            },
            status: StatusColors {
                success: rgb(0x7F, 0xD8, 0x8F),
                warning: rgb(0xF5, 0xA7, 0x42),
                error: rgb(0xE0, 0x6C, 0x75),
                info: rgb(0x56, 0xB6, 0xC2),
                disabled: rgb(0x80, 0x80, 0x80),
            },
            scrollbar: ScrollbarColors {
                track: rgb(0x14, 0x14, 0x14),
                thumb: rgb(0x32, 0x32, 0x32),
                thumb_active: rgb(0x60, 0x60, 0x60),
            },
            live_shell: Self::HARNESS_DARK_SHELL,
        }
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
        Self::harness_dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_dark_theme_matches_palette_contract() {
        let theme = Theme::harness_dark();
        assert_eq!(theme.surface.canvas, rgb(0x0A, 0x0A, 0x0A));
        assert_eq!(theme.surface.shell, rgb(0x0A, 0x0A, 0x0A));
        assert_eq!(theme.surface.panel, rgb(0x14, 0x14, 0x14));
        assert_eq!(theme.surface.panel_elevated, rgb(0x1E, 0x1E, 0x1E));
        assert_eq!(theme.surface.overlay, rgb(0x14, 0x14, 0x14));
        assert_eq!(theme.border.subtle, rgb(0x3C, 0x3C, 0x3C));
        assert_eq!(theme.border.strong, rgb(0x48, 0x48, 0x48));
        assert_eq!(theme.border.focus, rgb(0x60, 0x60, 0x60));
        assert_eq!(theme.text.primary, rgb(0xEE, 0xEE, 0xEE));
        assert_eq!(theme.text.secondary, rgb(0x80, 0x80, 0x80));
        assert_eq!(theme.text.tertiary, rgb(0x80, 0x80, 0x80));
        assert_eq!(theme.text.accent, rgb(0xFA, 0xB2, 0x83));
        assert_eq!(theme.text.inverse, rgb(0x0A, 0x0A, 0x0A));
        assert_eq!(theme.question_prompt.accent, rgb(0x9D, 0x7C, 0xD8));
        assert_eq!(theme.question_prompt.secondary, rgb(0x5C, 0x9C, 0xF5));
        assert_eq!(theme.status.success, rgb(0x7F, 0xD8, 0x8F));
        assert_eq!(theme.status.warning, rgb(0xF5, 0xA7, 0x42));
        assert_eq!(theme.status.error, rgb(0xE0, 0x6C, 0x75));
        assert_eq!(theme.status.info, rgb(0x56, 0xB6, 0xC2));
        assert_eq!(theme.status.disabled, rgb(0x80, 0x80, 0x80));
        assert_eq!(theme.scrollbar.track, rgb(0x14, 0x14, 0x14));
        assert_eq!(theme.scrollbar.thumb, rgb(0x32, 0x32, 0x32));
        assert_eq!(theme.scrollbar.thumb_active, rgb(0x60, 0x60, 0x60));
    }

    #[test]
    fn semantic_theme_families_preserve_default_contracts() {
        let theme = Theme::default();
        let tokens = theme.token_families();

        assert_eq!(
            tokens.semantic.chrome.chromeless.surface,
            theme.surface.shell
        );
        assert_eq!(tokens.semantic.chrome.divided.surface, theme.surface.panel);
        assert_eq!(tokens.semantic.chrome.card.surface, theme.surface.overlay);
        assert_ne!(theme.surface.shell, theme.surface.panel);
        assert_ne!(theme.surface.panel, theme.surface.panel_elevated);
        assert_ne!(theme.surface.shell, theme.surface.panel_elevated);
        assert_eq!(
            tokens.semantic.dividers.subtle.color,
            Some(theme.border.subtle)
        );
        assert_eq!(
            tokens.semantic.dividers.strong.color,
            Some(theme.border.strong)
        );
        assert_eq!(
            tokens.semantic.dividers.focus.color,
            Some(theme.border.focus)
        );
        assert_eq!(
            tokens.semantic.density.minimum.heights,
            theme.live_shell.heights
        );
        assert_eq!(
            tokens.semantic.density.split.rhythm,
            theme.live_shell.rhythm
        );
        assert_eq!(
            tokens.semantic.density.primary.content_margin_x,
            theme.live_shell.primary.content_margin_x
        );
        assert_eq!(
            tokens.semantic.composer.primary.padding_x,
            theme.live_shell.rhythm.composer_padding_x
        );
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
    fn default_theme_matches_harness_dark_contract() {
        let default = Theme::default();
        let harness_dark = Theme::harness_dark();

        assert_eq!(default, harness_dark);
        assert_eq!(default.token_families(), harness_dark.token_families());
    }

    #[test]
    fn semantic_chrome_tokens_map_to_harness_dark_defaults() {
        let theme = Theme::default();
        let tokens = theme.token_families();

        assert_eq!(
            tokens.semantic.chrome.chromeless.mode,
            ChromeMode::Chromeless
        );
        assert_eq!(
            tokens.semantic.chrome.chromeless.surface,
            theme.surface.shell
        );
        assert_eq!(
            tokens.semantic.chrome.chromeless.border,
            tokens.semantic.dividers.none
        );
        assert_eq!(tokens.semantic.chrome.divided.mode, ChromeMode::Divided);
        assert_eq!(tokens.semantic.chrome.divided.surface, theme.surface.panel);
        assert_eq!(
            tokens.semantic.chrome.divided.border,
            tokens.semantic.dividers.subtle
        );
        assert_eq!(tokens.semantic.chrome.card.mode, ChromeMode::Card);
        assert_eq!(tokens.semantic.chrome.card.surface, theme.surface.overlay);
        assert_eq!(
            tokens.semantic.chrome.card.border,
            tokens.semantic.dividers.subtle
        );
        assert_eq!(
            tokens.semantic.dividers.none.intensity,
            DividerIntensity::None
        );
        assert_eq!(tokens.semantic.dividers.none.color, None);
        assert_eq!(
            tokens.semantic.dividers.subtle.color,
            Some(theme.border.subtle)
        );
        assert_eq!(
            tokens.semantic.dividers.strong.color,
            Some(theme.border.strong)
        );
        assert_eq!(
            tokens.semantic.dividers.focus.color,
            Some(theme.border.focus)
        );
    }

    #[test]
    fn semantic_composer_tokens_have_primary_split_minimum_variants() {
        let theme = Theme::default();
        let tokens = theme.token_families();

        assert_eq!(
            tokens.semantic.composer.minimum.target,
            ShellGeometryTarget::Minimum
        );
        assert_eq!(tokens.semantic.composer.minimum.chrome, ChromeMode::Card);
        assert_eq!(
            tokens.semantic.composer.minimum.divider,
            DividerIntensity::Subtle
        );
        assert_eq!(
            tokens.semantic.composer.minimum.density,
            SpacingDensity::Compact
        );
        assert_eq!(
            tokens.semantic.composer.minimum.surface,
            theme.surface.panel_elevated
        );
        assert_eq!(
            tokens.semantic.composer.minimum.border,
            Some(theme.border.subtle)
        );
        assert_eq!(
            tokens.semantic.composer.split.target,
            ShellGeometryTarget::Split
        );
        assert_eq!(tokens.semantic.composer.split.chrome, ChromeMode::Divided);
        assert_eq!(
            tokens.semantic.composer.split.divider,
            DividerIntensity::Subtle
        );
        assert_eq!(
            tokens.semantic.composer.split.density,
            SpacingDensity::Standard
        );
        assert_eq!(
            tokens.semantic.composer.split.surface,
            theme.surface.panel_elevated
        );
        assert_eq!(
            tokens.semantic.composer.split.border,
            Some(theme.border.subtle)
        );
        assert_eq!(
            tokens.semantic.composer.primary.target,
            ShellGeometryTarget::Primary
        );
        assert_eq!(tokens.semantic.composer.primary.chrome, ChromeMode::Divided);
        assert_eq!(
            tokens.semantic.composer.primary.divider,
            DividerIntensity::Subtle
        );
        assert_eq!(
            tokens.semantic.composer.primary.density,
            SpacingDensity::Roomy
        );
        assert_eq!(
            tokens.semantic.composer.primary.surface,
            theme.surface.panel_elevated
        );
        assert_eq!(
            tokens.semantic.composer.primary.border,
            Some(theme.border.subtle)
        );
        assert_eq!(
            tokens.semantic.composer.minimum.padding_x,
            theme.live_shell.rhythm.composer_padding_x
        );
        assert_eq!(
            tokens.semantic.composer.split.padding_x,
            theme.live_shell.rhythm.composer_padding_x
        );
        assert_eq!(
            tokens.semantic.composer.primary.padding_x,
            theme.live_shell.rhythm.composer_padding_x
        );
        assert_eq!(
            tokens
                .semantic
                .composer
                .select(ShellGeometryTarget::Minimum),
            tokens.semantic.composer.minimum
        );
        assert_eq!(
            tokens.semantic.composer.select(ShellGeometryTarget::Split),
            tokens.semantic.composer.split
        );
        assert_eq!(
            tokens
                .semantic
                .composer
                .select(ShellGeometryTarget::Primary),
            tokens.semantic.composer.primary
        );
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
        assert_eq!(minimum.centered_content_width, 76);
        assert_eq!(minimum.content_margin_x, 1);
        assert_eq!(minimum.details_sidebar_width, 42);
        assert_eq!(minimum_lifecycle.startup_card.width, 70);
        assert_eq!(minimum_lifecycle.startup_card.height, 12);
        assert_eq!(minimum_lifecycle.post_run_card.width, 72);
        assert_eq!(minimum_lifecycle.post_run_card.height, 12);
        assert_eq!(minimum_lifecycle.overlay.width, 76);
        assert_eq!(theme.live_shell.rhythm.composer_padding_x, 2);
        assert_eq!(theme.live_shell.rhythm.sidebar_padding_x, 2);
        assert_eq!(theme.live_shell.rhythm.sidebar_padding_y, 1);
        assert_eq!(theme.live_shell.rhythm.footer_prefix_gap, 2);
        assert_eq!(split.centered_content_width, 86);
        assert_eq!(split.content_margin_x, 0);
        assert_eq!(split.details_sidebar_width, 42);
        assert_eq!(split_lifecycle.startup_card.width, 92);
        assert_eq!(split_lifecycle.startup_card.height, 13);
        assert_eq!(split_lifecycle.post_run_card.width, 76);
        assert_eq!(split_lifecycle.post_run_card.height, 12);
        assert_eq!(split_lifecycle.overlay.width, 86);
        assert_eq!(primary.centered_content_width, 90);
        assert_eq!(primary.content_margin_x, 0);
        assert_eq!(primary.details_sidebar_width, 42);
        assert_eq!(primary_lifecycle.startup_card.width, 82);
        assert_eq!(primary_lifecycle.startup_card.height, 12);
        assert_eq!(primary_lifecycle.post_run_card.width, 78);
        assert_eq!(primary_lifecycle.post_run_card.height, 12);
        assert_eq!(primary_lifecycle.overlay.width, 90);
        assert_eq!(theme.live_shell.rhythm.status_separator, 2);
        assert_eq!(theme.live_shell.heights.status, 1);
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
        assert_eq!(minimum.shell.width, 76);

        let primary = crate::layout::FrameLayoutPlan::for_app(
            &app,
            ratatui::layout::Rect::new(
                0,
                0,
                ShellGeometry::PRIMARY.width,
                ShellGeometry::PRIMARY.height,
            ),
        );
        assert_eq!(primary.shell.width, 100);

        let split =
            crate::layout::FrameLayoutPlan::for_app(&app, ratatui::layout::Rect::new(0, 0, 96, 40));
        assert_eq!(split.shell.width, 96);
    }

    #[test]
    fn diff_side_by_side_threshold_matches_geometry_contract() {
        assert_eq!(DIFF_SIDE_BY_SIDE_MIN_WIDTH, 96);
    }
}
