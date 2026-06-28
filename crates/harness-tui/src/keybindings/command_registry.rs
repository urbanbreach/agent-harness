//! Command and palette registries for TUI keybindings.

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CommandMetadata {
    pub(super) id: &'static str,
    pub(super) label: &'static str,
    pub(super) description: &'static str,
}

const COMMAND_METADATA: &[CommandMetadata] = &[
    CommandMetadata {
        id: "new_session",
        label: "New session",
        description: "Start a fresh live session",
    },
    CommandMetadata {
        id: "resume_session",
        label: "Continue session",
        description: "Continue a prior session when resumable",
    },
    CommandMetadata {
        id: "replay_session",
        label: "Replay session",
        description: "Replay a previous session as read-only",
    },
    CommandMetadata {
        id: "switch_model",
        label: "Switch model",
        description: "Browse available provider/model options",
    },
    CommandMetadata {
        id: "agent_cycle",
        label: "Next agent",
        description: "Cycle to the next primary agent",
    },
    CommandMetadata {
        id: "agent_cycle_reverse",
        label: "Previous agent",
        description: "Cycle to the previous primary agent",
    },
    CommandMetadata {
        id: "cycle_variant",
        label: "Cycle reasoning preset",
        description: "Cycle the configured model variant/reasoning preset",
    },
    CommandMetadata {
        id: "toggles",
        label: "Toggles",
        description: "Toggle profiles, tools, hooks, MCP, YOLO",
    },
    CommandMetadata {
        id: "auth",
        label: "Auth",
        description: "Manage provider login, logout, and auth status",
    },
    CommandMetadata {
        id: "connect",
        label: "Connect",
        description: "Connect a provider",
    },
    CommandMetadata {
        id: "close_review_surface",
        label: "Session shell",
        description: "Return to the transcript-first session shell",
    },
    CommandMetadata {
        id: "toggle_terminal_panel",
        label: "Toggle terminal panel",
        description: "Show or hide shell command output below the transcript",
    },
    CommandMetadata {
        id: "toggle_follow",
        label: "Toggle follow",
        description: "Toggle follow mode",
    },
    CommandMetadata {
        id: "show_thinking",
        label: "Show thinking",
        description: "Restore inline thinking rows in the transcript",
    },
    CommandMetadata {
        id: "hide_thinking",
        label: "Hide thinking",
        description: "Hide inline thinking rows in the transcript",
    },
    CommandMetadata {
        id: "show_timestamps",
        label: "Show timestamps",
        description: "Reveal user message timestamps in the transcript",
    },
    CommandMetadata {
        id: "hide_timestamps",
        label: "Hide timestamps",
        description: "Hide user message timestamps in the transcript",
    },
    CommandMetadata {
        id: "show_tool_details",
        label: "Show tool details",
        description: "Show completed successful tools in the transcript",
    },
    CommandMetadata {
        id: "hide_tool_details",
        label: "Hide tool details",
        description: "Hide completed successful tools in the transcript",
    },
    CommandMetadata {
        id: "show_generic_tool_output",
        label: "Show generic tool output",
        description: "Expand generic tool payload blocks in the transcript",
    },
    CommandMetadata {
        id: "hide_generic_tool_output",
        label: "Hide generic tool output",
        description: "Collapse generic tool payload blocks in the transcript",
    },
    CommandMetadata {
        id: "expand_selected_turn_results",
        label: "Expand turn results",
        description: "Expand overflow tool output in the selected turn",
    },
    CommandMetadata {
        id: "collapse_selected_turn_results",
        label: "Collapse turn results",
        description: "Collapse overflow tool output in the selected turn",
    },
    CommandMetadata {
        id: "stack_transcript_diffs",
        label: "Use stacked diffs",
        description: "Force unified stacked transcript diffs",
    },
    CommandMetadata {
        id: "split_transcript_diffs",
        label: "Use split diffs",
        description: "Allow side-by-side transcript diffs when wide",
    },
    CommandMetadata {
        id: "diff_hunk_next",
        label: "Next diff hunk",
        description: "Jump to the next transcript diff hunk",
    },
    CommandMetadata {
        id: "diff_hunk_previous",
        label: "Previous diff hunk",
        description: "Jump to the previous transcript diff hunk",
    },
    CommandMetadata {
        id: "focus_next",
        label: "Next focus",
        description: "Cycle focus forward",
    },
    CommandMetadata {
        id: "focus_prev",
        label: "Previous focus",
        description: "Cycle focus backward",
    },
    CommandMetadata {
        id: "submit_prompt",
        label: "Submit prompt",
        description: "Submit prompt",
    },
    CommandMetadata {
        id: "insert_newline",
        label: "Insert newline",
        description: "Insert newline",
    },
    CommandMetadata {
        id: "clear_prompt",
        label: "Clear prompt",
        description: "Clear prompt",
    },
    CommandMetadata {
        id: "move_down",
        label: "Move down",
        description: "Move down in list",
    },
    CommandMetadata {
        id: "move_up",
        label: "Move up",
        description: "Move up in list",
    },
    CommandMetadata {
        id: "reload",
        label: "Reload",
        description: "Reload session",
    },
    CommandMetadata {
        id: "allow_permission",
        label: "Allow permission",
        description: "Allow permission",
    },
    CommandMetadata {
        id: "deny_permission",
        label: "Deny permission",
        description: "Deny permission",
    },
    CommandMetadata {
        id: "dismiss_modal",
        label: "Reject permission",
        description: "Reject permission",
    },
    CommandMetadata {
        id: "history_up",
        label: "History up",
        description: "History up",
    },
    CommandMetadata {
        id: "history_down",
        label: "History down",
        description: "History down",
    },
    CommandMetadata {
        id: "help",
        label: "Help",
        description: "Show shortcuts and TUI controls",
    },
    CommandMetadata {
        id: "quit",
        label: "Quit",
        description: "Quit the application",
    },
    CommandMetadata {
        id: "revert_workspace",
        label: "Revert workspace",
        description: "Revert workspace to the most recent snapshot",
    },
    CommandMetadata {
        id: "slash_new",
        label: "New",
        description: "Return to the home shell",
    },
    CommandMetadata {
        id: "slash_sessions",
        label: "Sessions",
        description: "Switch session",
    },
    CommandMetadata {
        id: "slash_resume",
        label: "Resume",
        description: "Continue a saved session",
    },
    CommandMetadata {
        id: "slash_replay",
        label: "Replay",
        description: "Replay a saved session",
    },
    CommandMetadata {
        id: "slash_fork",
        label: "Fork",
        description: "Fork session",
    },
    CommandMetadata {
        id: "slash_tree",
        label: "Tree",
        description: "View the Harness session tree",
    },
    CommandMetadata {
        id: "slash_clone",
        label: "Clone",
        description: "Prepare a Harness session clone",
    },
    CommandMetadata {
        id: "slash_status",
        label: "Status",
        description: "View status",
    },
    CommandMetadata {
        id: "slash_compact",
        label: "Compact",
        description: "Write a manual context checkpoint",
    },
    CommandMetadata {
        id: "slash_rename",
        label: "Rename",
        description: "Rename the current session",
    },
    CommandMetadata {
        id: "select_char_left",
        label: "Select char left",
        description: "Extend selection one char left",
    },
    CommandMetadata {
        id: "select_char_right",
        label: "Select char right",
        description: "Extend selection one char right",
    },
    CommandMetadata {
        id: "select_word_left",
        label: "Select word left",
        description: "Extend selection one word left",
    },
    CommandMetadata {
        id: "select_word_right",
        label: "Select word right",
        description: "Extend selection one word right",
    },
    CommandMetadata {
        id: "select_line",
        label: "Select line",
        description: "Select the current line",
    },
    CommandMetadata {
        id: "select_all",
        label: "Select all",
        description: "Select the entire prompt buffer",
    },
    CommandMetadata {
        id: "move_word_left",
        label: "Move word left",
        description: "Move cursor one word left",
    },
    CommandMetadata {
        id: "move_word_right",
        label: "Move word right",
        description: "Move cursor one word right",
    },
    CommandMetadata {
        id: "move_line_start",
        label: "Move line start",
        description: "Move cursor to line start",
    },
    CommandMetadata {
        id: "move_line_end",
        label: "Move line end",
        description: "Move cursor to line end",
    },
    CommandMetadata {
        id: "move_buffer_start",
        label: "Move buffer start",
        description: "Move cursor to buffer start",
    },
    CommandMetadata {
        id: "move_buffer_end",
        label: "Move buffer end",
        description: "Move cursor to buffer end",
    },
    CommandMetadata {
        id: "delete_word_forward",
        label: "Delete word forward",
        description: "Delete the word after the cursor",
    },
    CommandMetadata {
        id: "delete_word_backward",
        label: "Delete word backward",
        description: "Delete the word before the cursor",
    },
    CommandMetadata {
        id: "delete_line",
        label: "Delete line",
        description: "Delete the current line",
    },
    CommandMetadata {
        id: "kill_to_line_start",
        label: "Kill to line start",
        description: "Delete from cursor to line start",
    },
    CommandMetadata {
        id: "kill_to_line_end",
        label: "Kill to line end",
        description: "Delete from cursor to line end",
    },
    CommandMetadata {
        id: "undo",
        label: "Undo",
        description: "Undo the last edit",
    },
    CommandMetadata {
        id: "redo",
        label: "Redo",
        description: "Redo the last undone edit",
    },
    CommandMetadata {
        id: "prompt_stash",
        label: "Stash prompt",
        description: "Stash the current composer draft to the prompt stash",
    },
    CommandMetadata {
        id: "prompt_stash_pop",
        label: "Pop stashed prompt",
        description: "Restore the most recently stashed prompt to the composer",
    },
    CommandMetadata {
        id: "prompt_stash_list",
        label: "Prompt stash list",
        description: "Open the prompt stash dialog to browse stashed prompts",
    },
    CommandMetadata {
        id: "open_lineage_browser",
        label: "Session tree",
        description: "Open the Harness session lineage browser",
    },
    CommandMetadata {
        id: "session_child_first",
        label: "First child session",
        description: "Open the first child session of the current session",
    },
    CommandMetadata {
        id: "session_child_cycle",
        label: "Next child session",
        description: "Cycle to the next sibling child session",
    },
    CommandMetadata {
        id: "session_child_cycle_reverse",
        label: "Previous child session",
        description: "Cycle to the previous sibling child session",
    },
    CommandMetadata {
        id: "session_parent",
        label: "Parent session",
        description: "Return to the parent session",
    },
    CommandMetadata {
        id: "variant_cycle",
        label: "Cycle variant",
        description: "Cycle to the next model variant",
    },
    CommandMetadata {
        id: "slash_copy",
        label: "Copy transcript",
        description: "Copy the session transcript to clipboard",
    },
    CommandMetadata {
        id: "slash_export",
        label: "Export transcript",
        description: "Export the session transcript to a file",
    },
    CommandMetadata {
        id: "slash_timestamps",
        label: "Toggle timestamps",
        description: "Toggle user message timestamps",
    },
    CommandMetadata {
        id: "slash_thinking",
        label: "Toggle thinking",
        description: "Toggle inline thinking rows",
    },
];

pub(super) fn command_metadata(id: &str) -> Option<&'static CommandMetadata> {
    COMMAND_METADATA.iter().find(|entry| entry.id == id)
}

const SLASH_COMMANDS: [SlashCommand; 21] = [
    SlashCommand {
        id: "new",
        metadata_id: "slash_new",
        aliases: &["clear"],
    },
    SlashCommand {
        id: "sessions",
        metadata_id: "slash_sessions",
        aliases: &["resume", "continue"],
    },
    SlashCommand {
        id: "fork",
        metadata_id: "slash_fork",
        aliases: &[],
    },
    SlashCommand {
        id: "tree",
        metadata_id: "slash_tree",
        aliases: &[],
    },
    SlashCommand {
        id: "clone",
        metadata_id: "slash_clone",
        aliases: &[],
    },
    SlashCommand {
        id: "models",
        metadata_id: "switch_model",
        aliases: &["mo"],
    },
    SlashCommand {
        id: "agents",
        metadata_id: "switch_model",
        aliases: &[],
    },
    SlashCommand {
        id: "mcps",
        metadata_id: "toggles",
        aliases: &[],
    },
    SlashCommand {
        id: "toggles",
        metadata_id: "toggles",
        aliases: &[],
    },
    SlashCommand {
        id: "auth",
        metadata_id: "auth",
        aliases: &["login"],
    },
    SlashCommand {
        id: "connect",
        metadata_id: "connect",
        aliases: &[],
    },
    SlashCommand {
        id: "help",
        metadata_id: "help",
        aliases: &[],
    },
    SlashCommand {
        id: "shell",
        metadata_id: "close_review_surface",
        aliases: &["session-shell"],
    },
    SlashCommand {
        id: "follow",
        metadata_id: "toggle_follow",
        aliases: &[],
    },
    SlashCommand {
        id: "compact",
        metadata_id: "slash_compact",
        aliases: &["summarize"],
    },
    SlashCommand {
        id: "exit",
        metadata_id: "quit",
        aliases: &["quit", "q"],
    },
    SlashCommand {
        id: "rename",
        metadata_id: "slash_rename",
        aliases: &[],
    },
    SlashCommand {
        id: "copy",
        metadata_id: "slash_copy",
        aliases: &[],
    },
    SlashCommand {
        id: "export",
        metadata_id: "slash_export",
        aliases: &[],
    },
    SlashCommand {
        id: "timestamps",
        metadata_id: "slash_timestamps",
        aliases: &["toggle-timestamps"],
    },
    SlashCommand {
        id: "thinking",
        metadata_id: "slash_thinking",
        aliases: &["toggle-thinking"],
    },
];

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

const PALETTE_COMMANDS: &[PaletteCommand] = &[
    PaletteCommand {
        id: "new_session",
        metadata_id: "new_session",
        shortcut: "new",
        section: PaletteCommandSection::Suggested,
    },
    PaletteCommand {
        id: "resume_session",
        metadata_id: "resume_session",
        shortcut: "resume",
        section: PaletteCommandSection::Session,
    },
    PaletteCommand {
        id: "replay_session",
        metadata_id: "replay_session",
        shortcut: "replay",
        section: PaletteCommandSection::Session,
    },
    PaletteCommand {
        id: "switch_model",
        metadata_id: "switch_model",
        shortcut: "models",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "agent_cycle",
        metadata_id: "agent_cycle",
        shortcut: "tab",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "agent_cycle_reverse",
        metadata_id: "agent_cycle_reverse",
        shortcut: "shift+tab",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "cycle_variant",
        metadata_id: "cycle_variant",
        shortcut: "ctrl+t",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "toggles",
        metadata_id: "toggles",
        shortcut: "toggles",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "auth",
        metadata_id: "auth",
        shortcut: "auth",
        section: PaletteCommandSection::System,
    },
    PaletteCommand {
        id: "close_review_surface",
        metadata_id: "close_review_surface",
        shortcut: "esc",
        section: PaletteCommandSection::Session,
    },
    PaletteCommand {
        id: "revert_workspace",
        metadata_id: "revert_workspace",
        shortcut: "",
        section: PaletteCommandSection::Session,
    },
    PaletteCommand {
        id: "toggle_terminal_panel",
        metadata_id: "toggle_terminal_panel",
        shortcut: "4",
        section: PaletteCommandSection::Session,
    },
    PaletteCommand {
        id: "toggle_follow",
        metadata_id: "toggle_follow",
        shortcut: "space",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "show_thinking",
        metadata_id: "show_thinking",
        shortcut: "",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "hide_thinking",
        metadata_id: "hide_thinking",
        shortcut: "",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "show_timestamps",
        metadata_id: "show_timestamps",
        shortcut: "",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "hide_timestamps",
        metadata_id: "hide_timestamps",
        shortcut: "",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "show_tool_details",
        metadata_id: "show_tool_details",
        shortcut: "",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "hide_tool_details",
        metadata_id: "hide_tool_details",
        shortcut: "",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "show_generic_tool_output",
        metadata_id: "show_generic_tool_output",
        shortcut: "",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "hide_generic_tool_output",
        metadata_id: "hide_generic_tool_output",
        shortcut: "",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "expand_selected_turn_results",
        metadata_id: "expand_selected_turn_results",
        shortcut: "",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "collapse_selected_turn_results",
        metadata_id: "collapse_selected_turn_results",
        shortcut: "",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "stack_transcript_diffs",
        metadata_id: "stack_transcript_diffs",
        shortcut: "",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "split_transcript_diffs",
        metadata_id: "split_transcript_diffs",
        shortcut: "",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "help",
        metadata_id: "help",
        shortcut: "?",
        section: PaletteCommandSection::System,
    },
    PaletteCommand {
        id: "quit",
        metadata_id: "quit",
        shortcut: "q",
        section: PaletteCommandSection::System,
    },
    PaletteCommand {
        id: "prompt_stash",
        metadata_id: "prompt_stash",
        shortcut: "",
        section: PaletteCommandSection::Session,
    },
    PaletteCommand {
        id: "prompt_stash_pop",
        metadata_id: "prompt_stash_pop",
        shortcut: "",
        section: PaletteCommandSection::Session,
    },
    PaletteCommand {
        id: "prompt_stash_list",
        metadata_id: "prompt_stash_list",
        shortcut: "",
        section: PaletteCommandSection::Session,
    },
    PaletteCommand {
        id: "open_lineage_browser",
        metadata_id: "open_lineage_browser",
        shortcut: "ctrl+x g",
        section: PaletteCommandSection::Session,
    },
    PaletteCommand {
        id: "session_child_first",
        metadata_id: "session_child_first",
        shortcut: "ctrl+]",
        section: PaletteCommandSection::Session,
    },
    PaletteCommand {
        id: "session_child_cycle",
        metadata_id: "session_child_cycle",
        shortcut: "]",
        section: PaletteCommandSection::Session,
    },
    PaletteCommand {
        id: "session_child_cycle_reverse",
        metadata_id: "session_child_cycle_reverse",
        shortcut: "[",
        section: PaletteCommandSection::Session,
    },
    PaletteCommand {
        id: "session_parent",
        metadata_id: "session_parent",
        shortcut: "ctrl+[",
        section: PaletteCommandSection::Session,
    },
];

pub(super) fn palette_commands() -> &'static [PaletteCommand] {
    PALETTE_COMMANDS
}
