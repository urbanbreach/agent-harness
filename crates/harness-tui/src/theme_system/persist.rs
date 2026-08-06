use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use super::auto::ThemeChoice;

pub const TUI_THEME_KEY: &str = "theme";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeConfigError {
    value: String,
}

impl ThemeConfigError {
    fn invalid(value: &str) -> Self {
        Self {
            value: value.to_owned(),
        }
    }
}

impl Display for ThemeConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "unknown TUI theme choice `{}`", self.value)
    }
}

impl std::error::Error for ThemeConfigError {}

pub fn load_theme_choice(
    keybindings: &BTreeMap<String, String>,
) -> Result<ThemeChoice, ThemeConfigError> {
    keybindings
        .get(TUI_THEME_KEY)
        .map_or(Ok(ThemeChoice::default()), |value| {
            ThemeChoice::from_label(value).ok_or_else(|| ThemeConfigError::invalid(value))
        })
}

pub fn store_theme_choice(keybindings: &mut BTreeMap<String, String>, choice: ThemeChoice) {
    keybindings.insert(TUI_THEME_KEY.to_owned(), choice.label().to_owned());
}
