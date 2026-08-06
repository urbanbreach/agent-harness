use crate::terminal::capability::ColorMode;
use crate::theme::ColorLevel;

use super::family::ThemeFamily;

pub use crate::app::theme_preview::SystemAppearance;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeChoice {
    Explicit(ThemeFamily),
    Auto,
}

impl ThemeChoice {
    pub const fn explicit(family: ThemeFamily) -> Self {
        Self::Explicit(family)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Explicit(family) => family.label(),
            Self::Auto => "auto",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        if label.trim().eq_ignore_ascii_case("auto") || label.trim().eq_ignore_ascii_case("system")
        {
            Some(Self::Auto)
        } else {
            ThemeFamily::from_label(label).map(Self::Explicit)
        }
    }
}

impl Default for ThemeChoice {
    fn default() -> Self {
        Self::Explicit(ThemeFamily::HarnessChat)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeEnvironment {
    pub no_color: Option<String>,
    pub colorterm: Option<String>,
    pub term: Option<String>,
    pub colorfgbg: Option<String>,
    pub appearance: Option<SystemAppearance>,
}

impl ThemeEnvironment {
    pub fn from_env() -> Self {
        Self {
            no_color: std::env::var("NO_COLOR").ok(),
            colorterm: std::env::var("COLORTERM").ok(),
            term: std::env::var("TERM").ok(),
            colorfgbg: std::env::var("COLORFGBG").ok(),
            appearance: None,
        }
    }

    pub fn from_colorfgbg(value: &str) -> Self {
        Self {
            colorfgbg: Some(value.to_owned()),
            ..Self::default()
        }
    }

    pub fn with_color_level(level: ColorLevel) -> Self {
        match level {
            ColorLevel::None => Self {
                no_color: Some(String::from("1")),
                ..Self::default()
            },
            ColorLevel::Basic => Self {
                term: Some(String::from("xterm")),
                ..Self::default()
            },
            ColorLevel::Ansi256 => Self {
                term: Some(String::from("xterm-256color")),
                ..Self::default()
            },
            ColorLevel::TrueColor => Self {
                colorterm: Some(String::from("truecolor")),
                term: Some(String::from("xterm-256color")),
                ..Self::default()
            },
        }
    }

    pub fn color_level(&self) -> ColorLevel {
        if self.no_color.is_some() {
            return ColorLevel::None;
        }
        match ColorMode::from_env(self.colorterm.as_deref(), self.term.as_deref()) {
            ColorMode::None => ColorLevel::None,
            ColorMode::Ansi16 => ColorLevel::Basic,
            ColorMode::Ansi256 => ColorLevel::Ansi256,
            ColorMode::Truecolor => ColorLevel::TrueColor,
        }
    }

    pub fn system_appearance(&self) -> Option<SystemAppearance> {
        self.appearance
            .or_else(|| detect_system_appearance(self.colorfgbg.as_deref()))
    }

    pub fn with_system_appearance(mut self, appearance: SystemAppearance) -> Self {
        self.appearance = Some(appearance);
        self
    }
}

pub fn detect_system_appearance(colorfgbg: Option<&str>) -> Option<SystemAppearance> {
    let background = colorfgbg?.rsplit(';').next()?.trim();
    match background.parse::<u8>().ok()? {
        0..=6 => Some(SystemAppearance::Dark),
        7..=15 => Some(SystemAppearance::Light),
        _ => None,
    }
}
