//! Memory browser state for browsing session memory and context entries.
//!
//! This module backs the memory browser overlay surface, which allows
//! operators to inspect session-local memory entries, context windows,
//! and compaction checkpoints. The browser is read-only and replay-safe.

/// A single memory entry displayed in the memory browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryBrowserEntry {
    /// Stable identifier for the memory entry.
    pub id: String,
    /// Human-readable label for the entry.
    pub label: String,
}

impl MemoryBrowserEntry {
    /// Create a new memory browser entry.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

/// State for the memory browser overlay.
///
/// The browser is read-only: it displays memory entries derived from
/// session projection and does not mutate runtime state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryBrowserState {
    /// Whether the browser overlay is currently visible.
    pub visible: bool,
    /// Memory entries to display (may be empty when no session is active).
    pub entries: Vec<MemoryBrowserEntry>,
    /// Index of the currently selected entry.
    pub selected: usize,
    /// Filter input for narrowing entries.
    pub filter_input: String,
}

impl MemoryBrowserState {
    /// Move the selection by `delta` positions, clamping at boundaries.
    pub fn move_selection(&mut self, delta: isize) {
        let total = self.entries.len();
        if total == 0 {
            self.selected = 0;
            return;
        }
        let amount = delta.unsigned_abs() % total;
        self.selected = if delta < 0 {
            if amount <= self.selected {
                self.selected - amount
            } else {
                total - (amount - self.selected)
            }
        } else {
            (self.selected + amount) % total
        };
    }

    /// Get the currently selected entry, if any.
    pub fn selected_entry(&self) -> Option<&MemoryBrowserEntry> {
        self.entries.get(self.selected)
    }

    /// Filtered entries based on the current filter input.
    pub fn filtered_entries(&self) -> Vec<&MemoryBrowserEntry> {
        if self.filter_input.is_empty() {
            self.entries.iter().collect()
        } else {
            let filter = self.filter_input.to_lowercase();
            self.entries
                .iter()
                .filter(|e| {
                    e.id.to_lowercase().contains(&filter)
                        || e.label.to_lowercase().contains(&filter)
                })
                .collect()
        }
    }
}

use crossterm::event::{KeyCode, KeyEvent};
use harness_core::memory::DurableMemoryStore;

use super::AppState;

impl AppState {
    /// Open the memory browser, seeding entries from durable workspace memory.
    pub fn open_memory_browser(&mut self) {
        self.close_palette();
        self.palette_focus_return.get_or_insert(self.focus);
        self.memory_browser.selected = 0;
        self.memory_browser.filter_input.clear();
        self.memory_browser.entries.clear();
        if let Some(root) = self.file_mention_workspace_root_opt() {
            let store = DurableMemoryStore::for_workspace(&root);
            if let Ok(entries) = store.search("") {
                self.memory_browser.entries = entries
                    .into_iter()
                    .map(|entry| MemoryBrowserEntry::new(entry.key, entry.value))
                    .collect();
            }
        }
        self.memory_browser.visible = true;
    }

    /// Close the memory browser and reset its transient state.
    pub fn close_memory_browser(&mut self) {
        self.memory_browser.visible = false;
        self.memory_browser.entries.clear();
        self.memory_browser.selected = 0;
        self.memory_browser.filter_input.clear();
        if let Some(previous_focus) = self.palette_focus_return.take() {
            self.focus = previous_focus;
        }
    }

    /// Route a key event while the memory browser overlay is active.
    pub(in crate::app) fn handle_memory_browser_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.close_memory_browser(),
            KeyCode::Up => self.memory_browser.move_selection(-1),
            KeyCode::Down => self.memory_browser.move_selection(1),
            KeyCode::PageUp => self.memory_browser.move_selection(-10),
            KeyCode::PageDown => self.memory_browser.move_selection(10),
            KeyCode::Home => self.memory_browser.selected = 0,
            KeyCode::End => {
                self.memory_browser.selected = self.memory_browser.entries.len().saturating_sub(1);
            }
            _ => {}
        }
    }
}
