//! Session navigation overlay leaf view.
//!
//! Deterministic view state for the session navigation overlay (OVL-SESSION).
//! No app-state or registry dependency — a plain `Copy` value type.

/// Deterministic view state for the session navigation overlay (OVL-SESSION).
///
/// Tracks visibility, selection, session count, and lineage presence. The
/// view is resize-invariant: terminal resize does not change any field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SessionLeafView {
    /// Whether the session navigation overlay is currently visible.
    pub visible: bool,
    /// Currently highlighted session index.
    pub selected_index: u16,
    /// Total number of sessions in the list.
    pub session_count: u16,
    /// Whether session lineage (parent/child tree) exists.
    pub has_lineage: bool,
}

impl SessionLeafView {
    /// Create a new session leaf view.
    pub const fn new(
        visible: bool,
        selected_index: u16,
        session_count: u16,
        has_lineage: bool,
    ) -> Self {
        Self {
            visible,
            selected_index,
            session_count,
            has_lineage,
        }
    }

    /// Returns `true` if the session list has no entries.
    pub fn is_empty(&self) -> bool {
        self.session_count == 0
    }

    /// Returns `true` if `selected_index` is within the valid range.
    pub fn is_selection_valid(&self) -> bool {
        self.session_count > 0 && self.selected_index < self.session_count
    }

    /// Returns the selected index clamped to the last valid entry.
    pub fn clamped_selection(&self) -> u16 {
        if self.session_count == 0 {
            0
        } else if self.selected_index >= self.session_count {
            self.session_count - 1
        } else {
            self.selected_index
        }
    }

    /// Returns the view state after a terminal resize.
    ///
    /// The session overlay is resize-invariant.
    pub fn after_resize(&self, _width: u16, _height: u16) -> Self {
        *self
    }
}
