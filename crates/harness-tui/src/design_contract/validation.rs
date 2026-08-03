use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesignContractValidationError {
    AdHocColor { marker: &'static str },
    AdHocGeometry { marker: &'static str },
}

impl fmt::Display for DesignContractValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AdHocColor { marker } => write!(formatter, "ad-hoc color marker: {marker}"),
            Self::AdHocGeometry { marker } => write!(formatter, "ad-hoc geometry marker: {marker}"),
        }
    }
}

impl std::error::Error for DesignContractValidationError {}

pub fn validate_no_adhoc_colors_or_geometry(
    source: &str,
) -> Result<(), DesignContractValidationError> {
    for marker in ["Rgb(", "Color::Rgb(", "ratatui::style::Color::Rgb("] {
        if source.contains(marker) {
            return Err(DesignContractValidationError::AdHocColor { marker });
        }
    }
    for marker in ["Rect::new(", "Constraint::Length(", "Layout::default()"] {
        if source.contains(marker) {
            return Err(DesignContractValidationError::AdHocGeometry { marker });
        }
    }
    Ok(())
}
