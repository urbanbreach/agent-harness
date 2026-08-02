//! Foreign session import picker overlay state.
//!
//! Backs the `/import` slash command journey: discover foreign session
//! candidates under a scan root, preview them in a filterable overlay, select
//! one, and emit [`UiIntent::ImportForeignSession`] so the runtime materializes
//! a new replay-only harness session. The picker itself never reads, writes, or
//! mutates any session store; all side effects are deferred to the intent
//! handler.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use harness_core::foreign_session::{
    discover_foreign_sessions, import_foreign_session_as_replay, ForeignSessionCandidate,
};

use super::lifecycle::UiIntent;
use super::AppState;

/// State for the foreign-session import picker overlay.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForeignImportPickerState {
    /// Whether the picker overlay is currently visible.
    pub visible: bool,
    /// Discovered foreign session candidates from the latest scan.
    pub candidates: Vec<ForeignSessionCandidate>,
    /// Index into `candidates` of the currently selected row.
    pub selected: usize,
    /// Scan root used for the latest discovery.
    pub scan_path: Option<PathBuf>,
    /// Human-readable error surfaced during discovery or import.
    pub error: Option<String>,
    /// One-line summary of the last successful import (shown as confirmation).
    pub last_import_summary: Option<String>,
}

impl ForeignImportPickerState {
    /// Move the selection by `delta` positions, wrapping at the boundaries.
    pub fn move_selection(&mut self, delta: isize) {
        let total = self.candidates.len();
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

    /// The currently selected candidate, if any.
    pub fn selected_candidate(&self) -> Option<&ForeignSessionCandidate> {
        self.candidates.get(self.selected)
    }

    /// Number of importable candidates in the current scan.
    pub fn importable_count(&self) -> usize {
        self.candidates.iter().filter(|c| c.is_importable()).count()
    }
}

impl AppState {
    /// Open the foreign import picker, discovering candidates under `scan_root`.
    ///
    /// Discovery is read-only and never mutates foreign or harness session data.
    pub fn open_foreign_import_picker(&mut self, scan_root: PathBuf) {
        self.close_palette();
        self.palette_focus_return.get_or_insert(self.focus);
        self.foreign_import_picker.selected = 0;
        self.foreign_import_picker.error = None;
        self.foreign_import_picker.last_import_summary = None;
        self.foreign_import_picker.candidates.clear();
        match discover_foreign_sessions(&scan_root) {
            Ok(candidates) => {
                // Pre-select the first importable candidate for convenience.
                let first_importable = candidates
                    .iter()
                    .position(|candidate| candidate.is_importable());
                self.foreign_import_picker.candidates = candidates;
                if let Some(index) = first_importable {
                    self.foreign_import_picker.selected = index;
                }
                if self.foreign_import_picker.candidates.is_empty() {
                    self.foreign_import_picker.error =
                        Some("No foreign session candidates found under the scan root".to_string());
                }
            }
            Err(err) => {
                self.foreign_import_picker.error = Some(err.to_string());
            }
        }
        self.foreign_import_picker.scan_path = Some(scan_root);
        self.foreign_import_picker.visible = true;
    }

    /// Close the foreign import picker and reset transient state.
    pub fn close_foreign_import_picker(&mut self) {
        self.foreign_import_picker.visible = false;
        self.foreign_import_picker.candidates.clear();
        self.foreign_import_picker.selected = 0;
        self.foreign_import_picker.scan_path = None;
        self.foreign_import_picker.error = None;
        if let Some(previous_focus) = self.palette_focus_return.take() {
            self.focus = previous_focus;
        }
        self.sync_slash_overlay();
        self.sync_file_mention_overlay();
    }

    /// Execute import for the currently selected candidate.
    ///
    /// Emits [`UiIntent::ImportForeignSession`] with the selected candidate
    /// path and the destination session directory. The actual import is
    /// performed by the runtime intent handler.
    fn execute_foreign_import_selection(&mut self) {
        let Some(candidate) = self.foreign_import_picker.selected_candidate() else {
            self.foreign_import_picker.error = Some("no candidate selected".to_string());
            return;
        };
        if !candidate.is_importable() {
            let path = candidate.path().display().to_string();
            self.foreign_import_picker.error = Some(format!(
                "selected candidate at `{path}` is not importable (corrupt or unsupported marker)"
            ));
            return;
        }
        let Some(dest_session_dir) = self.session_path.clone() else {
            self.foreign_import_picker.error =
                Some("no session path available for import destination".to_string());
            return;
        };
        let source_path = candidate.path().to_path_buf();
        self.emit_ui_intent(UiIntent::ImportForeignSession {
            source_path,
            dest_session_dir,
        });
        self.set_status_banner(Some(
            "foreign session import requested; replay session will be created".to_string(),
        ));
        self.close_foreign_import_picker();
    }

    /// Perform an inline foreign import (TUI-local, for deterministic tests).
    ///
    /// Calls the core import directly and records the result in the picker
    /// state. Used by the `/import` journey test to prove the full
    /// discover → preview → import → events-appended chain without requiring
    /// a live runtime intent handler.
    pub fn execute_foreign_import_inline(
        &mut self,
    ) -> Option<harness_core::foreign_session::ForeignImportResult> {
        let Some(candidate) = self.foreign_import_picker.selected_candidate() else {
            self.foreign_import_picker.error = Some("no candidate selected".to_string());
            return None;
        };
        if !candidate.is_importable() {
            let path = candidate.path().display().to_string();
            self.foreign_import_picker.error =
                Some(format!("selected candidate at `{path}` is not importable"));
            return None;
        }
        let Some(dest_session_dir) = self.session_path.clone() else {
            self.foreign_import_picker.error =
                Some("no session path available for import destination".to_string());
            return None;
        };
        let source_path = candidate.path().to_path_buf();
        match import_foreign_session_as_replay(&source_path, &dest_session_dir) {
            Ok(result) => {
                self.foreign_import_picker.last_import_summary = Some(result.one_line());
                self.set_status_banner(Some(format!(
                    "imported {} events from `{}`",
                    result.event_count,
                    result.source_path.display()
                )));
                Some(result)
            }
            Err(err) => {
                self.foreign_import_picker.error = Some(err.to_string());
                None
            }
        }
    }

    /// Handle keyboard input while the foreign import picker overlay is visible.
    pub(in crate::app) fn handle_foreign_import_picker_key(&mut self, key: &KeyEvent) -> bool {
        let ctrl_only = key.modifiers == KeyModifiers::CONTROL;
        match key.code {
            KeyCode::Esc => {
                self.close_foreign_import_picker();
                true
            }
            KeyCode::Char('c') if ctrl_only => {
                self.close_foreign_import_picker();
                true
            }
            KeyCode::Enter => {
                self.execute_foreign_import_selection();
                true
            }
            KeyCode::Up => {
                self.foreign_import_picker.move_selection(-1);
                true
            }
            KeyCode::Down => {
                self.foreign_import_picker.move_selection(1);
                true
            }
            KeyCode::Char('p') if ctrl_only => {
                self.foreign_import_picker.move_selection(-1);
                true
            }
            KeyCode::Char('n') if ctrl_only => {
                self.foreign_import_picker.move_selection(1);
                true
            }
            KeyCode::PageUp => {
                self.foreign_import_picker.move_selection(-10);
                true
            }
            KeyCode::PageDown => {
                self.foreign_import_picker.move_selection(10);
                true
            }
            KeyCode::Home => {
                self.foreign_import_picker.selected = 0;
                true
            }
            KeyCode::End => {
                self.foreign_import_picker.selected = self
                    .foreign_import_picker
                    .candidates
                    .len()
                    .saturating_sub(1);
                true
            }
            _ => false,
        }
    }
}
