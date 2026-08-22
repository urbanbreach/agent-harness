//! Terminal brand detection and brand-level capability conditionals.
//!
use super::env::TerminalEnv;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TerminalName {
    AppleTerminal,
    Ghostty,
    Iterm2,
    WarpTerminal,
    VsCode,
    Cursor,
    Windsurf,
    Zed,
    WezTerm,
    Kitty,
    Alacritty,
    Rio,
    Foot,
    JetBrains,
    Vte,
    Terminator,
    WindowsTerminal,
    Otty,
    #[default]
    Unknown,
}

impl TerminalName {
    pub const ALL: [Self; 19] = [
        Self::AppleTerminal,
        Self::Ghostty,
        Self::Iterm2,
        Self::WarpTerminal,
        Self::VsCode,
        Self::Cursor,
        Self::Windsurf,
        Self::Zed,
        Self::WezTerm,
        Self::Kitty,
        Self::Alacritty,
        Self::Rio,
        Self::Foot,
        Self::JetBrains,
        Self::Vte,
        Self::Terminator,
        Self::WindowsTerminal,
        Self::Otty,
        Self::Unknown,
    ];

    /// Stable machine-facing identifier matching the inventory symbol suffix.
    pub const fn key(self) -> &'static str {
        match self {
            Self::AppleTerminal => "AppleTerminal",
            Self::Ghostty => "Ghostty",
            Self::Iterm2 => "Iterm2",
            Self::WarpTerminal => "WarpTerminal",
            Self::VsCode => "VsCode",
            Self::Cursor => "Cursor",
            Self::Windsurf => "Windsurf",
            Self::Zed => "Zed",
            Self::WezTerm => "WezTerm",
            Self::Kitty => "Kitty",
            Self::Alacritty => "Alacritty",
            Self::Rio => "Rio",
            Self::Foot => "Foot",
            Self::JetBrains => "JetBrains",
            Self::Vte => "Vte",
            Self::Terminator => "Terminator",
            Self::WindowsTerminal => "WindowsTerminal",
            Self::Otty => "Otty",
            Self::Unknown => "Unknown",
        }
    }

    /// Pure brand detection from an environment snapshot. Discriminators are
    /// checked most-specific first so VSCode-family forks (Cursor, Windsurf)
    /// resolve to their own brand rather than collapsing onto `VsCode`.
    pub fn detect(env: &TerminalEnv) -> Self {
        if env.cursor_session.is_some() {
            return Self::Cursor;
        }
        if env.windsurf.is_some() {
            return Self::Windsurf;
        }
        if env.ghostty_resources_dir.is_some() || env.term_program_is("ghostty") {
            return Self::Ghostty;
        }
        if env.kitty_window_id.is_some()
            || env.term_program_is("kitty")
            || env.term_contains("kitty")
        {
            return Self::Kitty;
        }
        if env.warp_session_id.is_some() || env.term_program_is("warpterminal") {
            return Self::WarpTerminal;
        }
        if env.terminator_uuid.is_some() {
            return Self::Terminator;
        }
        if env.wt_session.is_some() {
            return Self::WindowsTerminal;
        }
        if env.terminal_emulator_contains("jetbrains") {
            return Self::JetBrains;
        }
        if env.rio_log_level.is_some() || env.term_program_is("rio") || env.term_contains("rio") {
            return Self::Rio;
        }
        if env.term_starts_with("foot") {
            return Self::Foot;
        }
        if env.term_contains("alacritty") {
            return Self::Alacritty;
        }
        if env.term_program_is("zed") {
            return Self::Zed;
        }
        if env.term_program_is("wezterm") {
            return Self::WezTerm;
        }
        if env.term_program_is("iterm.app") || env.lc_terminal_is("iterm2") {
            return Self::Iterm2;
        }
        if env.term_program_is("otty") {
            return Self::Otty;
        }
        if env.term_program_is("apple_terminal") {
            return Self::AppleTerminal;
        }
        if env.term_program_is("vscode") {
            return Self::VsCode;
        }
        if env.vte_version.is_some() || env.term_contains("vte") {
            return Self::Vte;
        }
        Self::Unknown
    }

    /// VTE-based backend (GNOME terminal stack). Terminator runs on VTE.
    pub const fn is_vte_based(self) -> bool {
        matches!(self, Self::Vte | Self::Terminator)
    }

    /// Part of the VSCode terminal family (VSCode and its forks).
    pub const fn is_vscode_family(self) -> bool {
        matches!(self, Self::VsCode | Self::Cursor | Self::Windsurf)
    }

    /// No reliable capability profile is known for this brand.
    pub const fn is_capability_unclassified(self) -> bool {
        matches!(self, Self::Unknown | Self::Otty)
    }

    /// Supports OSC 52 clipboard write. Allowlist of terminals with confirmed
    /// support; everything else degrades to native/disabled clipboard.
    pub const fn supports_osc52_clipboard(self) -> bool {
        matches!(
            self,
            Self::Ghostty
                | Self::WezTerm
                | Self::Kitty
                | Self::Alacritty
                | Self::Iterm2
                | Self::Foot
                | Self::Rio
                | Self::Zed
                | Self::VsCode
                | Self::Cursor
                | Self::Windsurf
        )
    }

    /// Delivers IME composition text via the bracketed-paste path instead of
    /// the enhanced keyboard protocol.
    pub const fn delivers_ime_as_bracketed_paste(self) -> bool {
        matches!(self, Self::Vte | Self::AppleTerminal | Self::JetBrains)
    }

    /// Intercepts device-attribute / capability CSI queries, so probe-based
    /// feature detection cannot rely on a response.
    pub const fn intercepts_csi_queries(self) -> bool {
        matches!(self, Self::JetBrains | Self::WarpTerminal)
    }

    /// Supports the enhanced (Kitty / CSI u) keyboard protocol, which
    /// disambiguates modified keys such as Ctrl+arrows and Shift+Enter.
    pub const fn supports_enhanced_keyboard(self) -> bool {
        matches!(
            self,
            Self::Ghostty
                | Self::Kitty
                | Self::WezTerm
                | Self::Iterm2
                | Self::Alacritty
                | Self::Zed
                | Self::Foot
                | Self::Rio
                | Self::VsCode
                | Self::Cursor
                | Self::Windsurf
                | Self::WindowsTerminal
                | Self::WarpTerminal
        )
    }
}
