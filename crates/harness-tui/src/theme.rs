// allow: SIZE_OK — TUI theme tokens (color system + shell geometry)
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
    pub tool_marker: &'static str,
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
    pub onboarding_hint: &'static str,
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
    pub card: Color,
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

/// Markdown token palette mirroring Harness's `harness.json` markdown rules.
///
/// Each field corresponds to an Harness markdown syntax token (e.g.
/// `markdownHeading`, `markdownCode`). When rendering reasoning bodies these
/// colors are blended at `thinkingOpacity` (0.6) over the surface color to
/// produce the subtle syntax-highlighted look from `generateSubtleSyntax`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkdownColors {
    pub heading: Color,
    pub link: Color,
    pub link_text: Color,
    pub code: Color,
    pub emph: Color,
    pub strong: Color,
    pub block_quote: Color,
    pub list_item: Color,
    pub list_enum: Color,
    pub rule: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentColors {
    pub build: Color,
    pub plan: Color,
    pub docs: Color,
    pub ask: Color,
    pub palette: [Color; 7],
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
    pub agents: AgentColors,
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
    pub markdown: MarkdownColors,
    pub agents: AgentColors,
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
            centered_content_width: 80,
            content_margin_x: 0,
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
            user_marker: "❯",
            tool_marker: "◆",
            card_top: "  ",
            card_mid: " ",
            card_bottom: "  ",
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
                tool_marker: "*",
                card_top: "  ",
                card_mid: " ",
                card_bottom: "  ",
            },
        },
        startup: StartupLifecycleTokens {
            title: "Harness",
            subtitle: "Dispatch a new run, reopen live work, or inspect saved history.",
            new_session_purpose: "open a fresh session in this directory",
            continue_session_purpose: "reopen interactive work",
            replay_session_purpose: "inspect saved runs read-only",
            secondary_hint: "Type to start immediately · Ctrl+P opens saved sessions",
            onboarding_hint: "First run? `harness doctor` or `harness auth login`",
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
                agents: self.agents,
            },
            live_shell: self.live_shell.families(),
        }
    }

    pub fn agent_accent(self, profile: &str) -> Color {
        let profile = profile.trim();
        if profile.is_empty()
            || profile.eq_ignore_ascii_case("default")
            || profile.eq_ignore_ascii_case("build")
        {
            return self.agents.build;
        }
        if profile.eq_ignore_ascii_case("plan") {
            return self.agents.plan;
        }
        if profile.eq_ignore_ascii_case("docs") {
            return self.agents.docs;
        }
        if profile.eq_ignore_ascii_case("ask") {
            return self.agents.ask;
        }

        let hash = profile
            .to_ascii_lowercase()
            .bytes()
            .fold(0usize, |hash, byte| {
                hash.wrapping_mul(31).wrapping_add(usize::from(byte))
            });
        self.agents.palette[hash % self.agents.palette.len()]
    }

    pub fn harness_dark() -> Self {
        Self {
            surface: SurfaceColors {
                canvas: rgb(0x0B, 0x0E, 0x14),
                shell: rgb(0x0B, 0x0E, 0x14),
                panel: rgb(0x0B, 0x0E, 0x14),
                panel_elevated: rgb(0x12, 0x16, 0x1E),
                overlay: rgb(0x0B, 0x0E, 0x14),
                card: rgb(0x55, 0x57, 0x53),
            },
            border: BorderColors {
                subtle: rgb(0x3A, 0x3D, 0x43),
                strong: rgb(0x48, 0x4B, 0x52),
                focus: rgb(0x60, 0x63, 0x6A),
            },
            text: TextColors {
                primary: rgb(0xEE, 0xEE, 0xEC),
                secondary: rgb(0x88, 0x8B, 0x91),
                tertiary: rgb(0x88, 0x8B, 0x91),
                accent: rgb(0xD9, 0x84, 0xD9),
                inverse: rgb(0x0B, 0x0E, 0x14),
            },
            question_prompt: QuestionPromptColors {
                accent: rgb(0xD9, 0x84, 0xD9),
                secondary: rgb(0x5C, 0x9C, 0xF5),
            },
            status: StatusColors {
                success: rgb(0x7F, 0xD8, 0x8F),
                warning: rgb(0xE5, 0xC0, 0x7B),
                error: rgb(0xE0, 0x6C, 0x75),
                info: rgb(0x56, 0xB6, 0xC2),
                disabled: rgb(0x80, 0x80, 0x80),
            },
            markdown: MarkdownColors {
                heading: rgb(0xD9, 0x84, 0xD9),
                link: rgb(0xE8, 0xA0, 0xE8),
                link_text: rgb(0x56, 0xB6, 0xC2),
                code: rgb(0x7F, 0xD8, 0x8F),
                emph: rgb(0xE5, 0xC0, 0x7B),
                strong: rgb(0xD9, 0x84, 0xD9),
                block_quote: rgb(0xE5, 0xC0, 0x7B),
                list_item: rgb(0xE8, 0xA0, 0xE8),
                list_enum: rgb(0x56, 0xB6, 0xC2),
                rule: rgb(0x80, 0x80, 0x80),
            },
            agents: AgentColors {
                build: rgb(0x5C, 0x9C, 0xF5),
                plan: rgb(0xD9, 0x84, 0xD9),
                docs: rgb(0xE5, 0xC0, 0x7B),
                ask: rgb(0xE8, 0xA0, 0xE8),
                palette: [
                    rgb(0x5C, 0x9C, 0xF5),
                    rgb(0xD9, 0x84, 0xD9),
                    rgb(0x7F, 0xD8, 0x8F),
                    rgb(0xE5, 0xC0, 0x7B),
                    rgb(0xE8, 0xA0, 0xE8),
                    rgb(0xE0, 0x6C, 0x75),
                    rgb(0x56, 0xB6, 0xC2),
                ],
            },
            scrollbar: ScrollbarColors {
                track: rgb(0x0B, 0x0E, 0x14),
                thumb: rgb(0x32, 0x36, 0x3C),
                thumb_active: rgb(0x60, 0x63, 0x6A),
            },
            live_shell: Self::HARNESS_DARK_SHELL,
        }
    }

    pub fn harness_high_contrast() -> Self {
        Self {
            surface: SurfaceColors {
                canvas: Color::Black,
                shell: Color::Black,
                panel: Color::Black,
                panel_elevated: Color::Black,
                overlay: Color::Black,
                card: Color::DarkGray,
            },
            border: BorderColors {
                subtle: Color::DarkGray,
                strong: Color::Gray,
                focus: Color::Yellow,
            },
            text: TextColors {
                primary: Color::White,
                secondary: Color::Gray,
                tertiary: Color::DarkGray,
                accent: Color::Yellow,
                inverse: Color::Black,
            },
            question_prompt: QuestionPromptColors {
                accent: Color::Yellow,
                secondary: Color::Cyan,
            },
            status: StatusColors {
                success: Color::LightGreen,
                warning: Color::Yellow,
                error: Color::LightRed,
                info: Color::LightCyan,
                disabled: Color::DarkGray,
            },
            markdown: MarkdownColors {
                heading: Color::Magenta,
                link: Color::Yellow,
                link_text: Color::Cyan,
                code: Color::LightGreen,
                emph: Color::Yellow,
                strong: Color::Yellow,
                block_quote: Color::Yellow,
                list_item: Color::Yellow,
                list_enum: Color::Cyan,
                rule: Color::Gray,
            },
            agents: AgentColors {
                build: Color::Cyan,
                plan: Color::Magenta,
                docs: Color::Yellow,
                ask: Color::LightYellow,
                palette: [
                    Color::Cyan,
                    Color::Magenta,
                    Color::LightGreen,
                    Color::Yellow,
                    Color::LightYellow,
                    Color::LightRed,
                    Color::LightCyan,
                ],
            },
            scrollbar: ScrollbarColors {
                track: Color::Black,
                thumb: Color::DarkGray,
                thumb_active: Color::Yellow,
            },
            live_shell: Self::HARNESS_DARK_SHELL,
        }
    }

    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "default" | "harness-dark" => Some(Self::harness_dark()),
            "high-contrast" => Some(Self::harness_high_contrast()),
            _ => None,
        }
    }

    pub const fn available_theme_names() -> &'static [&'static str] {
        &["default", "high-contrast"]
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
mod tests;
