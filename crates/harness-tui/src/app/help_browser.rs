mod catalog;
mod input;

use std::collections::BTreeSet;

use crossterm::event::KeyEvent;

use super::{AppState, Focus, ReviewSurface};
use crate::keybindings::{Action, HelpCategory};

pub(crate) use catalog::build_rows;
use input::HelpOutcome;

pub(crate) type HelpSection = HelpCategory;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HelpRow {
    Section {
        section: HelpSection,
        count: usize,
        collapsed: bool,
    },
    Shortcut {
        action: Action,
        label: String,
        bindings: String,
        description: String,
        dimmed: bool,
        expanded: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum HelpMode {
    #[default]
    Browse,
    Detail {
        action: Action,
        scroll: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HelpBrowserState {
    pub(crate) query: String,
    pub(crate) search_active: bool,
    pub(crate) selected: usize,
    pub(crate) collapsed_sections: BTreeSet<HelpSection>,
    pub(crate) expanded_actions: BTreeSet<Action>,
    pub(crate) hide_dimmed: bool,
    pub(crate) mode: HelpMode,
    pub(crate) return_action: Option<Action>,
    pub(crate) visual_offset: usize,
    pub(crate) follow_selection: bool,
}

impl Default for HelpBrowserState {
    fn default() -> Self {
        Self {
            query: String::new(),
            search_active: false,
            selected: 1,
            collapsed_sections: HelpSection::ALL.into_iter().skip(1).collect(),
            expanded_actions: BTreeSet::new(),
            hide_dimmed: false,
            mode: HelpMode::Browse,
            return_action: None,
            visual_offset: 0,
            follow_selection: true,
        }
    }
}

impl AppState {
    pub(crate) fn help_rows(&self) -> Vec<HelpRow> {
        build_rows(
            &self.keymap,
            self.focus,
            self.replay_mode,
            &self.help_browser,
        )
    }

    pub(in crate::app) fn handle_help_browser_key(&mut self, key: KeyEvent) -> bool {
        let rows = self.help_rows();
        let detail_max_scroll = self
            .last_frame_area()
            .and_then(|area| crate::ui::ui_overlays::modal_surface_model(self, area))
            .map_or(0, |model| model.max_scroll);
        match self.help_browser.handle_key(key, &rows, detail_max_scroll) {
            HelpOutcome::Close => self.close_review_surface(),
            HelpOutcome::Changed => {
                let rows = build_rows(
                    &self.keymap,
                    self.focus,
                    self.replay_mode,
                    &self.help_browser,
                );
                self.help_browser.normalize_selection(&rows);
            }
            HelpOutcome::Unchanged => {}
        }
        true
    }

    pub(crate) fn help_detail(&self) -> Option<(Action, usize)> {
        match self.help_browser.mode {
            HelpMode::Browse => None,
            HelpMode::Detail { action, scroll } => Some((action, scroll)),
        }
    }

    pub(crate) fn help_is_open(&self) -> bool {
        self.active_review_surface == Some(ReviewSurface::Help)
    }
}
