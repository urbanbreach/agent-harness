//! Per-card compaction expansion and the collapsed-history transcript mode.
//!
//! Expansion is keyed by the compaction event sequence, so every card opens and closes on
//! its own. The key toggles the card that is selected, otherwise the first card at or below
//! the top of the viewport, otherwise the newest one; a click on a divider toggles that
//! card.
use super::{AppState, ToastVariant};
use crate::ui::TranscriptMouseTarget;
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

pub(crate) struct CompactionViewState {
    expanded: BTreeSet<u64>,
    collapse_history: bool,
    config_path: Option<PathBuf>,
}

impl Default for CompactionViewState {
    fn default() -> Self {
        Self {
            expanded: BTreeSet::new(),
            collapse_history: true,
            config_path: None,
        }
    }
}

impl AppState {
    /// Loads the persisted collapsed-history preference and remembers the file it is
    /// written to.
    pub fn configure_collapsed_history(&mut self) {
        let context = harness_core::config::ConfigDiscoveryContext::from_env();
        match harness_core::config::collapse_compacted_history_settings(
            self.settings_project_config_path.as_deref(),
            &context,
        ) {
            Ok((enabled, path)) => {
                self.compaction_view.collapse_history = enabled;
                self.compaction_view.config_path = Some(path);
            }
            Err(error) => self.show_toast(error.to_string(), ToastVariant::Error),
        }
    }

    /// Whether history above the latest compaction summary is hidden, except for the
    /// originals the summary does not cover.
    pub fn collapse_compacted_history(&self) -> bool {
        self.compaction_view.collapse_history
    }

    /// Persists and applies the collapsed-history preference. Returns whether it was
    /// written; on failure the setting is unchanged and a toast explains why.
    pub fn set_collapse_compacted_history(&mut self, enabled: bool) -> bool {
        let Some(path) = self.compaction_view.config_path.as_ref() else {
            self.show_toast("No TUI config path is available.", ToastVariant::Error);
            return false;
        };
        match harness_core::config::write_collapse_compacted_history(path, enabled) {
            Ok(()) => {
                if self.compaction_view.collapse_history != enabled {
                    self.compaction_view.collapse_history = enabled;
                    self.bump_transcript_render_epoch();
                }
                true
            }
            Err(error) => {
                self.show_toast(error.to_string(), ToastVariant::Error);
                false
            }
        }
    }

    pub(crate) fn toggle_collapsed_history(&mut self) {
        let next = !self.compaction_view.collapse_history;
        if self.set_collapse_compacted_history(next) {
            self.show_toast(
                if next {
                    "Compacted history collapsed"
                } else {
                    "Compacted history shown"
                },
                ToastVariant::Info,
            );
        }
    }

    pub(crate) fn compaction_expanded(&self, seq: u64) -> bool {
        self.compaction_view.expanded.contains(&seq)
    }

    pub(crate) fn toggle_compaction_card(&mut self, seq: u64) {
        if !self.compaction_view.expanded.remove(&seq) {
            self.compaction_view.expanded.insert(seq);
        }
        self.bump_transcript_render_epoch();
    }

    /// Toggles the card the key acts on: the selected one, else the first card at or below
    /// the viewport top, else the newest.
    pub(crate) fn toggle_focused_compaction_details(&mut self) {
        let area = self
            .last_frame_area
            .unwrap_or(ratatui::layout::Rect::new(0, 0, 80, 24));
        let cards = crate::ui::transcript_navigation_entries(self, area)
            .into_iter()
            .filter_map(|entry| match entry.target {
                Some(TranscriptMouseTarget::Compaction { seq }) => {
                    Some((seq, entry.id, entry.top, entry.height))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let selected = self.transcript_view.selected_entry;
        let top = self.transcript_view.measured_viewport().top();
        let target = cards
            .iter()
            .find(|(_, id, ..)| Some(*id) == selected)
            .or_else(|| cards.iter().find(|(_, _, row, height)| row + height > top))
            .or(cards.last());
        if let Some(&(seq, ..)) = target {
            self.toggle_compaction_card(seq);
        }
    }

    pub(in crate::app) fn hash_compaction_view(&self, hasher: &mut impl Hasher) {
        self.compaction_view.expanded.hash(hasher);
        self.compaction_view.collapse_history.hash(hasher);
        for notice in self.notices.rows() {
            notice.anchor_seq.hash(hasher);
            notice.text.hash(hasher);
        }
    }
}

#[cfg(test)]
impl AppState {
    pub(crate) fn set_collapsed_history_config_for_test(&mut self, path: PathBuf, enabled: bool) {
        self.compaction_view.config_path = Some(path);
        self.compaction_view.collapse_history = enabled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_collapsed_history_persists() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("tui.json");
        let mut app = AppState::default();
        app.set_collapsed_history_config_for_test(path.clone(), true);

        app.toggle_collapsed_history();

        assert!(!app.collapse_compacted_history());
        let written = std::fs::read_to_string(&path).expect("tui.json written");
        let written: serde_json::Value = serde_json::from_str(&written).expect("valid json");
        assert_eq!(written["collapse_compacted_history"], false);
        assert!(app.set_collapse_compacted_history(true));
        assert!(app.collapse_compacted_history());
    }

    #[test]
    fn card_expansion_is_independent_per_card() {
        let mut app = AppState::default();
        app.toggle_compaction_card(7);
        assert!(app.compaction_expanded(7));
        assert!(!app.compaction_expanded(9));
        app.toggle_compaction_card(7);
        assert!(!app.compaction_expanded(7));
    }
}
