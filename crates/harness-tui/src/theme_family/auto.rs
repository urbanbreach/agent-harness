//! System color-scheme preference detection for auto theme mode.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoMode {
    Auto,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemPreference {
    Dark,
    Light,
}

impl SystemPreference {
    pub fn to_family(self) -> super::family::ThemeFamily {
        match self {
            Self::Dark => super::family::ThemeFamily::Dark,
            Self::Light => super::family::ThemeFamily::Light,
        }
    }
}

/// Detect the system color-scheme preference from the environment.
///
/// Resolution order:
/// 1. Parse the final `COLORFGBG` field as the background palette index.
/// 2. Fall back to `Dark` (the design contract default).
pub fn detect_system_preference() -> SystemPreference {
    system_preference_from_colorfgbg(std::env::var("COLORFGBG").ok().as_deref())
}

fn system_preference_from_colorfgbg(colorfgbg: Option<&str>) -> SystemPreference {
    let background = colorfgbg
        .and_then(|value| value.rsplit(';').next())
        .and_then(|value| value.trim().parse::<u8>().ok());
    match background {
        Some(7..=15) => SystemPreference::Light,
        _ => SystemPreference::Dark,
    }
}

pub struct AutoResolver {
    last_detected: Option<SystemPreference>,
}

impl AutoResolver {
    pub fn new() -> Self {
        Self {
            last_detected: None,
        }
    }

    /// Probe the environment and update the cached preference.
    /// Returns the freshly-detected preference.
    pub fn refresh(&mut self) -> SystemPreference {
        let detected = detect_system_preference();
        self.last_detected = Some(detected);
        detected
    }

    /// Returns the last-detected preference without probing again.
    pub fn current(&self) -> Option<SystemPreference> {
        self.last_detected
    }

    /// Returns the resolved family, probing if no cached preference exists.
    pub fn resolve(&mut self) -> super::family::ThemeFamily {
        let pref = self.last_detected.unwrap_or_else(detect_system_preference);
        self.last_detected = Some(pref);
        pref.to_family()
    }
}

impl Default for AutoResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoDetectError {
    ProbeFailure(String),
}

impl std::fmt::Display for AutoDetectError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProbeFailure(message) => write!(formatter, "auto theme probe failed: {message}"),
        }
    }
}

impl std::error::Error for AutoDetectError {}

#[cfg(test)]
mod tests {
    use super::{system_preference_from_colorfgbg, SystemPreference};

    #[test]
    fn colorfgbg_uses_the_final_background_field() {
        assert_eq!(
            system_preference_from_colorfgbg(Some("15;0")),
            SystemPreference::Dark
        );
        assert_eq!(
            system_preference_from_colorfgbg(Some("0;15")),
            SystemPreference::Light
        );
    }
}
