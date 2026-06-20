use std::collections::BTreeSet;

use crate::ui::OperatorSidebarSelection;

use super::OperatorSidebarPendingClick;
use super::OperatorSidebarSection;

pub(crate) struct OperatorSidebarState {
    pub(crate) selection: Option<OperatorSidebarSelection>,
    pub(crate) keyboard_index: Option<usize>,
    pub(crate) selection_dragging: bool,
    pub(super) pending_click: Option<OperatorSidebarPendingClick>,
    pub(crate) collapsed_sections: BTreeSet<OperatorSidebarSection>,
    pub(crate) expanded_subagent_groups: BTreeSet<String>,
}

impl Default for OperatorSidebarState {
    fn default() -> Self {
        Self {
            selection: None,
            keyboard_index: None,
            selection_dragging: false,
            pending_click: None,
            collapsed_sections: BTreeSet::from([OperatorSidebarSection::ModifiedFiles]),
            expanded_subagent_groups: BTreeSet::new(),
        }
    }
}
