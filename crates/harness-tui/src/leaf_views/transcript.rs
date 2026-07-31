//! Transcript area leaf view.

/// Deterministic view state for the transcript area.
///
/// No app-state or registry dependency — a plain `Copy` value type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TranscriptLeafView {
    pub scroll_offset: u16,
    pub visible_lines: u16,
}

impl TranscriptLeafView {
    pub const fn new(scroll_offset: u16, visible_lines: u16) -> Self {
        Self {
            scroll_offset,
            visible_lines,
        }
    }
}
