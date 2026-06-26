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
    ThemeDialog,
    ErrorDetails,
    PromptStashList,
    AuthDialog,
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
    pub theme_dialog_visible: bool,
    pub error_details_visible: bool,
    pub prompt_stash_list_visible: bool,
    pub auth_dialog_visible: bool,
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
        if state.theme_dialog_visible && !state.permission_pending {
            overlays.push(OverlayKind::ThemeDialog);
        }
        if state.error_details_visible && !state.permission_pending {
            overlays.push(OverlayKind::ErrorDetails);
        }
        if state.prompt_stash_list_visible && !state.permission_pending {
            overlays.push(OverlayKind::PromptStashList);
        }
        if state.permission_pending {
            overlays.push(OverlayKind::PermissionModal);
        }
        if state.auth_dialog_visible {
            overlays.push(OverlayKind::AuthDialog);
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
                    | OverlayKind::ThemeDialog
                    | OverlayKind::ErrorDetails
                    | OverlayKind::PromptStashList
                    | OverlayKind::AuthDialog,
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
#[path = "overlay_tests.rs"]
mod tests;
