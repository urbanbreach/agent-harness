//! Worktree picker state for switching the active session worktree.
//!
//! Backs the worktree picker overlay surface. Opening the picker enumerates the
//! git worktrees linked to the active workspace; confirming a row emits a
//! [`UiIntent::SwitchWorktree`] so the runtime can relaunch the live session in
//! the selected checkout. The picker itself never mutates the workspace.

use crossterm::event::{KeyCode, KeyEvent};
use harness_core::worktree::{list_session_worktrees, ListedWorktree};

use super::lifecycle::UiIntent;
use super::AppState;

/// State for the worktree picker overlay.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreePickerState {
    /// Whether the picker overlay is currently visible.
    pub visible: bool,
    /// Enumerated worktrees available to switch to.
    pub entries: Vec<ListedWorktree>,
    /// Index into `entries` of the currently selected row.
    pub selected: usize,
    /// Human-readable error surfaced while listing worktrees.
    pub error: Option<String>,
}

impl WorktreePickerState {
    /// Move the selection by `delta` positions, wrapping at the boundaries.
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

    /// The currently selected worktree, if any.
    pub fn selected_entry(&self) -> Option<&ListedWorktree> {
        self.entries.get(self.selected)
    }
}

impl AppState {
    /// Open the worktree picker, enumerating worktrees for the active workspace.
    pub fn open_worktree_picker(&mut self) {
        self.close_palette();
        self.palette_focus_return.get_or_insert(self.focus);
        self.worktree_picker.selected = 0;
        self.worktree_picker.error = None;
        self.worktree_picker.entries.clear();
        match self.file_mention_workspace_root_opt() {
            Some(repository_root) => match list_session_worktrees(&repository_root, None) {
                Ok(entries) => self.worktree_picker.entries = entries,
                Err(err) => self.worktree_picker.error = Some(err.to_string()),
            },
            None => {
                self.worktree_picker.error =
                    Some("No workspace root available for worktree listing".to_string());
            }
        }
        self.worktree_picker.visible = true;
    }

    /// Close the worktree picker and reset its transient state.
    pub fn close_worktree_picker(&mut self) {
        self.worktree_picker.visible = false;
        self.worktree_picker.entries.clear();
        self.worktree_picker.selected = 0;
        self.worktree_picker.error = None;
        if let Some(previous_focus) = self.palette_focus_return.take() {
            self.focus = previous_focus;
        }
    }

    /// Confirm the selected worktree, emitting a switch intent and closing.
    pub(in crate::app) fn confirm_worktree_switch(&mut self) {
        if let Some(entry) = self.worktree_picker.selected_entry().cloned() {
            let worktree_path = entry.path.clone();
            self.emit_ui_intent(UiIntent::SwitchWorktree { worktree_path });
            self.close_worktree_picker();
        }
    }

    /// Route a key event while the worktree picker overlay is active.
    pub(in crate::app) fn handle_worktree_picker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.close_worktree_picker(),
            KeyCode::Enter => self.confirm_worktree_switch(),
            KeyCode::Up => self.worktree_picker.move_selection(-1),
            KeyCode::Down => self.worktree_picker.move_selection(1),
            KeyCode::PageUp => self.worktree_picker.move_selection(-10),
            KeyCode::PageDown => self.worktree_picker.move_selection(10),
            KeyCode::Home => self.worktree_picker.selected = 0,
            KeyCode::End => {
                self.worktree_picker.selected =
                    self.worktree_picker.entries.len().saturating_sub(1);
            }
            _ => {}
        }
    }
}
