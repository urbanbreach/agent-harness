use std::collections::BTreeSet;

use crate::ui::OperatorSidebarSelection;

use super::{OperatorSidebarPendingClick, OperatorSidebarSection};

/// Local UI toggles for secondary surfaces (status dialog, section, focus).
/// Event-derived operator facts stay on SessionProjection, not here.
#[derive(Debug, Clone)]
pub(crate) struct SecondarySurfaceState {
    status_dialog_visible: bool,
    selected_section: Option<OperatorSidebarSection>,
    focused: bool,
    pub(crate) selection: Option<OperatorSidebarSelection>,
    pub(crate) keyboard_index: Option<usize>,
    pub(crate) selection_dragging: bool,
    pub(super) pending_click: Option<OperatorSidebarPendingClick>,
    pub(crate) collapsed_sections: BTreeSet<OperatorSidebarSection>,
    pub(crate) expanded_subagent_groups: BTreeSet<String>,
}

impl Default for SecondarySurfaceState {
    fn default() -> Self {
        Self {
            status_dialog_visible: false,
            selected_section: None,
            focused: false,
            selection: None,
            keyboard_index: None,
            selection_dragging: false,
            pending_click: None,
            collapsed_sections: BTreeSet::from([OperatorSidebarSection::ModifiedFiles]),
            expanded_subagent_groups: BTreeSet::new(),
        }
    }
}

impl SecondarySurfaceState {
    pub(crate) const fn status_dialog_visible(&self) -> bool {
        self.status_dialog_visible
    }

    pub(crate) fn open_status_dialog(&mut self) {
        self.status_dialog_visible = true;
    }

    pub(crate) fn close_status_dialog(&mut self) {
        self.status_dialog_visible = false;
    }

    pub(crate) fn set_status_dialog_visible(&mut self, visible: bool) {
        self.status_dialog_visible = visible;
    }

    pub(crate) const fn selected_section(&self) -> Option<OperatorSidebarSection> {
        self.selected_section
    }

    pub(crate) fn set_selected_section(&mut self, section: Option<OperatorSidebarSection>) {
        self.selected_section = section;
    }

    pub(crate) const fn focused(&self) -> bool {
        self.focused
    }

    pub(crate) fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    pub(crate) fn toggle_section(&mut self, section: OperatorSidebarSection) {
        if !self.collapsed_sections.insert(section) {
            self.collapsed_sections.remove(&section);
        }
    }

    pub(crate) fn section_collapsed(&self, section: OperatorSidebarSection) -> bool {
        self.collapsed_sections.contains(&section)
    }
}

pub(crate) type OperatorSidebarState = SecondarySurfaceState;

#[cfg(test)]
mod tests {
    use super::*;

    // Relocated from dashboard_queue_worktree_consistency_test.rs: section
    // collapse/expand toggles pub(crate) SecondarySurfaceState state.
    #[test]
    fn dashboard_roster_group_collapses_and_expands_sections() {
        // arrange
        // act
        let mut state = SecondarySurfaceState::default();
        // Subagents section starts expanded (not in collapsed_sections by default).
        // assert
        assert!(
            !state.section_collapsed(OperatorSidebarSection::Subagents),
            "Subagents section must start expanded"
        );
        state.toggle_section(OperatorSidebarSection::Subagents);
        assert!(
            state.section_collapsed(OperatorSidebarSection::Subagents),
            "toggling Subagents section must collapse it"
        );
        state.toggle_section(OperatorSidebarSection::Subagents);
        assert!(
            !state.section_collapsed(OperatorSidebarSection::Subagents),
            "toggling again must expand it"
        );
    }
}
