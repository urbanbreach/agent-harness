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
    SubagentActions,
    PermissionModal,
    ThemeDialog,
    ErrorDetails,
    PromptStashList,
    AuthDialog,
    NewWorktreeDialog,
    SettingsEditor,
    PlanView,
    MemoryBrowser,
    WorktreePicker,
    ForeignImportPicker,
    TrustFolderPrompt,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OverlayState {
    pub details_drawer_open: bool,
    pub slash_visible: bool,
    pub file_mention_visible: bool,
    pub palette_visible: bool,
    pub status_dialog_visible: bool,
    pub subagent_actions_visible: bool,
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
    pub new_worktree_dialog_visible: bool,
    pub settings_editor_visible: bool,
    pub plan_view_visible: bool,
    pub memory_browser_visible: bool,
    pub worktree_picker_visible: bool,
    pub foreign_import_picker_visible: bool,
    pub trust_folder_prompt_visible: bool,
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
        if state.subagent_actions_visible && !state.permission_pending {
            overlays.push(OverlayKind::SubagentActions);
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
        if state.new_worktree_dialog_visible && !state.permission_pending {
            overlays.push(OverlayKind::NewWorktreeDialog);
        }
        if state.settings_editor_visible && !state.permission_pending {
            overlays.push(OverlayKind::SettingsEditor);
        }
        if state.plan_view_visible && !state.permission_pending {
            overlays.push(OverlayKind::PlanView);
        }
        if state.memory_browser_visible && !state.permission_pending {
            overlays.push(OverlayKind::MemoryBrowser);
        }
        if state.worktree_picker_visible && !state.permission_pending {
            overlays.push(OverlayKind::WorktreePicker);
        }
        if state.foreign_import_picker_visible && !state.permission_pending {
            overlays.push(OverlayKind::ForeignImportPicker);
        }
        if state.trust_folder_prompt_visible {
            overlays.push(OverlayKind::TrustFolderPrompt);
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
                    | OverlayKind::SubagentActions
                    | OverlayKind::PermissionModal
                    | OverlayKind::ThemeDialog
                    | OverlayKind::ErrorDetails
                    | OverlayKind::PromptStashList
                    | OverlayKind::AuthDialog
                    | OverlayKind::NewWorktreeDialog
                    | OverlayKind::SettingsEditor
                    | OverlayKind::PlanView
                    | OverlayKind::MemoryBrowser
                    | OverlayKind::WorktreePicker
                    | OverlayKind::ForeignImportPicker,
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

/// Mutable overlay controller for tests and integration scenarios.
///
/// Provides push/pop/escape/close semantics over an ordered overlay stack.
/// Re-pushing an already-open overlay is a no-op (no duplicate entries).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OverlayController {
    overlays: Vec<OverlayKind>,
}

impl OverlayController {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        !self.overlays.is_empty()
    }

    pub fn top(&self) -> Option<OverlayKind> {
        self.overlays.last().copied()
    }

    pub fn depth(&self) -> usize {
        self.overlays.len()
    }

    pub fn contains(&self, kind: OverlayKind) -> bool {
        self.overlays.contains(&kind)
    }

    /// Push an overlay. If it is already open, this is a no-op.
    pub fn push(&mut self, kind: OverlayKind) {
        if !self.contains(kind) {
            self.overlays.push(kind);
        }
    }

    pub fn pop(&mut self) -> Option<OverlayKind> {
        self.overlays.pop()
    }

    /// Close the top overlay and return it, or `None` if the stack is empty.
    pub fn escape(&mut self) -> Option<OverlayKind> {
        self.overlays.pop()
    }

    /// Close a specific overlay by kind. Returns `true` if it was found and removed.
    pub fn close(&mut self, kind: OverlayKind) -> bool {
        let idx = self.overlays.iter().position(|&k| k == kind);
        if let Some(i) = idx {
            self.overlays.remove(i);
            true
        } else {
            false
        }
    }

    pub fn close_all(&mut self) {
        self.overlays.clear();
    }

    pub fn ordered(&self) -> &[OverlayKind] {
        &self.overlays
    }
}

#[cfg(test)]
#[path = "overlay_tests.rs"]
mod tests;
