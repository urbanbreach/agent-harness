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
        id: "palette",
        label: "Commands",
        description: "Open the command palette",
    },
    CommandMetadata {
        id: "toggle_operator_sidebar",
        label: "Operator sidebar",
        description: "Toggle the operator sidebar",
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
        id: "variant_list",
        label: "Select variant",
        description: "Choose the default or a named model variant",
    },
    CommandMetadata {
        id: "agent_list",
        label: "Select agent",
        description: "Choose the primary agent for the next turn",
    },
    CommandMetadata {
        id: "recent_model_next",
        label: "Next recent model",
        description: "Cycle to the next recently used model",
    },
    CommandMetadata {
        id: "recent_model_previous",
        label: "Previous recent model",
        description: "Cycle to the previous recently used model",
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
        id: "close_review_surface",
        label: "Session shell",
        description: "Return to the transcript-first session shell",
    },
    CommandMetadata {
        id: "open_event_log",
        label: "Event log",
        description: "Open the review event log surface",
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
        id: "messages_page_up",
        label: "Messages page up",
        description: "Page upward through the transcript",
    },
    CommandMetadata {
        id: "messages_page_down",
        label: "Messages page down",
        description: "Page downward through the transcript",
    },
    CommandMetadata {
        id: "messages_half_page_up",
        label: "Messages half page up",
        description: "Move a half page upward through the transcript",
    },
    CommandMetadata {
        id: "messages_half_page_down",
        label: "Messages half page down",
        description: "Move a half page downward through the transcript",
    },
    CommandMetadata {
        id: "messages_line_up",
        label: "Messages line up",
        description: "Move one transcript line upward",
    },
    CommandMetadata {
        id: "messages_line_down",
        label: "Messages line down",
        description: "Move one transcript line downward",
    },
    CommandMetadata {
        id: "messages_first",
        label: "First message",
        description: "Jump to the first transcript message",
    },
    CommandMetadata {
        id: "messages_last",
        label: "Last message",
        description: "Jump to the latest transcript message",
    },
    CommandMetadata {
        id: "messages_previous",
        label: "Previous message",
        description: "Jump to the previous transcript message",
    },
    CommandMetadata {
        id: "messages_next",
        label: "Next message",
        description: "Jump to the next transcript message",
    },
    CommandMetadata {
        id: "messages_last_user_message",
        label: "Last user message",
        description: "Jump to the latest user-authored transcript message",
    },
    CommandMetadata {
        id: "copy_message",
        label: "Copy message",
        description: "Copy the selected transcript message",
    },
    CommandMetadata {
        id: "copy_session",
        label: "Copy session",
        description: "Copy the visible session transcript",
    },
    CommandMetadata {
        id: "export_session",
        label: "Export session",
        description: "Export the current session as a JSON bundle",
    },
    CommandMetadata {
        id: "toggle_transcript_scrollbar",
        label: "Transcript scrollbar",
        description: "Show or hide the transcript scrollbar",
    },
    CommandMetadata {
        id: "show_last_error",
        label: "Show last error",
        description: "Inspect the last failed provider turn",
    },
    CommandMetadata {
        id: "child_sessions",
        label: "Child sessions",
        description: "Open child subagent sessions",
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
];

pub(super) fn command_metadata(id: &str) -> Option<&'static CommandMetadata> {
    COMMAND_METADATA.iter().find(|entry| entry.id == id)
}

const SLASH_COMMANDS: [SlashCommand; 17] = [
    SlashCommand {
        id: "new",
        metadata_id: "slash_new",
        aliases: &["new-session", "session"],
    },
    SlashCommand {
        id: "sessions",
        metadata_id: "slash_sessions",
        aliases: &[],
    },
    SlashCommand {
        id: "resume",
        metadata_id: "slash_resume",
        aliases: &["continue"],
    },
    SlashCommand {
        id: "replay",
        metadata_id: "slash_replay",
        aliases: &[],
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
        id: "model",
        metadata_id: "switch_model",
        aliases: &["models"],
    },
    SlashCommand {
        id: "status",
        metadata_id: "slash_status",
        aliases: &["system-status"],
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
        id: "events",
        metadata_id: "open_event_log",
        aliases: &["event-log"],
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
        aliases: &["summarize", "summary"],
    },
    SlashCommand {
        id: "exit",
        metadata_id: "quit",
        aliases: &["quit", "q"],
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
        id: "show_last_error",
        metadata_id: "show_last_error",
        shortcut: "error",
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
        shortcut: "model",
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
        id: "variant_list",
        metadata_id: "variant_list",
        shortcut: "ctrl+v",
        section: PaletteCommandSection::Agent,
    },
    PaletteCommand {
        id: "agent_list",
        metadata_id: "agent_list",
        shortcut: "ctrl+x a",
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
        id: "open_event_log",
        metadata_id: "open_event_log",
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
        id: "child_sessions",
        metadata_id: "child_sessions",
        shortcut: "child",
        section: PaletteCommandSection::Session,
    },
    PaletteCommand {
        id: "copy_message",
        metadata_id: "copy_message",
        shortcut: "ctrl+x y",
        section: PaletteCommandSection::Session,
    },
    PaletteCommand {
        id: "copy_session",
        metadata_id: "copy_session",
        shortcut: "ctrl+x shift+y",
        section: PaletteCommandSection::Session,
    },
    PaletteCommand {
        id: "export_session",
        metadata_id: "export_session",
        shortcut: "ctrl+x x",
        section: PaletteCommandSection::Session,
    },
    PaletteCommand {
        id: "toggle_transcript_scrollbar",
        metadata_id: "toggle_transcript_scrollbar",
        shortcut: "ctrl+x z",
        section: PaletteCommandSection::Session,
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
];

pub(super) fn palette_commands() -> &'static [PaletteCommand] {
    PALETTE_COMMANDS
}
