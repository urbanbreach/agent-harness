//! Composer area leaf view.

/// Deterministic view state for the composer area.
///
/// No app-state or registry dependency — a plain `Copy` value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ComposerLeafView {
    pub prompt_visible: bool,
    pub cursor_visible: bool,
}

impl ComposerLeafView {
    pub const fn new(prompt_visible: bool, cursor_visible: bool) -> Self {
        Self {
            prompt_visible,
            cursor_visible,
        }
    }
}
