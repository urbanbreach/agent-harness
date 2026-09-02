// allow: SIZE_OK — keybinding data and command registry (palette entries)
//! Command and palette registries for TUI keybindings.

use super::Action;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum HelpCategory {
    Essentials,
    Input,
    ConversationNavigation,
    ConversationActions,
    Panels,
    Session,
    Dashboard,
}

impl HelpCategory {
    pub(crate) const ALL: [Self; 7] = [
        Self::Essentials,
        Self::Input,
        Self::ConversationNavigation,
        Self::ConversationActions,
        Self::Panels,
        Self::Session,
        Self::Dashboard,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Essentials => "Essentials",
            Self::Input => "Input",
            Self::ConversationNavigation => "Conversation Navigation",
            Self::ConversationActions => "Conversation Actions",
            Self::Panels => "Panels",
            Self::Session => "Session",
            Self::Dashboard => "Dashboard",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PaletteCommandSection {
    Suggested,
    Session,
    Agent,
    System,
}

impl PaletteCommandSection {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Suggested => "Suggested",
            Self::Session => "Sessions",
            Self::Agent => "Agents",
            Self::System => "System",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteCommand {
    pub id: &'static str,
    pub metadata_id: &'static str,
    pub shortcut: &'static str,
    pub section: PaletteCommandSection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashCommand {
    pub id: &'static str,
    pub metadata_id: &'static str,
    pub aliases: &'static [&'static str],
    pub takes_args: bool,
    pub args_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CommandMetadata {
    pub(super) id: &'static str,
    pub(super) label: &'static str,
    pub(super) description: &'static str,
}

macro_rules! define_command_metadata {
    ($(($id:literal, $label:literal, $description:literal),)*) => {
        const COMMAND_METADATA: &[CommandMetadata] = &[
            $(CommandMetadata {
                id: $id,
                label: $label,
                description: $description,
            },)*
        ];
    };
}

macro_rules! define_slash_commands {
    ($(($id:literal, $metadata_id:literal, $aliases:expr, $takes_args:literal, $args_required:literal),)*) => {
        const SLASH_COMMANDS: &[SlashCommand] = &[
            $(SlashCommand {
                id: $id,
                metadata_id: $metadata_id,
                aliases: $aliases,
                takes_args: $takes_args,
                args_required: $args_required,
            },)*
        ];
    };
}

macro_rules! define_palette_commands {
    ($(($id:literal, $metadata_id:literal, $shortcut:literal, $section:path),)*) => {
        const PALETTE_COMMANDS: &[PaletteCommand] = &[
            $(PaletteCommand {
                id: $id,
                metadata_id: $metadata_id,
                shortcut: $shortcut,
                section: $section,
            },)*
        ];
    };
}

define_command_metadata! {
    ("palette", "Command palette", "Browse and run available commands"),
    ("new_session", "New session", "Start a fresh live session"),
    ("resume_session", "Continue session", "Continue a prior session when resumable"),
    ("replay_session", "Replay session", "Replay a previous session as read-only"),
    ("switch_model", "Switch model", "Browse available provider/model options"),
    ("cycle_variant", "Cycle reasoning preset", "Cycle the configured model variant/reasoning preset"),
    ("toggles", "Toggles", "Toggle profiles, tools, hooks, MCP, YOLO"),
    ("auth", "Auth", "Manage provider login, logout, and auth status"),
    ("connect", "Connect", "Connect a provider"),
    ("close_review_surface", "Session shell", "Return to the transcript-first session shell"),
    ("toggle_terminal_panel", "Toggle terminal panel", "Show or hide shell command output below the transcript"),
    ("toggle_follow", "Toggle follow", "Toggle follow mode"),
    ("show_thinking", "Show thinking", "Restore inline thinking rows in the transcript"),
    ("hide_thinking", "Hide thinking", "Hide inline thinking rows in the transcript"),
    ("show_timestamps", "Show timestamps", "Reveal user message timestamps in the transcript"),
    ("hide_timestamps", "Hide timestamps", "Hide user message timestamps in the transcript"),
    ("show_tool_details", "Show tool details", "Show completed successful tools in the transcript"),
    ("hide_tool_details", "Hide tool details", "Hide completed successful tools in the transcript"),
    ("show_generic_tool_output", "Show generic tool output", "Expand generic tool payload blocks in the transcript"),
    ("hide_generic_tool_output", "Hide generic tool output", "Collapse generic tool payload blocks in the transcript"),
    ("expand_selected_turn_results", "Expand turn results", "Expand overflow tool output in the selected turn"),
    ("collapse_selected_turn_results", "Collapse turn results", "Collapse overflow tool output in the selected turn"),
    ("stack_transcript_diffs", "Use stacked diffs", "Force unified stacked transcript diffs"),
    ("split_transcript_diffs", "Use split diffs", "Allow side-by-side transcript diffs when wide"),
    ("diff_hunk_next", "Next diff hunk", "Jump to the next transcript diff hunk"),
    ("diff_hunk_previous", "Previous diff hunk", "Jump to the previous transcript diff hunk"),
    ("focus_next", "Next focus", "Cycle focus forward"),
    ("focus_prev", "Previous focus", "Cycle focus backward"),
    ("submit_prompt", "Submit prompt", "Submit the current prompt"),
    ("interject_prompt", "interject", "Submit the draft without cancelling the active turn"),
    ("cancel_and_replace_prompt", "cancel & replace", "Cancel the active turn and submit the draft"),
    ("insert_newline", "Insert newline", "Insert newline"),
    ("toggle_multiline", "Toggle multiline input", "Toggle persistent multiline composer mode"),
    ("clear_prompt", "Clear prompt", "Clear prompt"),
    ("move_down", "Move down", "Move down in list"),
    ("move_up", "Move up", "Move up in list"),
    ("reload", "Reload", "Reload session"),
    ("allow_permission", "Allow permission", "Allow permission"),
    ("always_approve_permission", "Always approve permission", "Open always-approve confirm for the active permission"),
    ("deny_permission", "Deny permission", "Deny permission"),
    ("dismiss_modal", "Reject permission", "Reject permission"),
    ("history_up", "History up", "History up"),
    ("history_down", "History down", "History down"),
    ("help", "Help", "Show shortcuts and TUI controls"),
    ("quit", "Quit", "Quit the application"),
    ("revert_workspace", "Revert workspace", "Revert workspace to the most recent snapshot"),
    ("scroll_up", "Scroll up", "Scroll the active transcript or detail surface up"),
    ("scroll_down", "Scroll down", "Scroll the active transcript or detail surface down"),
    ("half_page_down", "Half page down", "Scroll the transcript down by half a viewport"),
    ("cursor_left", "Cursor left", "Move the composer cursor one character left"),
    ("cursor_right", "Cursor right", "Move the composer cursor one character right"),
    ("backspace", "Backspace", "Delete the character before the composer cursor"),
    ("delete", "Delete", "Delete the character after the composer cursor"),
    ("toggle_prompt_focus", "Toggle prompt focus", "Switch focus between the composer and transcript"),
    ("toggle_tasks", "Toggle tasks", "Show or hide the task and operator details surface"),
    ("open_theme_dialog", "Theme", "Open the theme selector"),
    ("open_model_switcher", "Switch model", "Open the model selector"),
    ("first_message", "First message", "Jump to the first transcript message"),
    ("last_message", "Last message", "Jump to the last transcript message"),
    ("next_message", "Next message", "Jump to the next transcript message"),
    ("previous_message", "Previous message", "Jump to the previous transcript message"),
    ("copy_message", "Copy message", "Copy the selected transcript message"),
    ("export_session", "Export session", "Export the current session transcript"),
    ("open_error_details", "Error details", "Open details for the selected failed activity"),
    ("open_session_history", "Session history", "Browse saved sessions"),
    ("slash_new", "New", "Return to the home shell"),
    ("slash_sessions", "Sessions", "Switch session"),
    ("slash_resume", "Resume", "Continue a saved session"),
    ("slash_replay", "Replay", "Replay a saved session"),
    ("slash_fork", "Fork", "Fork session"),
    ("slash_tree", "Tree", "View the Harness session tree"),
    ("slash_clone", "Clone", "Prepare a Harness session clone"),
    ("slash_status", "Status", "View status"),
    ("slash_compact", "Compact", "Write a manual context checkpoint"),
    ("slash_rename", "Rename", "Rename the current session"),
    ("select_char_left", "Select char left", "Extend selection one char left"),
    ("select_char_right", "Select char right", "Extend selection one char right"),
    ("select_word_left", "Select word left", "Extend selection one word left"),
    ("select_word_right", "Select word right", "Extend selection one word right"),
    ("select_line", "Select line", "Select the current line"),
    ("select_all", "Select all", "Select the entire prompt buffer"),
    ("move_word_left", "Move word left", "Move cursor one word left"),
    ("move_word_right", "Move word right", "Move cursor one word right"),
    ("move_line_start", "Move line start", "Move cursor to line start"),
    ("move_line_end", "Move line end", "Move cursor to line end"),
    ("move_buffer_start", "Move buffer start", "Move cursor to buffer start"),
    ("move_buffer_end", "Move buffer end", "Move cursor to buffer end"),
    ("delete_word_forward", "Delete word forward", "Delete the word after the cursor"),
    ("delete_word_backward", "Delete word backward", "Delete the word before the cursor"),
    ("delete_line", "Delete line", "Delete the current line"),
    ("kill_to_line_start", "Kill to line start", "Delete from cursor to line start"),
    ("kill_to_line_end", "Kill to line end", "Delete from cursor to line end"),
    ("undo", "Undo", "Undo the last edit"),
    ("redo", "Redo", "Redo the last undone edit"),
    ("prompt_stash", "Stash prompt", "Stash the current composer draft to the prompt stash"),
    ("prompt_stash_pop", "Pop stashed prompt", "Restore the most recently stashed prompt to the composer"),
    ("prompt_stash_list", "Prompt stash list", "Open the prompt stash dialog to browse stashed prompts"),
    ("open_settings", "Settings", "Browse typed settings registry entries (read-only)"),
    ("open_view_plan", "View Plan", "View plan files for this workspace/session"),
    ("open_status_dialog", "Status dialog", "Open the status dialog"),
    ("open_lineage_browser", "Session tree", "Open the Harness session lineage browser"),
    ("open_memory_browser", "Memory", "Open the durable memory browser"),
    ("open_worktree_picker", "Switch worktree", "Switch the active session worktree"),
    ("session_child_first", "First child session", "Open the first child session of the current session"),
    ("session_child_cycle", "Next child session", "Cycle to the next sibling child session"),
    ("session_child_cycle_reverse", "Previous child session", "Cycle to the previous sibling child session"),
    ("session_parent", "Parent session", "Return to the parent session"),
    ("session_background", "Background subagents", "Move foreground subagents to the background"),
    ("variant_cycle", "Cycle variant", "Cycle to the next model variant"),
    ("slash_copy", "Copy transcript", "Copy the session transcript to clipboard"),
    ("slash_export", "Export transcript", "Export the session transcript to a file"),
    ("slash_timestamps", "Toggle timestamps", "Toggle user message timestamps"),
    ("slash_thinking", "Toggle thinking", "Toggle inline thinking rows"),
    ("slash_import", "Import foreign session", "Discover and import a foreign session as replay-only"),
}

pub(super) fn command_metadata(id: &str) -> Option<&'static CommandMetadata> {
    COMMAND_METADATA.iter().find(|entry| entry.id == id)
}

pub(super) const fn help_category(action: Action) -> Option<HelpCategory> {
    match action {
        Action::SubmitPrompt
        | Action::FocusNext
        | Action::Palette
        | Action::Help
        | Action::OpenStatusDialog
        | Action::VariantCycle
        | Action::Quit => Some(HelpCategory::Essentials),
        Action::InsertNewline
        | Action::ClearPrompt
        | Action::HistoryUp
        | Action::HistoryDown
        | Action::CursorLeft
        | Action::CursorRight
        | Action::Backspace
        | Action::Delete
        | Action::SelectCharLeft
        | Action::SelectCharRight
        | Action::SelectWordLeft
        | Action::SelectWordRight
        | Action::SelectLine
        | Action::SelectAll
        | Action::MoveWordLeft
        | Action::MoveWordRight
        | Action::MoveLineStart
        | Action::MoveLineEnd
        | Action::MoveBufferStart
        | Action::MoveBufferEnd
        | Action::DeleteWordForward
        | Action::DeleteWordBackward
        | Action::DeleteLine
        | Action::KillToLineStart
        | Action::KillToLineEnd
        | Action::Undo
        | Action::Redo => Some(HelpCategory::Input),
        Action::MoveDown
        | Action::MoveUp
        | Action::ScrollUp
        | Action::ScrollDown
        | Action::HalfPageDown
        | Action::TogglePromptFocus
        | Action::FocusPrev
        | Action::ToggleFollow
        | Action::FirstMessage
        | Action::LastMessage
        | Action::NextMessage
        | Action::PreviousMessage
        | Action::DiffHunkNext
        | Action::DiffHunkPrevious => Some(HelpCategory::ConversationNavigation),
        Action::CloseReviewSurface
        | Action::Reload
        | Action::CopyMessage
        | Action::ExportSession
        | Action::RevertWorkspace => Some(HelpCategory::ConversationActions),
        Action::ToggleTerminalPanel
        | Action::OpenThemeDialog
        | Action::OpenModelSwitcher
        | Action::OpenErrorDetails
        | Action::PromptStash
        | Action::PromptStashPop
        | Action::PromptStashList
        | Action::OpenSettings
        | Action::OpenViewPlan
        | Action::OpenMemoryBrowser
        | Action::OpenWorktreePicker => Some(HelpCategory::Panels),
        Action::SessionChildFirst
        | Action::SessionChildCycle
        | Action::SessionChildCycleReverse
        | Action::SessionParent
        | Action::SessionBackground
        | Action::OpenSessionHistory
        | Action::OpenLineageBrowser => Some(HelpCategory::Session),
        Action::ToggleTasks => Some(HelpCategory::Dashboard),
        Action::InterjectPrompt
        | Action::CancelAndReplacePrompt
        | Action::ToggleMultiline
        | Action::OpenEventLog
        | Action::AllowPermission
        | Action::AlwaysApprovePermission
        | Action::DenyPermission
        | Action::DismissModal
        | Action::Char(_)
        | Action::ToggleScrollbar => None,
    }
}

define_slash_commands! {
    ("new", "slash_new", &["clear"], false, false),
    ("sessions", "slash_sessions", &["resume", "continue"], false, false),
    ("fork", "slash_fork", &[], false, false),
    ("tree", "slash_tree", &[], false, false),
    ("clone", "slash_clone", &[], false, false),
    ("models", "switch_model", &["mo"], false, false),
    ("agents", "switch_model", &[], false, false),
    ("mcps", "toggles", &[], false, false),
    ("toggles", "toggles", &[], false, false),
    ("auth", "auth", &["login"], true, false),
    ("connect", "connect", &[], false, false),
    ("help", "help", &[], false, false),
    ("feedback", "help", &[], false, false),
    ("shell", "close_review_surface", &["session-shell"], false, false),
    ("follow", "toggle_follow", &[], false, false),
    ("compact", "slash_compact", &["summarize"], false, false),
    ("exit", "quit", &["quit", "q"], false, false),
    ("rename", "slash_rename", &[], true, true),
    ("copy", "slash_copy", &[], false, false),
    ("export", "slash_export", &[], false, false),
    ("timestamps", "slash_timestamps", &["toggle-timestamps"], false, false),
    ("thinking", "slash_thinking", &["toggle-thinking"], false, false),
    ("settings", "open_settings", &[], false, false),
    ("view-plan", "open_view_plan", &["view_plan"], false, false),
    ("dashboard", "open_status_dialog", &["status"], false, false),
    ("import", "slash_import", &["import-session"], false, false),
}

pub fn slash_commands() -> &'static [SlashCommand] {
    &SLASH_COMMANDS
}

pub fn slash_command_description(command: &str) -> &'static str {
    slash_commands()
        .iter()
        .find(|entry| entry.id == command)
        .and_then(|entry| command_metadata(entry.metadata_id))
        .map(|metadata| metadata.description)
        .unwrap_or("")
}

pub fn slash_command_aliases(command: &str) -> &'static [&'static str] {
    slash_commands()
        .iter()
        .find_map(|entry| (entry.id == command).then_some(entry.aliases))
        .unwrap_or(&[])
}

define_palette_commands! {
    ("new_session", "new_session", "new", PaletteCommandSection::Suggested),
    ("resume_session", "resume_session", "resume", PaletteCommandSection::Session),
    ("replay_session", "replay_session", "replay", PaletteCommandSection::Session),
    ("switch_model", "switch_model", "models", PaletteCommandSection::Agent),
    ("cycle_variant", "cycle_variant", "ctrl+t", PaletteCommandSection::Agent),
    ("toggles", "toggles", "toggles", PaletteCommandSection::Agent),
    ("auth", "auth", "auth", PaletteCommandSection::System),
    ("close_review_surface", "close_review_surface", "esc", PaletteCommandSection::Session),
    ("revert_workspace", "revert_workspace", "", PaletteCommandSection::Session),
    ("toggle_terminal_panel", "toggle_terminal_panel", "4", PaletteCommandSection::Session),
    ("toggle_follow", "toggle_follow", "space", PaletteCommandSection::Agent),
    ("show_thinking", "show_thinking", "", PaletteCommandSection::Agent),
    ("hide_thinking", "hide_thinking", "", PaletteCommandSection::Agent),
    ("show_timestamps", "show_timestamps", "", PaletteCommandSection::Agent),
    ("hide_timestamps", "hide_timestamps", "", PaletteCommandSection::Agent),
    ("show_tool_details", "show_tool_details", "", PaletteCommandSection::Agent),
    ("hide_tool_details", "hide_tool_details", "", PaletteCommandSection::Agent),
    ("show_generic_tool_output", "show_generic_tool_output", "", PaletteCommandSection::Agent),
    ("hide_generic_tool_output", "hide_generic_tool_output", "", PaletteCommandSection::Agent),
    ("expand_selected_turn_results", "expand_selected_turn_results", "", PaletteCommandSection::Agent),
    ("collapse_selected_turn_results", "collapse_selected_turn_results", "", PaletteCommandSection::Agent),
    ("stack_transcript_diffs", "stack_transcript_diffs", "", PaletteCommandSection::Agent),
    ("split_transcript_diffs", "split_transcript_diffs", "", PaletteCommandSection::Agent),
    ("help", "help", "?", PaletteCommandSection::System),
    ("open_settings", "open_settings", "", PaletteCommandSection::System),
    ("open_view_plan", "open_view_plan", "", PaletteCommandSection::Session),
    ("quit", "quit", "q", PaletteCommandSection::System),
    ("prompt_stash", "prompt_stash", "", PaletteCommandSection::Session),
    ("prompt_stash_pop", "prompt_stash_pop", "", PaletteCommandSection::Session),
    ("prompt_stash_list", "prompt_stash_list", "", PaletteCommandSection::Session),
    ("open_lineage_browser", "open_lineage_browser", "ctrl+x g", PaletteCommandSection::Session),
    ("session_child_first", "session_child_first", "ctrl+x ↓", PaletteCommandSection::Session),
    ("session_child_cycle", "session_child_cycle", "right", PaletteCommandSection::Session),
    ("session_child_cycle_reverse", "session_child_cycle_reverse", "left", PaletteCommandSection::Session),
    ("session_parent", "session_parent", "up", PaletteCommandSection::Session),
    ("session_background", "session_background", "ctrl+b", PaletteCommandSection::Session),
}

pub(super) fn palette_commands() -> &'static [PaletteCommand] {
    PALETTE_COMMANDS
}

#[cfg(test)]
mod tests {
    use super::slash_commands;

    #[test]
    fn slash_commands_expose_argument_metadata_and_preserve_order() {
        // Given
        let commands = slash_commands();

        // When
        let metadata = |id| {
            commands
                .iter()
                .find(|command| command.id == id)
                .map(|command| (command.takes_args, command.args_required))
        };
        let ids: Vec<_> = commands.iter().map(|command| command.id).collect();

        // Then
        assert_eq!(metadata("help"), Some((false, false)));
        assert_eq!(metadata("auth"), Some((true, false)));
        assert_eq!(metadata("rename"), Some((true, true)));
        assert!(commands
            .iter()
            .all(|command| !command.args_required || command.takes_args));
        assert_eq!(
            ids,
            [
                "new",
                "sessions",
                "fork",
                "tree",
                "clone",
                "models",
                "agents",
                "mcps",
                "toggles",
                "auth",
                "connect",
                "help",
                "feedback",
                "shell",
                "follow",
                "compact",
                "exit",
                "rename",
                "copy",
                "export",
                "timestamps",
                "thinking",
                "settings",
                "view-plan",
                "dashboard",
                "import",
            ]
        );
    }
}
