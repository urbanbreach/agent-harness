//! Diff leaf view.

/// Deterministic view state for a diff block rendered in the transcript.
///
/// No app-state or registry dependency — a plain `Copy` value type.
/// Captures whether a diff is present, its line counts, and whether it
/// was derived from event-sourced tool output (not synthetic injection).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiffLeafView {
    pub present: bool,
    pub added_lines: u16,
    pub removed_lines: u16,
    pub event_derived: bool,
    pub file_path: &'static str,
}

impl DiffLeafView {
    pub const fn new() -> Self {
        Self {
            present: false,
            added_lines: 0,
            removed_lines: 0,
            event_derived: false,
            file_path: "",
        }
    }

    /// Construct a diff view from event-derived tool output.
    pub const fn from_event(file_path: &'static str, added_lines: u16, removed_lines: u16) -> Self {
        Self {
            present: true,
            added_lines,
            removed_lines,
            event_derived: true,
            file_path,
        }
    }

    /// Returns true when the diff is present and was derived from events
    /// (not synthetic injection).
    pub fn is_valid(&self) -> bool {
        self.present && self.event_derived
    }

    /// Total changed lines (added + removed).
    pub fn total_changed(&self) -> u16 {
        self.added_lines.saturating_add(self.removed_lines)
    }
}
