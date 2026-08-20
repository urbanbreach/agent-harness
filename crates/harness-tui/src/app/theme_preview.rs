//! Theme preview/apply/revert state and auto dark/light system appearance switch.
//!
//! Preview values are NOT persisted before apply. No network calls.

/// System appearance preference reported by the terminal emulator or OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemAppearance {
    Dark,
    Light,
}

/// Theme preview state — tracks the current theme, an optional preview
/// (not persisted), and auto mode with system appearance tracking.
#[derive(Debug, Clone)]
pub struct ThemePreviewState {
    current_name: String,
    preview_name: Option<String>,
    auto_mode: bool,
    system_appearance: Option<SystemAppearance>,
}

impl ThemePreviewState {
    /// Create a new preview state with the given initial theme.
    pub fn new(initial_theme: &str) -> Self {
        Self {
            current_name: initial_theme.to_string(),
            preview_name: None,
            auto_mode: false,
            system_appearance: None,
        }
    }

    /// Returns the current (committed) theme name.
    pub fn current_name(&self) -> &str {
        &self.current_name
    }

    /// Returns the preview theme name, if a preview is active.
    pub fn preview_name(&self) -> Option<&str> {
        self.preview_name.as_deref()
    }

    /// Returns true if a preview is active (not yet applied or reverted).
    pub fn is_previewing(&self) -> bool {
        self.preview_name.is_some()
    }

    /// Start a preview of the given theme. Does NOT persist.
    pub fn preview(&mut self, theme_name: &str) {
        self.preview_name = Some(theme_name.to_string());
    }

    /// Revert the preview, discarding any uncommitted change.
    pub fn revert(&mut self) {
        self.preview_name = None;
    }

    /// Apply the preview, persisting it as the current theme.
    pub fn apply(&mut self) {
        if let Some(name) = self.preview_name.take() {
            self.current_name = name;
        }
    }

    /// Returns true if auto dark/light mode is enabled.
    pub fn is_auto_mode(&self) -> bool {
        self.auto_mode
    }

    /// Enable or disable auto dark/light mode.
    pub fn set_auto_mode(&mut self, enabled: bool) {
        self.auto_mode = enabled;
    }

    /// Returns the last reported system appearance, if any.
    pub fn system_appearance(&self) -> Option<SystemAppearance> {
        self.system_appearance
    }

    /// Handle a system appearance change. When auto mode is enabled,
    /// the current theme switches to match the system appearance.
    pub fn on_system_appearance_change(&mut self, appearance: SystemAppearance) {
        self.system_appearance = Some(appearance);
        if self.auto_mode {
            self.current_name = match appearance {
                SystemAppearance::Dark => "harness-chat".to_string(),
                SystemAppearance::Light => "harness-light".to_string(),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SystemAppearance, ThemePreviewState};

    #[test]
    fn automatic_dark_appearance_selects_harness_chat() {
        // arrange
        let mut state = ThemePreviewState::new("harness-light");
        state.set_auto_mode(true);

        // act
        state.on_system_appearance_change(SystemAppearance::Dark);

        // assert
        assert_eq!(state.current_name(), "harness-chat");
    }
}
