use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputMode {
    Modern,
    Legacy,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionMode {
    Full,
    Reduced,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationMode {
    Native,
    Bell,
    Off,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FidelityConfig {
    pub schema_version: String,
    pub theme: crate::theme_family::ThemeChoice,
    pub input_mode: InputMode,
    pub motion: MotionMode,
    pub notification: NotificationMode,
    pub inline_images: bool,
    pub inline_video: bool,
    pub dashboard_enabled: bool,
    pub tips_enabled: bool,
    pub terminal_title: bool,
}

impl FidelityConfig {
    pub fn from_defaults() -> Self {
        FidelityDefaults::current()
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let config = crate::fidelity_config::ConfigMigration::migrate(json).map_err(|error| {
            serde_json::Error::io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
        })?;
        config.validate().map_err(|error| {
            serde_json::Error::io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
        })?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        if !FidelityDefaults::known_schema_versions().contains(&self.schema_version.as_str()) {
            return Err(ConfigValidationError::UnknownSchema(
                self.schema_version.clone(),
            ));
        }
        if self.schema_version == FidelityDefaults::schema_version() {
            Ok(())
        } else {
            Err(ConfigValidationError::InvalidValue(
                "configuration must be migrated to fidelity-v1".to_string(),
            ))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigValidationError {
    UnknownSchema(String),
    InvalidValue(String),
    MissingField(String),
}

impl Display for ConfigValidationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownSchema(value) => write!(formatter, "unknown fidelity schema: {value}"),
            Self::InvalidValue(value) => write!(formatter, "invalid fidelity value: {value}"),
            Self::MissingField(value) => write!(formatter, "missing fidelity field: {value}"),
        }
    }
}
impl std::error::Error for ConfigValidationError {}

pub struct FidelityDefaults;

impl FidelityDefaults {
    pub fn current() -> FidelityConfig {
        FidelityConfig {
            schema_version: Self::schema_version().to_string(),
            theme: crate::theme_family::ThemeChoice::Auto,
            input_mode: InputMode::Auto,
            motion: MotionMode::Full,
            notification: NotificationMode::Native,
            inline_images: true,
            inline_video: false,
            dashboard_enabled: true,
            tips_enabled: true,
            terminal_title: true,
        }
    }

    pub fn conservative() -> FidelityConfig {
        let mut config = Self::current();
        config.inline_images = false;
        config.terminal_title = false;
        config
    }

    pub const fn schema_version() -> &'static str {
        "fidelity-v1"
    }

    pub const fn known_schema_versions() -> &'static [&'static str] {
        &["fidelity-v0", "fidelity-v1"]
    }
}
