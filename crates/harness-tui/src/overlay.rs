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
    fn test_from_state_empty() {
        let state = OverlayState::default();
        let stack = OverlayStack::from_state(state);
        assert_eq!(stack.ordered(), &[]);
    }

    #[test]
    fn test_from_state_all_independent() {
        let state = OverlayState {
            details_drawer_open: true,
            slash_visible: true,
            file_mention_visible: true,
            status_dialog_visible: true,
            ..Default::default()
        };
        let stack = OverlayStack::from_state(state);
        assert_eq!(
            stack.ordered(),
            &[
                OverlayKind::DetailsDrawer,
                OverlayKind::SlashCommands,
                OverlayKind::FileMentions,
                OverlayKind::StatusDialog,
            ]
        );
    }

    #[test]
    fn test_from_state_permission_pending_overrides() {
        let state = OverlayState {
            details_drawer_open: true,
            slash_visible: true,
            file_mention_visible: true,
            palette_visible: true,
            status_dialog_visible: true,
            permission_pending: true,
            ..Default::default()
        };
        let stack = OverlayStack::from_state(state);
        assert_eq!(
            stack.ordered(),
            &[
                OverlayKind::DetailsDrawer,
                OverlayKind::PermissionModal,
            ]
        );
    }

    #[test]
    fn test_from_state_command_palette_hierarchy() {
        // TogglesMenu takes precedence over LineageBrowser, ForkSelector, and CommandPalette
        let mut state = OverlayState {
            palette_visible: true,
            toggles_menu_visible: true,
            lineage_browser_visible: true,
            fork_selector_visible: true,
            ..Default::default()
        };
        assert_eq!(
            OverlayStack::from_state(state).ordered(),
            &[OverlayKind::TogglesMenu]
        );

        // LineageBrowser takes precedence over ForkSelector and CommandPalette
        state.toggles_menu_visible = false;
        assert_eq!(
            OverlayStack::from_state(state).ordered(),
            &[OverlayKind::LineageBrowser]
        );

        // ForkSelector takes precedence over CommandPalette
        state.lineage_browser_visible = false;
        assert_eq!(
            OverlayStack::from_state(state).ordered(),
            &[OverlayKind::ForkSelector]
        );

        // CommandPalette is the default
        state.fork_selector_visible = false;
        assert_eq!(
            OverlayStack::from_state(state).ordered(),
            &[OverlayKind::CommandPalette]
        );
    }
}
