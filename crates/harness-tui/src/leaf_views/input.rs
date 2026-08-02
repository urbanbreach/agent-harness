//! Input / composer-input leaf view.
//!
//! Models the composer input surface: draft buffer, cursor position,
//! focus owner, and Unicode handling. Covers P0-START-03 (typing clears
//! welcome), P0-COMP-01 (bordered strip), and the failure scenario
//! `empty-small-unicode-enhanced-key`.

/// The area that owns keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusOwner {
    #[default]
    None,
    Composer,
    Transcript,
    Palette,
}

impl FocusOwner {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Composer => "composer",
            Self::Transcript => "transcript",
            Self::Palette => "palette",
        }
    }
}

/// Deterministic view state for the composer input surface.
///
/// No app-state or registry dependency — a plain `Copy` value type
/// (the `draft` is an owned `&str` borrow with the lifetime of the
/// source data, so the view stays cheap to copy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputLeafView<'a> {
    pub focus_owner: FocusOwner,
    pub draft: &'a str,
    pub cursor: usize,
    pub welcome_visible: bool,
}

impl<'a> InputLeafView<'a> {
    pub const fn new(
        focus_owner: FocusOwner,
        draft: &'a str,
        cursor: usize,
        welcome: bool,
    ) -> Self {
        Self {
            focus_owner,
            draft,
            cursor,
            welcome_visible: welcome,
        }
    }

    /// Derive the input leaf view from real app state fields.
    pub fn from_state(
        focus_is_prompt: bool,
        draft: &'a str,
        cursor: usize,
        startup_shell_visible: bool,
    ) -> Self {
        let focus_owner = if focus_is_prompt || startup_shell_visible {
            FocusOwner::Composer
        } else {
            FocusOwner::None
        };
        let welcome = startup_shell_visible && draft.is_empty();
        Self::new(focus_owner, draft, cursor, welcome)
    }

    /// P0-START-03: typing clears the welcome panel.
    pub const fn draft_clears_welcome(self) -> bool {
        !self.draft.is_empty() && !self.welcome_visible
    }

    /// The composer is always the input target at startup.
    pub const fn focus_is_composer(self) -> bool {
        matches!(self.focus_owner, FocusOwner::Composer)
    }

    /// Unicode width probe: returns the display width of the draft
    /// using `unicode_width::UnicodeWidthStr`. Used by the failure
    /// scenario `empty-small-unicode-enhanced-key` to prove the
    /// composer handles CJK / enhanced keys without panic.
    pub fn draft_display_width(self) -> usize {
        unicode_width::UnicodeWidthStr::width(self.draft)
    }

    /// Cursor is within bounds of the draft (char count).
    pub fn cursor_in_bounds(self) -> bool {
        self.cursor <= self.draft.chars().count()
    }
}

impl Default for InputLeafView<'static> {
    fn default() -> Self {
        Self::new(FocusOwner::None, "", 0, false)
    }
}
