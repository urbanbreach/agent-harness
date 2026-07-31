//! Model switcher overlay leaf view.
//!
//! Deterministic view state for the model switcher overlay.
//! No app-state or registry dependency — a plain `Copy` value type.

/// Deterministic view state for the model switcher overlay.
///
/// Tracks visibility, selection, provider count, and model count. The
/// view is resize-invariant: terminal resize does not change any field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModelLeafView {
    /// Whether the model switcher overlay is currently visible.
    pub visible: bool,
    /// Currently highlighted model index.
    pub selected_index: u16,
    /// Number of available providers.
    pub provider_count: u16,
    /// Number of available models across all providers.
    pub model_count: u16,
}

impl ModelLeafView {
    /// Create a new model leaf view.
    pub const fn new(
        visible: bool,
        selected_index: u16,
        provider_count: u16,
        model_count: u16,
    ) -> Self {
        Self {
            visible,
            selected_index,
            provider_count,
            model_count,
        }
    }

    /// Returns `true` if there are no available models.
    pub fn is_empty(&self) -> bool {
        self.model_count == 0
    }

    /// Returns `true` if at least one provider is available.
    pub fn has_provider(&self) -> bool {
        self.provider_count > 0
    }

    /// Returns `true` if `selected_index` is within the valid range.
    pub fn is_selection_valid(&self) -> bool {
        self.model_count > 0 && self.selected_index < self.model_count
    }

    /// Returns the selected index clamped to the last valid entry.
    pub fn clamped_selection(&self) -> u16 {
        if self.model_count == 0 {
            0
        } else if self.selected_index >= self.model_count {
            self.model_count - 1
        } else {
            self.selected_index
        }
    }

    /// Returns the view state after a terminal resize.
    ///
    /// The model switcher overlay is resize-invariant.
    pub fn after_resize(&self, _width: u16, _height: u16) -> Self {
        *self
    }
}
