//! Terminal screen mode preferences: inline/fullscreen, compact/minimal/native
//! scrollback, and fold/raw/manual view preferences.
//!
//! Self-contained module — no `super::` or `crate::` imports. Included via
//! `#[path]` in integration tests and usable standalone.

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Terminal rendering mode: inline (embedded in shell) or fullscreen
/// (alternate screen buffer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TerminalMode {
    /// Inline rendering within the normal terminal scrollback.
    #[default]
    Inline,
    /// Fullscreen alternate-screen rendering.
    Fullscreen,
}

/// Scrollback rendering style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScrollbackStyle {
    /// Compact: condensed rows with reduced spacing.
    #[default]
    Compact,
    /// Minimal: stripped-down native scrollback with minimal chrome.
    Minimal,
    /// Native: full terminal-native scrollback behaviour.
    Native,
}

/// Transcript view preference for tool/thinking blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewPreference {
    /// Fold collapsible blocks by default.
    #[default]
    Fold,
    /// Show raw content without folding.
    Raw,
    /// Manual: user controls fold state per block.
    Manual,
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors from screen-mode transitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenModeError {
    /// The requested terminal mode is not supported by the current terminal.
    FullscreenNotSupported,
    /// An unsupported terminal mode was requested.
    UnsupportedTerminalMode(TerminalMode),
}

impl std::fmt::Display for TerminalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Inline => write!(f, "inline"),
            Self::Fullscreen => write!(f, "fullscreen"),
        }
    }
}

impl std::fmt::Display for ScreenModeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FullscreenNotSupported => {
                write!(f, "fullscreen terminal mode is not supported")
            }
            Self::UnsupportedTerminalMode(mode) => {
                write!(f, "unsupported terminal mode: {mode}")
            }
        }
    }
}

impl std::error::Error for ScreenModeError {}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Screen mode state: terminal mode, scrollback style, and view preference.
///
/// Created with `ScreenModeState::new(fullscreen_supported)` where
/// `fullscreen_supported` indicates whether the terminal can enter fullscreen
/// (alternate screen) mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenModeState {
    terminal_mode: TerminalMode,
    scrollback_style: ScrollbackStyle,
    view_preference: ViewPreference,
    fullscreen_supported: bool,
}

impl Default for ScreenModeState {
    fn default() -> Self {
        Self {
            terminal_mode: TerminalMode::Inline,
            scrollback_style: ScrollbackStyle::Compact,
            view_preference: ViewPreference::Fold,
            fullscreen_supported: true,
        }
    }
}

impl ScreenModeState {
    /// Create a new screen mode state with the given fullscreen support flag.
    pub fn new(fullscreen_supported: bool) -> Self {
        Self {
            fullscreen_supported,
            ..Self::default()
        }
    }

    // -- accessors --

    /// Current terminal mode.
    pub fn terminal_mode(&self) -> TerminalMode {
        self.terminal_mode
    }

    /// Current scrollback style.
    pub fn scrollback_style(&self) -> ScrollbackStyle {
        self.scrollback_style
    }

    /// Current view preference.
    pub fn view_preference(&self) -> ViewPreference {
        self.view_preference
    }

    /// Whether fullscreen mode is supported.
    pub fn fullscreen_supported(&self) -> bool {
        self.fullscreen_supported
    }

    // -- terminal mode transitions --

    /// Toggle between inline and fullscreen terminal mode.
    ///
    /// Returns `Err(FullscreenNotSupported)` if the terminal does not support
    /// fullscreen mode.
    pub fn toggle_fullscreen(&mut self) -> Result<(), ScreenModeError> {
        if !self.fullscreen_supported {
            return Err(ScreenModeError::FullscreenNotSupported);
        }
        self.terminal_mode = match self.terminal_mode {
            TerminalMode::Inline => TerminalMode::Fullscreen,
            TerminalMode::Fullscreen => TerminalMode::Inline,
        };
        Ok(())
    }

    /// Explicitly set the terminal mode.
    ///
    /// Returns `Err(UnsupportedTerminalMode)` if fullscreen is requested but
    /// not supported.
    pub fn set_terminal_mode(&mut self, mode: TerminalMode) -> Result<(), ScreenModeError> {
        if mode == TerminalMode::Fullscreen && !self.fullscreen_supported {
            return Err(ScreenModeError::UnsupportedTerminalMode(mode));
        }
        self.terminal_mode = mode;
        Ok(())
    }

    // -- scrollback style transitions --

    /// Explicitly set the scrollback style.
    pub fn set_scrollback_style(&mut self, style: ScrollbackStyle) {
        self.scrollback_style = style;
    }

    /// Cycle scrollback style: Compact -> Minimal -> Native -> Compact.
    pub fn cycle_scrollback_style(&mut self) {
        self.scrollback_style = match self.scrollback_style {
            ScrollbackStyle::Compact => ScrollbackStyle::Minimal,
            ScrollbackStyle::Minimal => ScrollbackStyle::Native,
            ScrollbackStyle::Native => ScrollbackStyle::Compact,
        };
    }

    // -- view preference transitions --

    /// Explicitly set the view preference.
    pub fn set_view_preference(&mut self, pref: ViewPreference) {
        self.view_preference = pref;
    }

    /// Cycle view preference: Fold -> Raw -> Manual -> Fold.
    pub fn cycle_view_preference(&mut self) {
        self.view_preference = match self.view_preference {
            ViewPreference::Fold => ViewPreference::Raw,
            ViewPreference::Raw => ViewPreference::Manual,
            ViewPreference::Manual => ViewPreference::Fold,
        };
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_inline_compact_fold() {
        let state = ScreenModeState::default();
        assert_eq!(state.terminal_mode(), TerminalMode::Inline);
        assert_eq!(state.scrollback_style(), ScrollbackStyle::Compact);
        assert_eq!(state.view_preference(), ViewPreference::Fold);
    }

    #[test]
    fn unsupported_fullscreen_returns_error() {
        let mut state = ScreenModeState::new(false);
        let result = state.toggle_fullscreen();
        assert!(matches!(
            result,
            Err(ScreenModeError::FullscreenNotSupported)
        ));
    }
}
