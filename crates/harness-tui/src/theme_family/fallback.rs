//! Truecolor to terminal-capability fallback ladder reusing theme primitives.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedColor {
    pub level: crate::theme::ColorLevel,
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl ResolvedColor {
    pub fn rgb(&self) -> (u8, u8, u8) {
        (self.red, self.green, self.blue)
    }
}

pub struct FallbackLadder;

impl FallbackLadder {
    pub fn resolve(rgb: (u8, u8, u8), level: crate::theme::ColorLevel) -> ResolvedColor {
        let (red, green, blue) = match level {
            crate::theme::ColorLevel::TrueColor => rgb,
            crate::theme::ColorLevel::Ansi256 => {
                let index = crate::theme::nearest_indexed(rgb.0, rgb.1, rgb.2);
                crate::theme::indexed_to_rgb(index)
            }
            crate::theme::ColorLevel::Basic => {
                let color = crate::theme::quantize_color(
                    ratatui::style::Color::Rgb(rgb.0, rgb.1, rgb.2),
                    level,
                );
                match crate::theme::resolve_to_rgb(color) {
                    Some(resolved) => resolved,
                    None => rgb,
                }
            }
            crate::theme::ColorLevel::None => (0, 0, 0),
        };

        ResolvedColor {
            level,
            red,
            green,
            blue,
        }
    }

    pub fn resolve_all(rgb: (u8, u8, u8)) -> Vec<ResolvedColor> {
        [
            crate::theme::ColorLevel::None,
            crate::theme::ColorLevel::Basic,
            crate::theme::ColorLevel::Ansi256,
            crate::theme::ColorLevel::TrueColor,
        ]
        .into_iter()
        .map(|level| Self::resolve(rgb, level))
        .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FallbackError {
    QuantizeFailure(String),
}

impl std::fmt::Display for FallbackError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::QuantizeFailure(message) => {
                write!(formatter, "color quantization failed: {message}")
            }
        }
    }
}

impl std::error::Error for FallbackError {}
