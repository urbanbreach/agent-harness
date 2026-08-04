//! Persisted theme choice round-trip through the TUI config contract.

/// A user's persisted preference for the theme family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    /// Always use the dark family.
    Dark,
    /// Always use the light family.
    Light,
    /// Follow the detected system preference.
    Auto,
}

impl ThemeChoice {
    pub fn all() -> [Self; 3] {
        [Self::Dark, Self::Light, Self::Auto]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::Auto => "auto",
        }
    }

    pub fn from_label(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("dark") {
            Some(Self::Dark)
        } else if value.eq_ignore_ascii_case("light") {
            Some(Self::Light)
        } else if value.eq_ignore_ascii_case("auto") {
            Some(Self::Auto)
        } else {
            None
        }
    }

    pub fn to_family(self, system_dark: bool) -> super::family::ThemeFamily {
        match self {
            Self::Dark => super::family::ThemeFamily::Dark,
            Self::Light => super::family::ThemeFamily::Light,
            Self::Auto if system_dark => super::family::ThemeFamily::Dark,
            Self::Auto => super::family::ThemeFamily::Light,
        }
    }
}

impl std::fmt::Display for ThemeChoice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

/// The on-disk representation of a persisted theme choice.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PersistedTheme {
    /// Schema version for forward-compatible migration.
    pub schema: String,
    /// The persisted theme choice.
    pub theme: ThemeChoice,
}

impl PersistedTheme {
    pub fn new(choice: ThemeChoice) -> Self {
        Self {
            schema: "theme-family-v1".to_string(),
            theme: choice,
        }
    }
}

pub fn serialize_choice(choice: ThemeChoice) -> Result<String, PersistError> {
    serde_json::to_string_pretty(&PersistedTheme::new(choice))
        .map_err(|error| PersistError::Serialize(error.to_string()))
}

pub fn deserialize_choice(json: &str) -> Result<ThemeChoice, PersistError> {
    let persisted: PersistedTheme =
        serde_json::from_str(json).map_err(|error| PersistError::Deserialize(error.to_string()))?;
    if persisted.schema != "theme-family-v1" {
        return Err(PersistError::UnknownSchema(persisted.schema));
    }
    Ok(persisted.theme)
}

#[derive(Debug)]
pub enum PersistError {
    Serialize(String),
    Deserialize(String),
    UnknownSchema(String),
}

impl std::fmt::Display for PersistError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Serialize(message) => write!(formatter, "theme serialization failed: {message}"),
            Self::Deserialize(message) => {
                write!(formatter, "theme deserialization failed: {message}")
            }
            Self::UnknownSchema(schema) => write!(formatter, "unknown theme schema: {schema}"),
        }
    }
}

impl std::error::Error for PersistError {}
