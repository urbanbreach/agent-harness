//! Palette overlay leaf view.
//!
//! Deterministic view state for the command palette overlay (OVL-PALETTE).
//! No app-state or registry dependency — a plain `Copy` value type.

/// Deterministic view state for the command palette overlay (OVL-PALETTE).
///
/// Tracks visibility, selection, entry count, and query length. The view is
/// resize-invariant: terminal resize does not change any field because the
/// palette overlay reflows its content to fit the new dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PaletteLeafView {
    /// Whether the palette overlay is currently visible.
    pub visible: bool,
    /// Currently highlighted entry index.
    pub selected_index: u16,
    /// Total number of entries matching the current query.
    pub entry_count: u16,
    /// Length of the filter query string (deterministic proxy).
    pub query_len: u16,
}

impl PaletteLeafView {
    /// Create a new palette leaf view.
    pub const fn new(visible: bool, selected_index: u16, entry_count: u16, query_len: u16) -> Self {
        Self {
            visible,
            selected_index,
            entry_count,
            query_len,
        }
    }

    /// Returns `true` if the palette has no matching entries.
    pub fn is_empty(&self) -> bool {
        self.entry_count == 0
    }

    /// Returns `true` if `selected_index` is within the valid range `[0, entry_count)`.
    pub fn is_selection_valid(&self) -> bool {
        self.entry_count > 0 && self.selected_index < self.entry_count
    }

    /// Returns the selected index clamped to the last valid entry.
    ///
    /// If the palette is empty, returns `0`.
    pub fn clamped_selection(&self) -> u16 {
        if self.entry_count == 0 {
            0
        } else if self.selected_index >= self.entry_count {
            self.entry_count - 1
        } else {
            self.selected_index
        }
    }

    /// Returns the view state after a terminal resize.
    ///
    /// The palette overlay is resize-invariant: all fields are preserved
    /// because the overlay reflows its content to fit the new dimensions.
    /// The width and height parameters are accepted for API symmetry with
    /// future layout-aware views but do not modify the returned state.
    pub fn after_resize(&self, _width: u16, _height: u16) -> Self {
        *self
    }
}
