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
    pub(crate) filtering: bool,
    pub(crate) fullscreen: bool,
    pub(crate) preview_scroll: u16,
}

impl MemoryBrowserState {
    /// Move the selection by `delta` positions, clamping at boundaries.
    pub fn move_selection(&mut self, delta: isize) {
        let total = self.filtered_entries().len();
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
        self.filtered_entries().get(self.selected).copied()
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

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
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
        if let Some(root) = self
            .file_mention_workspace_root_opt()
            .filter(|_| !self.replay_mode)
        {
            let store = DurableMemoryStore::for_workspace(&root);
            if let Ok(entries) = store.search("") {
                self.memory_browser.entries = entries
                    .into_iter()
                    .map(|entry| MemoryBrowserEntry::new(entry.key, entry.value))
                    .collect();
            }
        }
        self.memory_browser.filtering = false;
        self.memory_browser.fullscreen = false;
        self.memory_browser.preview_scroll = 0;
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
        let selection_delta = match key.code {
            KeyCode::Down => 1,
            KeyCode::Up => -1,
            _ => 0,
        };
        if self.memory_browser.filtering {
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Down | KeyCode::Up => {
                    self.memory_browser.filtering = false;
                    if matches!(key.code, KeyCode::Down | KeyCode::Up) {
                        self.memory_browser.move_selection(selection_delta);
                    }
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.memory_browser.filter_input.push(c);
                    self.memory_browser.selected = 0;
                }
                KeyCode::Backspace => {
                    let _ = self.memory_browser.filter_input.pop();
                    self.memory_browser.selected = 0;
                }
                _ => {
                    self.memory_browser.filtering = false;
                    self.handle_memory_browser_key(key);
                }
            }
            self.memory_browser.preview_scroll = 0;
            self.modal_interaction.invalidate();
            return;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if let Some(entry) = self.memory_browser.selected_entry() {
                let result = crate::clipboard::copy(&entry.label);
                match result {
                    Ok(()) => self.show_toast("Memory value copied", super::ToastVariant::Info),
                    Err(err) => self.show_toast(
                        format!("clipboard copy failed: {err}"),
                        super::ToastVariant::Error,
                    ),
                }
            }
            return;
        }
        match key.code {
            KeyCode::Esc if self.memory_browser.fullscreen => {
                self.memory_browser.fullscreen = false
            }
            KeyCode::Esc => self.close_memory_browser(),
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.memory_browser.fullscreen = !self.memory_browser.fullscreen;
            }
            KeyCode::Enter => self.memory_browser.fullscreen = true,
            KeyCode::Char('/') => self.memory_browser.filtering = true,
            KeyCode::Up | KeyCode::PageUp if self.memory_browser.fullscreen => {
                self.memory_browser.preview_scroll = self
                    .memory_browser
                    .preview_scroll
                    .saturating_sub(if key.code == KeyCode::Up { 1 } else { 10 });
            }
            KeyCode::Down | KeyCode::PageDown if self.memory_browser.fullscreen => {
                self.memory_browser.preview_scroll = self
                    .memory_browser
                    .preview_scroll
                    .saturating_add(if key.code == KeyCode::Down { 1 } else { 10 });
            }
            KeyCode::Up => self.memory_browser.move_selection(-1),
            KeyCode::Down => self.memory_browser.move_selection(1),
            KeyCode::PageUp => self.memory_browser.move_selection(-10),
            KeyCode::PageDown => self.memory_browser.move_selection(10),
            KeyCode::Home => self.memory_browser.selected = 0,
            KeyCode::End => {
                self.memory_browser.selected = self
                    .memory_browser
                    .filtered_entries()
                    .len()
                    .saturating_sub(1)
            }
            _ => {}
        }
        self.modal_interaction.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnwrapOrAbort;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn memory_filter_fullscreen_and_resize_share_the_selected_value() {
        let mut app = AppState::new_live(None, false, None);
        app.open_memory_browser();
        app.memory_browser.entries = vec![
            MemoryBrowserEntry::new("alpha-one", "**First value**"),
            MemoryBrowserEntry::new("beta", "Excluded value"),
            MemoryBrowserEntry::new("alpha-two", "**Second value**\nMore recorded content"),
        ];
        for c in "/alpha".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.memory_browser.filtered_entries().len(), 2);
        assert_eq!(
            app.memory_browser.selected_entry().unwrap_or_abort().id,
            "alpha-two"
        );
        for (width, height, fullscreen) in [(80, 24, false), (80, 24, true), (40, 12, true)] {
            if fullscreen != app.memory_browser.fullscreen {
                app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
            }
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap_or_abort();
            terminal
                .draw(|frame| crate::ui::render_app(frame, &app))
                .unwrap_or_abort();
            let text = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(text.contains("Second value"), "{width}x{height}: {text}");
            assert!(!text.contains("Excluded value"));
        }
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.memory_browser.visible);
        assert!(!app.memory_browser.fullscreen);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.memory_browser.visible);
    }
}
