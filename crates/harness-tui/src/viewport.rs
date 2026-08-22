use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewportId {
    Compact40x10,
    Dense60x15,
    Default80x24,
    Standard100x30,
    Wide132x40,
    Large160x50,
    Maximum200x60,
}

impl ViewportId {
    pub const ALL: [Self; 7] = [
        Self::Compact40x10,
        Self::Dense60x15,
        Self::Default80x24,
        Self::Standard100x30,
        Self::Wide132x40,
        Self::Large160x50,
        Self::Maximum200x60,
    ];

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

    pub fn closest(width: u16, height: u16) -> Self {
        Self::ALL
            .into_iter()
            .min_by_key(|viewport| {
                let (candidate_width, candidate_height) = viewport.dimensions();
                candidate_width.abs_diff(width) + candidate_height.abs_diff(height)
            })
            .unwrap_or(Self::Standard100x30)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Compact40x10 => "40x10",
            Self::Dense60x15 => "60x15",
            Self::Default80x24 => "80x24",
            Self::Standard100x30 => "100x30",
            Self::Wide132x40 => "132x40",
            Self::Large160x50 => "160x50",
            Self::Maximum200x60 => "200x60",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakpointBand {
    UltraCompact,
    Compact,
    Standard,
    Primary,
    Wide,
    Large,
    Maximum,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewportBreakpoint {
    pub id: ViewportId,
    pub width: u16,
    pub height: u16,
    pub band: BreakpointBand,
    pub composer_inset: u16,
    pub breadcrumb_top_margin: u16,
    pub composer_footer_spacer: u16,
}

impl ViewportBreakpoint {
    pub const fn dimensions(self) -> (u16, u16) {
        (self.width, self.height)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponsiveBreakpoints {
    pub all: [ViewportBreakpoint; 7],
}
