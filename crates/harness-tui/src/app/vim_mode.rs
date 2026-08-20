//! Vim-style modal editing state for the composer: simple/vim toggle and
//! cursor navigation.
//!
//! Self-contained module — no `super::` or `crate::` imports. Included via
//! `#[path]` in integration tests and usable standalone.

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Vim editor mode state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VimEditorMode {
    /// Vim mode disabled (simple editing).
    #[default]
    Disabled,
    /// Normal mode: navigation keys active, typing deferred.
    Normal,
    /// Insert mode: typing active, navigation deferred.
    Insert,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Vim editing state: mode toggle and cursor position.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VimState {
    mode: VimEditorMode,
    cursor_line: usize,
    cursor_col: usize,
}

impl VimState {
    /// Create a new vim state (disabled by default).
    pub fn new() -> Self {
        Self::default()
    }

    // -- accessors --

    /// Current vim editor mode.
    pub fn mode(&self) -> VimEditorMode {
        self.mode
    }

    /// Current cursor line (0-indexed).
    pub fn cursor_line(&self) -> usize {
        self.cursor_line
    }

    /// Current cursor column (0-indexed).
    pub fn cursor_col(&self) -> usize {
        self.cursor_col
    }

    /// Whether vim mode is enabled (Normal or Insert).
    pub fn is_enabled(&self) -> bool {
        self.mode != VimEditorMode::Disabled
    }

    // -- mode transitions --

    /// Toggle vim mode on/off.
    ///
    /// Disabled -> Normal, Normal|Insert -> Disabled.
    pub fn toggle_vim(&mut self) {
        self.mode = match self.mode {
            VimEditorMode::Disabled => VimEditorMode::Normal,
            VimEditorMode::Normal | VimEditorMode::Insert => VimEditorMode::Disabled,
        };
    }

    /// Enter insert mode (no-op when disabled).
    pub fn enter_insert(&mut self) {
        if self.mode != VimEditorMode::Disabled {
            self.mode = VimEditorMode::Insert;
        }
    }

    /// Enter normal mode (no-op when disabled).
    pub fn enter_normal(&mut self) {
        if self.mode != VimEditorMode::Disabled {
            self.mode = VimEditorMode::Normal;
        }
    }

    // -- navigation (Normal mode only) --

    /// Move cursor up by one line (Normal mode only; clamps at 0).
    pub fn move_up(&mut self) {
        if self.mode == VimEditorMode::Normal {
            self.cursor_line = self.cursor_line.saturating_sub(1);
        }
    }

    /// Move cursor down by one line (Normal mode only; clamps at `max_line`).
    pub fn move_down(&mut self, max_line: usize) {
        if self.mode == VimEditorMode::Normal {
            self.cursor_line = (self.cursor_line + 1).min(max_line);
        }
    }

    /// Move cursor left by one column (Normal mode only; clamps at 0).
    pub fn move_left(&mut self) {
        if self.mode == VimEditorMode::Normal {
            self.cursor_col = self.cursor_col.saturating_sub(1);
        }
    }

    /// Move cursor right by one column (Normal mode only; clamps at `max_col`).
    pub fn move_right(&mut self, max_col: usize) {
        if self.mode == VimEditorMode::Normal {
            self.cursor_col = (self.cursor_col + 1).min(max_col);
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vim_mode_defaults_to_disabled_state() {
        // arrange
        // act
        let state = VimState::new();
        // assert
        assert_eq!(state.mode(), VimEditorMode::Disabled);
        assert!(!state.is_enabled());
    }

    #[test]
    fn toggle_enables_normal_mode() {
        // arrange
        // act
        let mut state = VimState::new();
        state.toggle_vim();
        // assert
        assert_eq!(state.mode(), VimEditorMode::Normal);
    }
}
