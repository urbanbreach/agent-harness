// allow: SIZE_OK — keybinding data and command registry (palette entries)
use crate::keybindings::Action;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PaletteCategory {
    Suggested,
    Session,
    Context,
    ModelInput,
    Tools,
    Agent,
    System,
    Workspace,
    Provider,
    Prompt,
}

impl PaletteCategory {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Suggested => "Suggested",
            Self::Session => "Session",
            Self::Context => "Context",
            Self::ModelInput => "Model & Input",
            Self::Tools => "Tools",
            Self::Agent => "Agent",
            Self::System => "System",
            Self::Workspace => "Workspace",
            Self::Provider => "Provider",
            Self::Prompt => "Prompt",
        }
    }
}

/// Dynamic title rule for toggle commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynamicTitle {
    /// Static title.
    Static(&'static str),
    /// Toggle: "Show X" when hidden, "Hide X" when shown.
    ShowHide {
        show: &'static str,
        hide: &'static str,
    },
    /// Toggle: "Enable X" when off, "Disable X" when on.
    Toggle {
        enable: &'static str,
        disable: &'static str,
    },
}

/// Suggested rule for the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestedRule {
    Never,
    Always,
    WhenSessionsExist,
    WhenSessionRoute,
    WhenDisconnected,
}

/// Dispatch target for a palette command.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaletteDispatch {
    /// Execute a keybinding Action.
    Action(Action),
    /// Toggle a transcript view field (local AppState mutation).
    ToggleTranscriptThinking,
    ToggleTranscriptTimestamps,
    ToggleToolDetails,
    ToggleGenericToolOutput,
    ToggleStackedDiffs,
    ExpandTurnResults,
    CollapseTurnResults,
    /// Open a dialog/overlay.
    OpenSessionHistory,
    OpenSessionRename,
    OpenForkSelector,
    OpenModelSwitcher,
    OpenTogglesMenu,
    OpenAuth,
    OpenEventLog,
    OpenConnectDialog,
    /// Emit a UiIntent.
    NewSession,
    NewWorktreeSession,
    CompactSession,
    /// Copy the full session transcript to clipboard.
    CopySessionTranscript,
    /// Not yet implemented; opens placeholder.
    Placeholder,
}

impl PaletteCommandEntry {
    pub fn freeze_shortcut(&self) -> &'static str {
        match self.id {
            "session.new" => "Ctrl+N",
            "session.new.worktree" => "Ctrl+P \u{2192} worktree",
            "session.dashboard" => "/dashboard",
            "session.home" => "/home",
            "session.list" => "/resume",
            "session.rename" => "/rename",
            "session.info" => "/session-info",
            "session.feedback" => "/feedback",
            "session.compact" => "/compact",
            "context.usage" => "/context",
            "context.view_plan" => "/view-plan",
            "context.memory" => "/memory",
            "worktree.switch" => "/worktree",
            "model.list" => "/model",
            "model.always_approve" => "/always-approve",
            "model.multiline" => "/multiline",
            "tools.hooks" => "/hooks",
            "tools.plugins" => "/plugins",
            "tools.marketplace" => "/marketplace",
            _ => "",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PaletteCommandEntry {
    /// Stable Harness command ID (e.g., "session.new").
    pub id: &'static str,
    /// Category for grouping.
    pub category: PaletteCategory,
    /// Title rule (static or dynamic).
    pub title: DynamicTitle,
    /// Description for the row.
    pub description: &'static str,
    /// Suggested rule.
    pub suggested: SuggestedRule,
    /// Dispatch target.
    pub dispatch: PaletteDispatch,
}

pub const PALETTE_COMMAND_ENTRIES: &[PaletteCommandEntry] = &[
    PaletteCommandEntry {
        id: "session.new",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("New Session"),
        description: "Start a fresh live session",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::NewSession,
    },
    PaletteCommandEntry {
        id: "session.new.worktree",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("New Session in Worktree"),
        description: "Create a git worktree and start a fresh session rooted in it",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::NewWorktreeSession,
    },
    PaletteCommandEntry {
        id: "session.dashboard",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Agent Dashboard"),
        description: "Open the agent dashboard",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::OpenStatusDialog),
    },
    PaletteCommandEntry {
        id: "session.home",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Back to Home"),
        description: "Return to the home shell",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::CloseReviewSurface),
    },
    PaletteCommandEntry {
        id: "session.list",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Resume Session"),
        description: "Continue a prior session when resumable",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::OpenSessionHistory,
    },
    PaletteCommandEntry {
        id: "session.rename",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Rename Session"),
        description: "Rename the current session",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::OpenSessionRename,
    },
    PaletteCommandEntry {
        id: "session.info",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Session Info"),
        description: "Open status and session details",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::OpenStatusDialog),
    },
    PaletteCommandEntry {
        id: "session.feedback",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Send Feedback"),
        description: "Open help (feedback action maps to help surface)",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::Help),
    },
    PaletteCommandEntry {
        id: "session.fork",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Fork session"),
        description: "Fork the current session",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::OpenForkSelector,
    },
    PaletteCommandEntry {
        id: "session.compact",
        category: PaletteCategory::Context,
        title: DynamicTitle::Static("Compact History"),
        description: "Write a manual context checkpoint",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::CompactSession,
    },
    PaletteCommandEntry {
        id: "context.usage",
        category: PaletteCategory::Context,
        title: DynamicTitle::Static("Context Usage"),
        description: "Show context usage",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::OpenStatusDialog),
    },
    PaletteCommandEntry {
        id: "context.view_plan",
        category: PaletteCategory::Context,
        title: DynamicTitle::Static("View Plan"),
        description: "View plan files for this workspace/session",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::OpenViewPlan),
    },
    PaletteCommandEntry {
        id: "context.memory",
        category: PaletteCategory::Context,
        title: DynamicTitle::Static("Memory"),
        description: "Open the durable memory browser",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::OpenMemoryBrowser),
    },
    PaletteCommandEntry {
        id: "worktree.switch",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Switch Worktree"),
        description: "Switch the active session worktree",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::OpenWorktreePicker),
    },
    PaletteCommandEntry {
        id: "session.status.open",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Open status"),
        description: "Open status and session details",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::OpenStatusDialog),
    },
    PaletteCommandEntry {
        id: "session.toggle.timestamps",
        category: PaletteCategory::Session,
        title: DynamicTitle::ShowHide {
            show: "Show timestamps",
            hide: "Hide timestamps",
        },
        description: "Toggle user message timestamps",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::ToggleTranscriptTimestamps,
    },
    PaletteCommandEntry {
        id: "session.toggle.thinking",
        category: PaletteCategory::Session,
        title: DynamicTitle::ShowHide {
            show: "Expand thinking",
            hide: "Collapse thinking",
        },
        description: "Toggle inline thinking rows",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::ToggleTranscriptThinking,
    },
    PaletteCommandEntry {
        id: "session.toggle.actions",
        category: PaletteCategory::Session,
        title: DynamicTitle::ShowHide {
            show: "Show tool details",
            hide: "Hide tool details",
        },
        description: "Toggle completed successful tools",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::ToggleToolDetails,
    },
    PaletteCommandEntry {
        id: "session.toggle.scrollbar",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Toggle session scrollbar"),
        description: "Toggle the session scrollbar",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::ToggleScrollbar),
    },
    PaletteCommandEntry {
        id: "session.toggle.generic_tool_output",
        category: PaletteCategory::Session,
        title: DynamicTitle::ShowHide {
            show: "Show generic tool output",
            hide: "Hide generic tool output",
        },
        description: "Toggle generic tool payload blocks",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::ToggleGenericToolOutput,
    },
    PaletteCommandEntry {
        id: "messages.copy",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Copy last assistant message"),
        description: "Copy the last assistant message to clipboard",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::CopyMessage),
    },
    PaletteCommandEntry {
        id: "session.copy",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Copy session transcript"),
        description: "Copy the session transcript to clipboard",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::CopySessionTranscript,
    },
    PaletteCommandEntry {
        id: "session.export",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Export session transcript"),
        description: "Export the session transcript to a file",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::ExportSession),
    },
    // === Agent ===
    PaletteCommandEntry {
        id: "model.list",
        category: PaletteCategory::ModelInput,
        title: DynamicTitle::Static("Switch Model"),
        description: "Browse available provider/model options",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::OpenModelSwitcher,
    },
    PaletteCommandEntry {
        id: "agent.list",
        category: PaletteCategory::ModelInput,
        title: DynamicTitle::Static("Switch agent"),
        description: "Switch the active agent profile",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::OpenModelSwitcher,
    },
    PaletteCommandEntry {
        id: "mcp.list",
        category: PaletteCategory::ModelInput,
        title: DynamicTitle::Static("Toggle MCPs"),
        description: "Toggle MCP server registrations",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::OpenTogglesMenu,
    },
    PaletteCommandEntry {
        id: "variant.cycle",
        category: PaletteCategory::ModelInput,
        title: DynamicTitle::Static("Variant cycle"),
        description: "Cycle the configured model variant/reasoning preset",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::VariantCycle),
    },
    PaletteCommandEntry {
        id: "model.always_approve",
        category: PaletteCategory::ModelInput,
        title: DynamicTitle::Static("Always Approve Mode"),
        description: "Toggle always-approve (YOLO) mode for tool permissions",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::OpenTogglesMenu,
    },
    PaletteCommandEntry {
        id: "model.multiline",
        category: PaletteCategory::ModelInput,
        title: DynamicTitle::Static("Multiline Input"),
        description: "Toggle multiline input mode in the composer",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::InsertNewline),
    },
    // === Tools ===
    PaletteCommandEntry {
        id: "tools.hooks",
        category: PaletteCategory::Tools,
        title: DynamicTitle::Static("Hooks"),
        description: "Manage hooks configuration",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::OpenTogglesMenu,
    },
    PaletteCommandEntry {
        id: "tools.plugins",
        category: PaletteCategory::Tools,
        title: DynamicTitle::Static("Plugins"),
        description: "Manage plugins",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::OpenTogglesMenu,
    },
    PaletteCommandEntry {
        id: "tools.marketplace",
        category: PaletteCategory::Tools,
        title: DynamicTitle::Static("Marketplace"),
        description: "Browse the plugin marketplace",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::Help),
    },
    // === Workspace ===
    // === Provider ===
    PaletteCommandEntry {
        id: "provider.connect",
        category: PaletteCategory::Provider,
        title: DynamicTitle::Static("Connect provider"),
        description: "Connect a provider",
        suggested: SuggestedRule::WhenDisconnected,
        dispatch: PaletteDispatch::OpenConnectDialog,
    },
    // === Prompt ===
    PaletteCommandEntry {
        id: "prompt.stash",
        category: PaletteCategory::Prompt,
        title: DynamicTitle::Static("Stash prompt"),
        description: "Stash the current composer draft",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::PromptStash),
    },
    PaletteCommandEntry {
        id: "prompt.stash.pop",
        category: PaletteCategory::Prompt,
        title: DynamicTitle::Static("Stash pop"),
        description: "Restore the most recently stashed prompt",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::PromptStashPop),
    },
    PaletteCommandEntry {
        id: "prompt.stash.list",
        category: PaletteCategory::Prompt,
        title: DynamicTitle::Static("Stash list"),
        description: "Browse stashed prompts",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::PromptStashList),
    },
    // === System ===
    PaletteCommandEntry {
        id: "settings.list",
        category: PaletteCategory::System,
        title: DynamicTitle::Static("Settings"),
        description: "Browse typed settings registry entries (read-only)",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::OpenSettings),
    },
    PaletteCommandEntry {
        id: "app.exit",
        category: PaletteCategory::System,
        title: DynamicTitle::Static("Exit the app"),
        description: "Quit the application",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::Quit),
    },
];

const INTERNAL_COMMAND_ENTRIES: &[PaletteCommandEntry] = &[
    PaletteCommandEntry {
        id: "harness.close_review_surface",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Session shell"),
        description: "Return to the transcript-first session shell",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::CloseReviewSurface),
    },
    PaletteCommandEntry {
        id: "harness.toggle_terminal_panel",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Toggle terminal panel"),
        description: "Show or hide shell command output",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::ToggleTerminalPanel),
    },
    PaletteCommandEntry {
        id: "harness.toggle_follow",
        category: PaletteCategory::Agent,
        title: DynamicTitle::Static("Toggle follow"),
        description: "Toggle follow mode",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::ToggleFollow),
    },
    PaletteCommandEntry {
        id: "harness.revert_workspace",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Revert workspace"),
        description: "Revert workspace to the most recent snapshot",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::RevertWorkspace),
    },
    PaletteCommandEntry {
        id: "harness.open_lineage_browser",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Session tree"),
        description: "Open the Harness session lineage browser",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::OpenLineageBrowser),
    },
    PaletteCommandEntry {
        id: "harness.session_child_first",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("First child session"),
        description: "Open the first child session",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::SessionChildFirst),
    },
    PaletteCommandEntry {
        id: "harness.session_child_cycle",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Next child session"),
        description: "Cycle to the next sibling child session",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::SessionChildCycle),
    },
    PaletteCommandEntry {
        id: "harness.session_child_cycle_reverse",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Previous child session"),
        description: "Cycle to the previous sibling child session",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::SessionChildCycleReverse),
    },
    PaletteCommandEntry {
        id: "harness.session_parent",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Parent session"),
        description: "Return to the parent session",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::SessionParent),
    },
    PaletteCommandEntry {
        id: "harness.session_background",
        category: PaletteCategory::Session,
        title: DynamicTitle::Static("Background subagents"),
        description: "Move foreground subagents to the background",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::Action(Action::SessionBackground),
    },
    PaletteCommandEntry {
        id: "harness.stack_transcript_diffs",
        category: PaletteCategory::Agent,
        title: DynamicTitle::Static("Use stacked diffs"),
        description: "Force unified stacked transcript diffs",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::ToggleStackedDiffs,
    },
    PaletteCommandEntry {
        id: "harness.split_transcript_diffs",
        category: PaletteCategory::Agent,
        title: DynamicTitle::Static("Use split diffs"),
        description: "Allow side-by-side transcript diffs when wide",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::ToggleStackedDiffs,
    },
    PaletteCommandEntry {
        id: "harness.expand_turn_results",
        category: PaletteCategory::Agent,
        title: DynamicTitle::Static("Expand turn results"),
        description: "Expand overflow tool output in the selected turn",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::ExpandTurnResults,
    },
    PaletteCommandEntry {
        id: "harness.collapse_turn_results",
        category: PaletteCategory::Agent,
        title: DynamicTitle::Static("Collapse turn results"),
        description: "Collapse overflow tool output in the selected turn",
        suggested: SuggestedRule::Never,
        dispatch: PaletteDispatch::CollapseTurnResults,
    },
];

/// Get all palette command entries.
pub fn entries() -> &'static [PaletteCommandEntry] {
    PALETTE_COMMAND_ENTRIES
}

/// Find an entry by command ID.
pub fn find(id: &str) -> Option<&'static PaletteCommandEntry> {
    PALETTE_COMMAND_ENTRIES
        .iter()
        .chain(INTERNAL_COMMAND_ENTRIES.iter())
        .find(|entry| entry.id == id)
}

pub fn all_ids() -> Vec<&'static str> {
    PALETTE_COMMAND_ENTRIES
        .iter()
        .chain(INTERNAL_COMMAND_ENTRIES.iter())
        .map(|entry| entry.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn entries_have_no_duplicate_ids() {
        // arrange
        // act
        // assert
        let entries: Vec<&PaletteCommandEntry> = PALETTE_COMMAND_ENTRIES
            .iter()
            .chain(INTERNAL_COMMAND_ENTRIES.iter())
            .collect();
        let ids: HashSet<&str> = entries.iter().map(|entry| entry.id).collect();
        assert_eq!(
            ids.len(),
            entries.len(),
            "palette command entries have duplicate IDs"
        );
    }

    #[test]
    fn production_palette_entries_have_no_placeholder_dispatch() {
        // arrange
        // act
        // assert
        let placeholders: Vec<&str> = PALETTE_COMMAND_ENTRIES
            .iter()
            .chain(INTERNAL_COMMAND_ENTRIES.iter())
            .filter(|entry| matches!(entry.dispatch, PaletteDispatch::Placeholder))
            .map(|entry| entry.id)
            .collect();
        assert!(
            placeholders.is_empty(),
            "production palette must not advertise Placeholder dispatches: {placeholders:?}"
        );
    }
}
