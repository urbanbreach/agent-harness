use std::fmt::{Display, Formatter};

use super::contract::{FidelityConfig, FidelityDefaults, InputMode, MotionMode, NotificationMode};

pub struct ConfigMigration;
pub type MigrationResult = Result<FidelityConfig, MigrationError>;

impl ConfigMigration {
    pub fn migrate(raw: &str) -> MigrationResult {
        let value: serde_json::Value = serde_json::from_str(raw)
            .map_err(|error| MigrationError::ParseError(error.to_string()))?;
        let version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| MigrationError::InvalidValue("missing schema_version".to_string()))?;
        match version {
            "fidelity-v0" => Self::migrate_v0_to_v1(&value),
            "fidelity-v1" => serde_json::from_value(value)
                .map_err(|error| MigrationError::InvalidValue(error.to_string())),
            other => Err(MigrationError::UnknownSchema(other.to_string())),
        }
    }

    pub fn migrate_v0_to_v1(v0: &serde_json::Value) -> MigrationResult {
        let theme = v0
            .get("theme")
            .and_then(serde_json::Value::as_str)
            .and_then(crate::theme_family::ThemeChoice::from_label)
            .ok_or_else(|| MigrationError::InvalidValue("theme".to_string()))?;
        let reduced_motion = v0
            .get("reduced_motion")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| MigrationError::InvalidValue("reduced_motion".to_string()))?;
        let graphics = v0
            .get("graphics")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| MigrationError::InvalidValue("graphics".to_string()))?;
        let defaults = FidelityDefaults::current();
        Ok(FidelityConfig {
            schema_version: FidelityDefaults::schema_version().to_string(),
            theme,
            input_mode: InputMode::Auto,
            motion: if reduced_motion {
                MotionMode::Reduced
            } else {
                MotionMode::Full
            },
            notification: NotificationMode::Native,
            inline_images: graphics,
            inline_video: defaults.inline_video,
            dashboard_enabled: defaults.dashboard_enabled,
            tips_enabled: defaults.tips_enabled,
            terminal_title: defaults.terminal_title,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationError {
    UnknownSchema(String),
    ParseError(String),
    InvalidValue(String),
}

impl Display for MigrationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownSchema(value) => write!(formatter, "unknown fidelity schema: {value}"),
            Self::ParseError(value) => write!(formatter, "fidelity parse failed: {value}"),
            Self::InvalidValue(value) => write!(formatter, "invalid fidelity value: {value}"),
        }
    }
}
impl std::error::Error for MigrationError {}
