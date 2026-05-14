#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKind {
    DetailsDrawer,
    SlashCommands,
    FileMentions,
    CommandPalette,
    TogglesMenu,
    LineageBrowser,
    ForkSelector,
    StatusDialog,
    PermissionModal,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OverlayState {
    pub details_drawer_open: bool,
    pub slash_visible: bool,
    pub file_mention_visible: bool,
    pub palette_visible: bool,
    pub status_dialog_visible: bool,
    pub session_history_visible: bool,
    pub model_switcher_visible: bool,
    pub toggles_menu_visible: bool,
    pub lineage_browser_visible: bool,
    pub fork_selector_visible: bool,
    pub permission_pending: bool,
}

impl OverlayState {
    pub fn command_palette_channel_visible(self) -> bool {
        self.palette_visible
            || self.session_history_visible
            || self.model_switcher_visible
            || self.toggles_menu_visible
            || self.lineage_browser_visible
            || self.fork_selector_visible
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OverlayStack {
    overlays: Vec<OverlayKind>,
}

impl OverlayStack {
    pub fn from_state(state: OverlayState) -> Self {
        let mut overlays = Vec::with_capacity(4);
        if state.details_drawer_open {
            overlays.push(OverlayKind::DetailsDrawer);
        }
        if state.slash_visible && !state.permission_pending {
            overlays.push(OverlayKind::SlashCommands);
        }
        if state.file_mention_visible && !state.permission_pending {
            overlays.push(OverlayKind::FileMentions);
        }
        if state.command_palette_channel_visible() && !state.permission_pending {
            if state.toggles_menu_visible {
                overlays.push(OverlayKind::TogglesMenu);
            } else if state.lineage_browser_visible {
                overlays.push(OverlayKind::LineageBrowser);
            } else if state.fork_selector_visible {
                overlays.push(OverlayKind::ForkSelector);
            } else {
                overlays.push(OverlayKind::CommandPalette);
            }
        }
        if state.status_dialog_visible && !state.permission_pending {
            overlays.push(OverlayKind::StatusDialog);
        }
        if state.permission_pending {
            overlays.push(OverlayKind::PermissionModal);
        }
        Self { overlays }
    }

    pub fn top(&self) -> Option<OverlayKind> {
        self.overlays.last().copied()
    }

    pub fn ordered(&self) -> &[OverlayKind] {
        &self.overlays
    }

    pub fn blocks_pointer_interaction(&self) -> bool {
        matches!(
            self.top(),
            Some(
                OverlayKind::SlashCommands
                    | OverlayKind::FileMentions
                    | OverlayKind::CommandPalette
                    | OverlayKind::TogglesMenu
                    | OverlayKind::LineageBrowser
                    | OverlayKind::ForkSelector
                    | OverlayKind::StatusDialog
                    | OverlayKind::PermissionModal
            )
        )
    }
}

impl<'a> IntoIterator for &'a OverlayStack {
    type Item = OverlayKind;
    type IntoIter = std::iter::Copied<std::slice::Iter<'a, OverlayKind>>;

    fn into_iter(self) -> Self::IntoIter {
        self.overlays.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_palette_channel_visible() {
        let mut state = OverlayState::default();

        // Initially should be false
        assert!(!state.command_palette_channel_visible());

        // Changing unrelated fields should not affect it
        state.details_drawer_open = true;
        state.slash_visible = true;
        state.file_mention_visible = true;
        state.status_dialog_visible = true;
        state.permission_pending = true;
        assert!(!state.command_palette_channel_visible());

        // Test each field that should make it visible
        let mut state = OverlayState::default();
        state.palette_visible = true;
        assert!(state.command_palette_channel_visible());

        let mut state = OverlayState::default();
        state.session_history_visible = true;
        assert!(state.command_palette_channel_visible());

        let mut state = OverlayState::default();
        state.model_switcher_visible = true;
        assert!(state.command_palette_channel_visible());

        let mut state = OverlayState::default();
        state.toggles_menu_visible = true;
        assert!(state.command_palette_channel_visible());

        let mut state = OverlayState::default();
        state.lineage_browser_visible = true;
        assert!(state.command_palette_channel_visible());

        let mut state = OverlayState::default();
        state.fork_selector_visible = true;
        assert!(state.command_palette_channel_visible());

        // Test multiple fields
        let mut state = OverlayState::default();
        state.palette_visible = true;
        state.toggles_menu_visible = true;
        assert!(state.command_palette_channel_visible());
    }
}
