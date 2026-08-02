// allow: SIZE_OK — TUI theme tokens (color system + shell geometry)
use ratatui::style::{Color, Modifier, Style};

pub const DIFF_SIDE_BY_SIDE_MIN_WIDTH: u16 = 96;

const fn rgb(red: u8, green: u8, blue: u8) -> Color {
    Color::Rgb(red, green, blue)
}

// ---------------------------------------------------------------------------
// Terminal color capability and quantization
// ---------------------------------------------------------------------------

/// Terminal color support level (ordered low → high).
///
/// Mirrors the reference binary's `ColorLevel`: the theme is defined in
/// truecolor RGB and degrades cleanly to 256-color, 16-color, or monochrome
/// based on the terminal's detected capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum ColorLevel {
    /// No color support (monochrome / `NO_COLOR`).
    None,
    /// Basic 16-color ANSI (SGR 30–37 / 90–97).
    Basic,
    /// 256-color indexed palette (SGR 38;5;N).
    Ansi256,
    /// 24-bit truecolor RGB (SGR 38;2;R;G;B).
    #[default]
    TrueColor,
}

impl ColorLevel {
    pub fn has_color(self) -> bool {
        self >= Self::Basic
    }

    pub fn has_256(self) -> bool {
        self >= Self::Ansi256
    }

    pub fn has_truecolor(self) -> bool {
        self >= Self::TrueColor
    }

    /// Canonical lowercase spelling for diagnostics and env-var parsing.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Basic => "basic",
            Self::Ansi256 => "256",
            Self::TrueColor => "truecolor",
        }
    }

    /// Parse from a canonical or common spelling (case-insensitive).
    pub fn from_str_ci(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" | "mono" | "monochrome" => Some(Self::None),
            "basic" | "16" | "ansi16" => Some(Self::Basic),
            "256" | "ansi256" => Some(Self::Ansi256),
            "truecolor" | "24bit" | "true" | "rgb" => Some(Self::TrueColor),
            _ => None,
        }
    }
}

impl std::fmt::Display for ColorLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The 6 channel values in the 256-color 6×6×6 cube.
const CUBE_VALUES: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Convert a 256-color indexed color to its (R, G, B) components.
///
/// Handles all three regions of the 256-color palette:
/// - 0–15:    standard/bright ANSI colors (common xterm defaults)
/// - 16–231:  6×6×6 color cube
/// - 232–255: 24-step grayscale ramp
pub fn indexed_to_rgb(index: u8) -> (u8, u8, u8) {
    match index {
        0 => (0, 0, 0),
        1 => (128, 0, 0),
        2 => (0, 128, 0),
        3 => (128, 128, 0),
        4 => (0, 0, 128),
        5 => (128, 0, 128),
        6 => (0, 128, 128),
        7 => (192, 192, 192),
        8 => (128, 128, 128),
        9 => (255, 0, 0),
        10 => (0, 255, 0),
        11 => (255, 255, 0),
        12 => (0, 0, 255),
        13 => (255, 0, 255),
        14 => (0, 255, 255),
        15 => (255, 255, 255),
        16..=231 => {
            let n = index - 16;
            let r = CUBE_VALUES[(n / 36) as usize];
            let g = CUBE_VALUES[((n % 36) / 6) as usize];
            let b = CUBE_VALUES[(n % 6) as usize];
            (r, g, b)
        }
        232..=255 => {
            let v = 8 + (index - 232) * 10;
            (v, v, v)
        }
    }
}

/// Map an RGB triplet to the nearest 256-color palette index (16–255).
pub fn nearest_indexed(r: u8, g: u8, b: u8) -> u8 {
    let ri = nearest_cube_channel(r);
    let gi = nearest_cube_channel(g);
    let bi = nearest_cube_channel(b);
    let cube_idx = 16 + 36 * ri as u16 + 6 * gi as u16 + bi as u16;
    let cube_dist = sq_dist(
        r,
        g,
        b,
        CUBE_VALUES[ri as usize],
        CUBE_VALUES[gi as usize],
        CUBE_VALUES[bi as usize],
    );

    let lum = (r as u16 + g as u16 + b as u16) / 3;
    let gray_step = if lum <= 3 {
        0u8
    } else if lum >= 243 {
        23
    } else {
        ((lum as i16 - 8 + 5) / 10).clamp(0, 23) as u8
    };
    let gv = (8 + gray_step as u16 * 10) as u8;
    let gray_dist = sq_dist(r, g, b, gv, gv, gv);

    if gray_dist < cube_dist {
        232 + gray_step
    } else {
        cube_idx as u8
    }
}

fn nearest_cube_channel(v: u8) -> u8 {
    let mut best = 0u8;
    let mut best_d = v.abs_diff(CUBE_VALUES[0]) as u16;
    for i in 1..6u8 {
        let d = v.abs_diff(CUBE_VALUES[i as usize]) as u16;
        if d < best_d {
            best = i;
            best_d = d;
        }
    }
    best
}

fn sq_dist(r1: u8, g1: u8, b1: u8, r2: u8, g2: u8, b2: u8) -> u32 {
    let dr = r1 as i32 - r2 as i32;
    let dg = g1 as i32 - g2 as i32;
    let db = b1 as i32 - b2 as i32;
    (dr * dr + dg * dg + db * db) as u32
}

/// Find the nearest ANSI 16 color for an RGB triplet.
fn rgb_to_ansi16(r: u8, g: u8, b: u8) -> Color {
    const PALETTE: [(u8, u8, u8, Color); 16] = [
        (0, 0, 0, Color::Black),
        (128, 0, 0, Color::Red),
        (0, 128, 0, Color::Green),
        (128, 128, 0, Color::Yellow),
        (0, 0, 128, Color::Blue),
        (128, 0, 128, Color::Magenta),
        (0, 128, 128, Color::Cyan),
        (192, 192, 192, Color::White),
        (128, 128, 128, Color::DarkGray),
        (255, 0, 0, Color::LightRed),
        (0, 255, 0, Color::LightGreen),
        (255, 255, 0, Color::LightYellow),
        (0, 0, 255, Color::LightBlue),
        (255, 0, 255, Color::LightMagenta),
        (0, 255, 255, Color::LightCyan),
        (255, 255, 255, Color::White),
    ];

    let mut best = Color::White;
    let mut best_dist = u32::MAX;
    for &(pr, pg, pb, color) in &PALETTE {
        let dist = sq_dist(r, g, b, pr, pg, pb);
        if dist < best_dist {
            best_dist = dist;
            best = color;
        }
    }
    best
}

/// Map a 256-color index to the nearest basic ANSI 16 color.
fn indexed_to_ansi16(n: u8) -> Color {
    match n {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::White,
        8 => Color::DarkGray,
        9 => Color::LightRed,
        10 => Color::LightGreen,
        11 => Color::LightYellow,
        12 => Color::LightBlue,
        13 => Color::LightMagenta,
        14 => Color::LightCyan,
        15 => Color::White,
        _ => {
            let (r, g, b) = indexed_to_rgb(n);
            rgb_to_ansi16(r, g, b)
        }
    }
}

/// Downgrade a [`Color`] to the highest representation the terminal supports.
///
/// | Terminal level | `Rgb`            | `Indexed`         | Named (`Red`…) |
/// |----------------|------------------|--------------------|----------------|
/// | TrueColor      | pass-through     | pass-through       | pass-through   |
/// | Ansi256        | → nearest idx    | pass-through       | pass-through   |
/// | Basic          | → nearest ANSI16 | → nearest ANSI16   | pass-through   |
/// | None           | → `Reset`        | → `Reset`          | → `Reset`      |
pub fn quantize_color(color: Color, level: ColorLevel) -> Color {
    match level {
        ColorLevel::TrueColor => color,
        ColorLevel::Ansi256 => match color {
            Color::Rgb(r, g, b) => Color::Indexed(nearest_indexed(r, g, b)),
            other => other,
        },
        ColorLevel::Basic => match color {
            Color::Rgb(r, g, b) => indexed_to_ansi16(nearest_indexed(r, g, b)),
            Color::Indexed(n) => indexed_to_ansi16(n),
            other => other,
        },
        ColorLevel::None => Color::Reset,
    }
}

/// Resolve any `Color` variant back to an RGB triple.
///
/// Returns `None` for `Color::Reset` (terminal default — unknown at
/// theme-definition time).
pub fn resolve_to_rgb(color: Color) -> Option<(u8, u8, u8)> {
    let idx: u8 = match color {
        Color::Rgb(r, g, b) => return Some((r, g, b)),
        Color::Indexed(n) => return Some(indexed_to_rgb(n)),
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
        Color::Reset => return None,
    };
    Some(indexed_to_rgb(idx))
}

/// Detect the terminal's color support from environment variables (pure; no I/O).
///
/// Checks `NO_COLOR`, `COLORTERM`, and `TERM` to determine the highest color
/// level the terminal supports. When `NO_COLOR` is set, returns [`ColorLevel::None`].
/// When `COLORTERM` contains `truecolor` or `24bit`, returns [`ColorLevel::TrueColor`].
/// Otherwise falls back to [`ColorLevel::Ansi256`] for known terminals or
/// [`ColorLevel::Basic`] for `TERM=dumb`.
pub fn detect_color_level(
    no_color: Option<&str>,
    colorterm: Option<&str>,
    term: Option<&str>,
) -> ColorLevel {
    if no_color.is_some() {
        return ColorLevel::None;
    }

    let has_truecolor = colorterm
        .map(|ct| {
            let lower = ct.to_ascii_lowercase();
            lower.contains("truecolor") || lower.contains("24bit")
        })
        .unwrap_or(false);

    if has_truecolor {
        return ColorLevel::TrueColor;
    }

    let lower_term = term.unwrap_or("").to_ascii_lowercase();
    if lower_term == "dumb" || lower_term.is_empty() {
        return ColorLevel::Basic;
    }

    // Most modern terminals that set TERM to xterm-* or screen-* support
    // at least 256 colors even without COLORTERM.
    if lower_term.starts_with("xterm")
        || lower_term.starts_with("screen")
        || lower_term.starts_with("tmux")
        || lower_term.starts_with("rxvt")
        || lower_term.starts_with("alacritty")
        || lower_term.starts_with("kitty")
        || lower_term.starts_with("foot")
    {
        ColorLevel::Ansi256
    } else {
        ColorLevel::Basic
    }
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
pub enum StatusRole {
    Success,
    Warning,
    Error,
    Info,
    Disabled,
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

    pub fn harness_light() -> Self {
        Self {
            surface: SurfaceColors {
                canvas: rgb(0xF5, 0xF5, 0xF0),
                shell: rgb(0xF5, 0xF5, 0xF0),
                panel: rgb(0xF5, 0xF5, 0xF0),
                panel_elevated: rgb(0xEF, 0xEF, 0xEA),
                overlay: rgb(0xFA, 0xFA, 0xF5),
                card: rgb(0xD4, 0xD4, 0xCF),
            },
            border: BorderColors {
                subtle: rgb(0xC8, 0xC8, 0xC3),
                strong: rgb(0xA0, 0xA0, 0x9B),
                focus: rgb(0x70, 0x70, 0x6B),
            },
            text: TextColors {
                primary: rgb(0x1E, 0x1E, 0x1E),
                secondary: rgb(0x5A, 0x5A, 0x5A),
                tertiary: rgb(0x7A, 0x7A, 0x7A),
                accent: rgb(0x7B, 0x2D, 0x8E),
                inverse: rgb(0xF5, 0xF5, 0xF0),
            },
            question_prompt: QuestionPromptColors {
                accent: rgb(0x7B, 0x2D, 0x8E),
                secondary: rgb(0x1A, 0x5C, 0xB0),
            },
            status: StatusColors {
                success: rgb(0x1A, 0x7F, 0x3D),
                warning: rgb(0xB8, 0x86, 0x0B),
                error: rgb(0xC0, 0x39, 0x2B),
                info: rgb(0x1A, 0x6B, 0x8A),
                disabled: rgb(0xA0, 0xA0, 0xA0),
            },
            markdown: MarkdownColors {
                heading: rgb(0x7B, 0x2D, 0x8E),
                link: rgb(0x9C, 0x4A, 0xAE),
                link_text: rgb(0x1A, 0x6B, 0x8A),
                code: rgb(0x1A, 0x7F, 0x3D),
                emph: rgb(0xB8, 0x86, 0x0B),
                strong: rgb(0x7B, 0x2D, 0x8E),
                block_quote: rgb(0xB8, 0x86, 0x0B),
                list_item: rgb(0x9C, 0x4A, 0xAE),
                list_enum: rgb(0x1A, 0x6B, 0x8A),
                rule: rgb(0xC8, 0xC8, 0xC3),
            },
            agents: AgentColors {
                build: rgb(0x1A, 0x5C, 0xB0),
                plan: rgb(0x7B, 0x2D, 0x8E),
                docs: rgb(0xB8, 0x86, 0x0B),
                ask: rgb(0x9C, 0x4A, 0xAE),
                palette: [
                    rgb(0x1A, 0x5C, 0xB0),
                    rgb(0x7B, 0x2D, 0x8E),
                    rgb(0x1A, 0x7F, 0x3D),
                    rgb(0xB8, 0x86, 0x0B),
                    rgb(0x9C, 0x4A, 0xAE),
                    rgb(0xC0, 0x39, 0x2B),
                    rgb(0x1A, 0x6B, 0x8A),
                ],
            },
            scrollbar: ScrollbarColors {
                track: rgb(0xF5, 0xF5, 0xF0),
                thumb: rgb(0xC8, 0xC8, 0xC3),
                thumb_active: rgb(0xA0, 0xA0, 0x9B),
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
            "harness-light" | "light" => Some(Self::harness_light()),
            "high-contrast" => Some(Self::harness_high_contrast()),
            "terminal-native" => Some(Self::terminal_native()),
            _ => None,
        }
    }

    pub const fn available_theme_names() -> &'static [&'static str] {
        &["default", "harness-light", "high-contrast"]
    }

    /// Return a copy with every color quantized to the given level.
    ///
    /// TrueColor passes through unchanged; Ansi256 maps RGB to the nearest
    /// indexed palette entry; Basic maps to ANSI16 named colors; None strips
    /// all color to `Color::Reset`. Modifiers (bold/dim) are preserved.
    pub fn quantized(self, level: ColorLevel) -> Self {
        let q = |c: Color| quantize_color(c, level);
        Self {
            surface: SurfaceColors {
                canvas: q(self.surface.canvas),
                shell: q(self.surface.shell),
                panel: q(self.surface.panel),
                panel_elevated: q(self.surface.panel_elevated),
                overlay: q(self.surface.overlay),
                card: q(self.surface.card),
            },
            border: BorderColors {
                subtle: q(self.border.subtle),
                strong: q(self.border.strong),
                focus: q(self.border.focus),
            },
            text: TextColors {
                primary: q(self.text.primary),
                secondary: q(self.text.secondary),
                tertiary: q(self.text.tertiary),
                accent: q(self.text.accent),
                inverse: q(self.text.inverse),
            },
            question_prompt: QuestionPromptColors {
                accent: q(self.question_prompt.accent),
                secondary: q(self.question_prompt.secondary),
            },
            status: StatusColors {
                success: q(self.status.success),
                warning: q(self.status.warning),
                error: q(self.status.error),
                info: q(self.status.info),
                disabled: q(self.status.disabled),
            },
            markdown: MarkdownColors {
                heading: q(self.markdown.heading),
                link: q(self.markdown.link),
                link_text: q(self.markdown.link_text),
                code: q(self.markdown.code),
                emph: q(self.markdown.emph),
                strong: q(self.markdown.strong),
                block_quote: q(self.markdown.block_quote),
                list_item: q(self.markdown.list_item),
                list_enum: q(self.markdown.list_enum),
                rule: q(self.markdown.rule),
            },
            agents: AgentColors {
                build: q(self.agents.build),
                plan: q(self.agents.plan),
                docs: q(self.agents.docs),
                ask: q(self.agents.ask),
                palette: self.agents.palette.map(q),
            },
            scrollbar: ScrollbarColors {
                track: q(self.scrollbar.track),
                thumb: q(self.scrollbar.thumb),
                thumb_active: q(self.scrollbar.thumb_active),
            },
            live_shell: self.live_shell,
        }
    }

    /// Pin chrome and semantic-accent colors to ANSI-named entries so
    /// they survive 16-color quantization.
    ///
    /// On a dark canvas: bright variants (Light*) for semantic accents,
    /// DarkGray/Gray/White for chrome hierarchy. On a light canvas: normal
    /// variants (idx 1–7) for accents, Gray/DarkGray/Black for chrome.
    ///
    /// Applied after `quantized(Basic)` to restore the chromatic signal
    /// that naive nearest-RGB quantization erases.
    pub fn ansi16_chrome_overrides(self, dark: bool) -> Self {
        let canvas_bg = if dark { Color::Black } else { Color::White };
        let elevated_bg = if dark { Color::DarkGray } else { Color::Gray };
        let high_contrast_fg = if dark { Color::White } else { Color::Black };
        let muted_fg = if dark { Color::Gray } else { Color::DarkGray };
        let dim_fg = if dark { Color::DarkGray } else { Color::Gray };

        let red = if dark { Color::LightRed } else { Color::Red };
        let green = if dark {
            Color::LightGreen
        } else {
            Color::Green
        };
        let yellow = if dark {
            Color::LightYellow
        } else {
            Color::Yellow
        };
        let blue = if dark { Color::LightBlue } else { Color::Blue };
        let magenta = if dark {
            Color::LightMagenta
        } else {
            Color::Magenta
        };
        let cyan = if dark { Color::LightCyan } else { Color::Cyan };

        Self {
            surface: SurfaceColors {
                canvas: canvas_bg,
                shell: canvas_bg,
                panel: canvas_bg,
                panel_elevated: elevated_bg,
                overlay: elevated_bg,
                card: elevated_bg,
            },
            border: BorderColors {
                subtle: dim_fg,
                strong: muted_fg,
                focus: high_contrast_fg,
            },
            text: TextColors {
                primary: high_contrast_fg,
                secondary: muted_fg,
                tertiary: dim_fg,
                accent: magenta,
                inverse: canvas_bg,
            },
            question_prompt: QuestionPromptColors {
                accent: magenta,
                secondary: blue,
            },
            status: StatusColors {
                success: green,
                warning: yellow,
                error: red,
                info: cyan,
                disabled: dim_fg,
            },
            markdown: MarkdownColors {
                heading: magenta,
                link: blue,
                link_text: cyan,
                code: green,
                emph: yellow,
                strong: magenta,
                block_quote: yellow,
                list_item: magenta,
                list_enum: cyan,
                rule: dim_fg,
            },
            agents: AgentColors {
                build: blue,
                plan: magenta,
                docs: yellow,
                ask: magenta,
                palette: [blue, magenta, green, yellow, magenta, red, cyan],
            },
            scrollbar: ScrollbarColors {
                track: canvas_bg,
                thumb: muted_fg,
                thumb_active: high_contrast_fg,
            },
            live_shell: self.live_shell,
        }
    }

    /// Whether this theme reads as dark per BT.709 luminance of its canvas.
    ///
    /// Must be called pre-quantization while `surface.canvas` is still RGB;
    /// named/Reset fall back to "dark" (the default theme polarity).
    pub fn is_dark(self) -> bool {
        let (r, g, b) = match self.surface.canvas {
            Color::Rgb(r, g, b) => (r, g, b),
            Color::Indexed(n) => indexed_to_rgb(n),
            _ => return true,
        };
        // BT.709 luminance: 0.2126R + 0.7152G + 0.0722B
        let luminance = 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
        luminance < 128.0
    }

    /// Terminal-native theme: uses `Color::Reset` for all backgrounds and
    /// default foreground, with named ANSI-16 accents for semantic roles.
    ///
    /// Matches the reference binary's terminal-native mode: the core shell
    /// surfaces defer to the terminal's own fg/bg (polarity-safe on any
    /// profile), while state signals (error/success/warning) use ANSI-16
    /// hues that survive 16-color quantization. De-emphasis is via
    /// `Modifier::DIM`, not hard-coded grays.
    pub fn terminal_native() -> Self {
        Self {
            surface: SurfaceColors {
                canvas: Color::Reset,
                shell: Color::Reset,
                panel: Color::Reset,
                panel_elevated: Color::Reset,
                overlay: Color::Reset,
                card: Color::Reset,
            },
            border: BorderColors {
                subtle: Color::Reset,
                strong: Color::Reset,
                focus: Color::Reset,
            },
            text: TextColors {
                primary: Color::Reset,
                secondary: Color::Reset,
                tertiary: Color::Reset,
                accent: Color::Magenta,
                inverse: Color::Reset,
            },
            question_prompt: QuestionPromptColors {
                accent: Color::Magenta,
                secondary: Color::Blue,
            },
            status: StatusColors {
                success: Color::Green,
                warning: Color::Yellow,
                error: Color::Red,
                info: Color::Cyan,
                disabled: Color::Reset,
            },
            markdown: MarkdownColors {
                heading: Color::Reset,
                link: Color::Blue,
                link_text: Color::Cyan,
                code: Color::Cyan,
                emph: Color::Reset,
                strong: Color::Reset,
                block_quote: Color::Reset,
                list_item: Color::Reset,
                list_enum: Color::Reset,
                rule: Color::Reset,
            },
            agents: AgentColors {
                build: Color::Blue,
                plan: Color::Yellow,
                docs: Color::Yellow,
                ask: Color::Magenta,
                palette: [
                    Color::Blue,
                    Color::Magenta,
                    Color::Green,
                    Color::Yellow,
                    Color::Magenta,
                    Color::Red,
                    Color::Cyan,
                ],
            },
            scrollbar: ScrollbarColors {
                track: Color::Reset,
                thumb: Color::Reset,
                thumb_active: Color::Reset,
            },
            live_shell: Self::HARNESS_DARK_SHELL,
        }
    }

    /// Select a theme appropriate for the terminal's color capability.
    ///
    /// On TrueColor terminals the explicit RGB theme is used. On Ansi256
    /// the theme is quantized to indexed colors. On Basic the theme is
    /// quantized to ANSI16 and chrome overrides are applied. On None the
    /// terminal-native theme is used (Reset + modifiers only).
    pub fn for_color_level(self, level: ColorLevel) -> Self {
        match level {
            ColorLevel::TrueColor => self,
            ColorLevel::Ansi256 => self.quantized(level),
            ColorLevel::Basic => {
                let dark = self.is_dark();
                self.quantized(level).ansi16_chrome_overrides(dark)
            }
            ColorLevel::None => Self::terminal_native(),
        }
    }

    pub fn primary_text_style(self) -> Style {
        Style::new().fg(self.text.primary)
    }

    pub fn secondary_text_style(self) -> Style {
        Style::new().fg(self.text.secondary)
    }

    pub fn accent_text_style(self) -> Style {
        Style::new()
            .fg(self.text.accent)
            .add_modifier(Modifier::BOLD)
    }

    /// Muted text style: fg + DIM modifier (used for secondary chrome).
    pub fn muted_text_style(self) -> Style {
        Style::new()
            .fg(self.text.secondary)
            .add_modifier(Modifier::DIM)
    }

    /// Dim text style: fg + DIM modifier (used for truly faded chrome).
    pub fn dim_text_style(self) -> Style {
        Style::new()
            .fg(self.text.tertiary)
            .add_modifier(Modifier::DIM)
    }

    pub fn status_style(self, role: StatusRole) -> Style {
        match role {
            StatusRole::Success => Style::new().fg(self.status.success),
            StatusRole::Warning => Style::new().fg(self.status.warning),
            StatusRole::Error => Style::new().fg(self.status.error),
            StatusRole::Info => Style::new().fg(self.status.info),
            StatusRole::Disabled => Style::new().fg(self.status.disabled),
        }
    }

    pub fn border_style(self, intensity: DividerIntensity) -> Style {
        match intensity {
            DividerIntensity::None => Style::new(),
            DividerIntensity::Subtle => Style::new().fg(self.border.subtle),
            DividerIntensity::Strong => Style::new().fg(self.border.strong),
            DividerIntensity::Focus => Style::new().fg(self.border.focus),
        }
    }

    pub fn chrome_style(self, mode: ChromeMode) -> Style {
        match mode {
            ChromeMode::Chromeless => Style::new().bg(self.surface.shell),
            ChromeMode::Divided => Style::new().bg(self.surface.panel).fg(self.border.subtle),
            ChromeMode::Card => Style::new().bg(self.surface.overlay).fg(self.border.subtle),
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
mod tests;
