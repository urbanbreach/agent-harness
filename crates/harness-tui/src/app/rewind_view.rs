//! Rewind view: snapshot and restore prompt drafts.
//!
//! Provides a history of prompt draft snapshots that can be rewound (stepped
//! backward) and forwarded (stepped forward) to restore previous draft states.
//!
//! Self-contained module — no `super::` or `crate::` imports. Included via
//! `#[path]` in integration tests and usable standalone.

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

/// A snapshot of a prompt draft at a point in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptDraftSnapshot {
    /// The prompt text.
    pub text: String,
    /// Cursor position within the text.
    pub cursor: usize,
    /// Selection anchor (if a selection was active).
    pub selection_anchor: Option<usize>,
}

// ---------------------------------------------------------------------------
// Rewind view
// ---------------------------------------------------------------------------

/// Rewind view: a history of prompt draft snapshots with rewind/forward
/// navigation.
#[derive(Debug, Clone, Default)]
pub struct RewindView {
    snapshots: Vec<PromptDraftSnapshot>,
    selected: usize,
}

impl RewindView {
    /// Create a new empty rewind view.
    pub fn new() -> Self {
        Self::default()
    }

    // -- mutation --

    /// Snapshot the current prompt draft.
    ///
    /// The new snapshot becomes the selected (current) entry.
    pub fn snapshot(&mut self, text: String, cursor: usize, selection_anchor: Option<usize>) {
        self.snapshots.push(PromptDraftSnapshot {
            text,
            cursor,
            selection_anchor,
        });
        self.selected = self.snapshots.len().saturating_sub(1);
    }

    // -- access --

    /// Number of snapshots in the history.
    pub fn len(&self) -> usize {
        self.snapshots.len()
    }

    /// Whether the history is empty.
    pub fn is_empty(&self) -> bool {
        self.snapshots.is_empty()
    }

    /// Currently selected snapshot index.
    pub fn selected_index(&self) -> usize {
        self.selected
    }

    /// Restore the currently selected snapshot (if any).
    pub fn restore(&self) -> Option<PromptDraftSnapshot> {
        if self.snapshots.is_empty() {
            return None;
        }
        let index = self.selected.min(self.snapshots.len() - 1);
        Some(self.snapshots[index].clone())
    }

    // -- navigation --

    /// Rewind (step backward) by `steps` snapshots.
    ///
    /// Returns `true` if the selection changed (or stayed at 0), `false` if
    /// the history is empty.
    pub fn rewind(&mut self, steps: usize) -> bool {
        if self.snapshots.is_empty() {
            return false;
        }
        self.selected = self.selected.saturating_sub(steps);
        true
    }

    /// Forward (step forward) by `steps` snapshots.
    ///
    /// Returns `true` if the selection changed (or stayed at last), `false` if
    /// the history is empty.
    pub fn forward(&mut self, steps: usize) -> bool {
        if self.snapshots.is_empty() {
            return false;
        }
        let max = self.snapshots.len().saturating_sub(1);
        self.selected = (self.selected + steps).min(max);
        true
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_rewind_view_starts_without_snapshot() {
        // arrange
        // act
        let view = RewindView::new();
        // assert
        assert!(view.is_empty());
        assert_eq!(view.len(), 0);
        assert!(view.restore().is_none());
    }

    #[test]
    fn snapshot_and_restore_preserves_rewind_state() {
        // arrange
        // act
        let mut view = RewindView::new();
        view.snapshot("hello".to_string(), 3, None);
        let restored = view.restore().expect("restore");
        // assert
        assert_eq!(restored.text, "hello");
        assert_eq!(restored.cursor, 3);
    }
}
