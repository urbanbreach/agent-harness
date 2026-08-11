use super::axes::*;

pub struct CapabilityClassifier {
    pub(crate) env_term: String,
    pub(crate) env_term_program: String,
    pub(crate) env_colorterm: String,
    pub(crate) env_tmux: bool,
    pub(crate) env_zellij: bool,
    pub(crate) env_ssh: bool,
    pub(crate) env_wt: bool,
    pub(crate) env_no_color: bool,
    pub(crate) env_vte_version: Option<u32>,
}

impl CapabilityClassifier {
    pub fn from_env() -> Self {
        Self::new(
            std::env::var("TERM").unwrap_or_default(),
            std::env::var("TERM_PROGRAM").unwrap_or_default(),
            std::env::var("COLORTERM").unwrap_or_default(),
            std::env::var_os("TMUX").is_some(),
            std::env::var_os("ZELLIJ").is_some(),
            std::env::var_os("SSH_CONNECTION").is_some(),
            std::env::var_os("WT_SESSION").is_some(),
            std::env::var_os("NO_COLOR").is_some(),
            std::env::var("VTE_VERSION")
                .ok()
                .and_then(|v| v.parse().ok()),
        )
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "constructor parameters map directly to terminal environment fields"
    )]
    pub fn new(
        term: String,
        program: String,
        colorterm: String,
        tmux: bool,
        zellij: bool,
        ssh: bool,
        wt: bool,
        no_color: bool,
        vte_version: Option<u32>,
    ) -> Self {
        Self {
            env_term: term,
            env_term_program: program,
            env_colorterm: colorterm,
            env_tmux: tmux,
            env_zellij: zellij,
            env_ssh: ssh,
            env_wt: wt,
            env_no_color: no_color,
            env_vte_version: vte_version,
        }
    }
    fn modern_program(&self) -> bool {
        matches!(
            self.env_term_program.to_ascii_lowercase().as_str(),
            "wezterm" | "kitty" | "alacritty"
        )
    }
    pub fn color(&self) -> ColorCapability {
        if self.env_no_color {
            return ColorCapability::NoColor;
        }
        if self
            .env_colorterm
            .to_ascii_lowercase()
            .contains("truecolor")
            || self.env_colorterm.contains("24bit")
            || self.modern_program()
        {
            return ColorCapability::TrueColor;
        }
        if self.env_term.contains("256") {
            ColorCapability::Ansi256
        } else if self.env_term.contains("color")
            || self.env_term.starts_with("xterm")
            || self.env_term.starts_with("screen")
        {
            ColorCapability::Basic16
        } else {
            ColorCapability::NoColor
        }
    }
    pub fn graphics(&self) -> GraphicsCapability {
        match self.env_term_program.to_ascii_lowercase().as_str() {
            "wezterm" | "kitty" => GraphicsCapability::Kitty,
            "iterm.app" => GraphicsCapability::ITerm2,
            _ => GraphicsCapability::None,
        }
    }
    pub fn keyboard(&self) -> KeyboardCapability {
        if matches!(
            self.env_term_program.to_ascii_lowercase().as_str(),
            "wezterm" | "kitty" | "alacritty" | "foot"
        ) {
            KeyboardCapability::Modern
        } else if self.env_term.contains("xterm") || self.env_term.contains("gnome") {
            KeyboardCapability::Legacy
        } else {
            KeyboardCapability::Minimal
        }
    }
    pub fn focus(&self) -> FocusCapability {
        if matches!(
            self.env_term_program.to_ascii_lowercase().as_str(),
            "wezterm" | "kitty" | "iterm.app"
        ) {
            FocusCapability::Reported
        } else {
            FocusCapability::Unknown
        }
    }
    pub fn notification(&self) -> NotificationCapability {
        if self.env_wt || self.env_term_program.eq_ignore_ascii_case("wezterm") {
            NotificationCapability::Osc99
        } else if self.env_tmux || self.env_zellij {
            NotificationCapability::Osc9
        } else {
            NotificationCapability::Bell
        }
    }
    pub fn clipboard(&self) -> ClipboardCapability {
        if self.env_term.contains("xterm")
            || self.env_term.contains("screen")
            || self.env_term.contains("tmux")
        {
            ClipboardCapability::Osc52
        } else {
            ClipboardCapability::None
        }
    }
    pub fn title(&self) -> TitleCapability {
        if self.env_term.is_empty() || self.env_term == "dumb" {
            TitleCapability::Unsupported
        } else {
            TitleCapability::Supported
        }
    }
    pub fn multiplexer(&self) -> MultiplexerCapability {
        if self.env_tmux {
            MultiplexerCapability::Tmux
        } else if self.env_zellij {
            MultiplexerCapability::Zellij
        } else if self.env_ssh {
            MultiplexerCapability::Ssh
        } else if self.env_wt {
            MultiplexerCapability::WindowsTerminal
        } else {
            MultiplexerCapability::None
        }
    }
    pub fn platform(&self) -> PlatformCapability {
        match std::env::consts::OS {
            "linux" => PlatformCapability::Linux,
            "macos" => PlatformCapability::MacOS,
            "windows" => PlatformCapability::Windows,
            _ => PlatformCapability::Other,
        }
    }
    pub fn width(&self) -> WidthCapability {
        if self.env_vte_version.is_some_and(|v| v >= 7800)
            || matches!(
                self.env_term_program.to_ascii_lowercase().as_str(),
                "wezterm" | "kitty" | "foot"
            )
        {
            WidthCapability::Unicode11
        } else if self.env_term_program == "Apple_Terminal" {
            WidthCapability::Unicode9
        } else {
            WidthCapability::Compact
        }
    }
    pub fn glyph_mode(&self) -> crate::theme::GlyphMode {
        if self.width() == WidthCapability::Compact
            && self.keyboard() == KeyboardCapability::Minimal
        {
            crate::theme::GlyphMode::Ascii
        } else {
            crate::theme::GlyphMode::Preferred
        }
    }
    pub const fn motion(&self) -> MotionCapability {
        MotionCapability::Full
    }
    pub const fn reduced_motion(&self, override_value: bool) -> MotionCapability {
        if override_value {
            MotionCapability::Reduced
        } else {
            MotionCapability::Full
        }
    }
}
