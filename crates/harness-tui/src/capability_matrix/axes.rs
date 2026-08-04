use serde::{Deserialize, Serialize};

macro_rules! axis {
    ($name:ident { $($variant:ident => $label:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }
        impl $name {
            pub const fn label(self) -> &'static str {
                match self { $(Self::$variant => $label),+ }
            }
        }
    };
}

axis!(ColorCapability { TrueColor => "true_color", Ansi256 => "ansi256", Basic16 => "basic16", NoColor => "no_color" });
impl ColorCapability {
    pub const fn to_color_level(self) -> crate::theme::ColorLevel {
        match self {
            Self::TrueColor => crate::theme::ColorLevel::TrueColor,
            Self::Ansi256 => crate::theme::ColorLevel::Ansi256,
            Self::Basic16 => crate::theme::ColorLevel::Basic,
            Self::NoColor => crate::theme::ColorLevel::None,
        }
    }
}
axis!(GraphicsCapability { Kitty => "kitty", ITerm2 => "i_term2", Sixel => "sixel", None => "none" });
impl GraphicsCapability {
    pub const fn supports_inline(self) -> bool {
        !matches!(self, Self::None)
    }
}
axis!(KeyboardCapability { Modern => "modern", Legacy => "legacy", Minimal => "minimal" });
axis!(FocusCapability { Reported => "reported", Unknown => "unknown" });
axis!(NotificationCapability { Osc9 => "osc9", Osc99 => "osc99", Osc777 => "osc777", Bell => "bell", None => "none" });
axis!(ClipboardCapability { Osc52 => "osc52", None => "none" });
axis!(TitleCapability { Supported => "supported", Unsupported => "unsupported" });
axis!(MultiplexerCapability { Tmux => "tmux", Zellij => "zellij", Ssh => "ssh", WindowsTerminal => "windows_terminal", None => "none", Unknown => "unknown" });
axis!(PlatformCapability { Linux => "linux", MacOS => "mac_os", Windows => "windows", Other => "other" });
axis!(WidthCapability { Unicode11 => "unicode11", Unicode9 => "unicode9", Compact => "compact" });
impl WidthCapability {
    pub const fn handles_cjk(self) -> bool {
        !matches!(self, Self::Compact)
    }
    pub const fn handles_emoji(self) -> bool {
        matches!(self, Self::Unicode11)
    }
}
axis!(MotionCapability { Full => "full", Reduced => "reduced" });
axis!(ViewportCapability { Compact40x10 => "compact40x10", Dense60x15 => "dense60x15", Default80x24 => "default80x24", Standard100x30 => "standard100x30", Wide132x40 => "wide132x40", Large160x50 => "large160x50", Maximum200x60 => "maximum200x60" });
impl ViewportCapability {
    pub const fn all() -> [Self; 7] {
        [
            Self::Compact40x10,
            Self::Dense60x15,
            Self::Default80x24,
            Self::Standard100x30,
            Self::Wide132x40,
            Self::Large160x50,
            Self::Maximum200x60,
        ]
    }
    pub const fn dimensions(self) -> (u16, u16) {
        match self {
            Self::Compact40x10 => (40, 10),
            Self::Dense60x15 => (60, 15),
            Self::Default80x24 => (80, 24),
            Self::Standard100x30 => (100, 30),
            Self::Wide132x40 => (132, 40),
            Self::Large160x50 => (160, 50),
            Self::Maximum200x60 => (200, 60),
        }
    }
}
