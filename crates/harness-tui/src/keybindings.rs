//! Keybinding system for the TUI.
//!
//! Maps KeyEvent to Action with support for configurable overrides.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

mod binding;
mod command_registry;
mod sequence;

use binding::format_key_binding;
pub use binding::KeyBinding;
use command_registry::command_metadata;
pub use command_registry::{
    slash_command_aliases, slash_command_description, slash_commands, PaletteCommand,
    PaletteCommandSection, SlashCommand,
};
pub use sequence::KeySequence;

/// Actions that can be triggered via keybindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    /// Quit the application
    Quit,
    /// Move focus to the next pane
    FocusNext,
    /// Move focus to the previous pane
    FocusPrev,
    /// Open the command palette
    Palette,
    NewSession,
    ResumeSession,
    ReplaySession,
    SwitchModel,
    OpenToggles,
    OpenStatusDialog,
    OpenLineageBrowser,
    OpenChildSessions,
    ShowLastError,
    CompactSession,
    /// Open/close the help tab
    Help,
    ToggleOperatorSidebar,
    ToggleTerminalPanel,
    /// Toggle follow mode
    ToggleFollow,
    /// Submit the prompt
    SubmitPrompt,
    InsertNewline,
    /// Clear the prompt
    ClearPrompt,
    /// Scroll up in the details pane
    ScrollUp,
    /// Scroll down in the details pane
    ScrollDown,
    /// Return to the transcript-first session shell
    CloseReviewSurface,
    /// Open the event log review surface
    OpenEventLog,
    /// Move down in the list
    MoveDown,
    /// Move up in the list
    MoveUp,
    Reload,
    SessionChildFirst,
    SessionChildCycle,
    SessionChildCycleReverse,
    SessionParent,
    DiffHunkNext,
    DiffHunkPrevious,
    MessagesPageUp,
    MessagesPageDown,
    MessagesHalfPageUp,
    MessagesHalfPageDown,
    MessagesLineUp,
    MessagesLineDown,
    MessagesFirst,
    MessagesLast,
    MessagesPrevious,
    MessagesNext,
    MessagesLastUserMessage,
    CopyMessage,
    CopySession,
    ExportSession,
    ToggleTranscriptScrollbar,
    AgentCycle,
    AgentCycleReverse,
    VariantCycle,
    OpenVariantDialog,
    OpenAgentDialog,
    RecentModelNext,
    RecentModelPrevious,
    AllowPermission,
    /// Deny permission in modal
    DenyPermission,
    /// Dismiss modal
    DismissModal,
    /// Navigate history up (prompt)
    HistoryUp,
    /// Navigate history down (prompt)
    HistoryDown,
    /// Move cursor left (prompt)
    CursorLeft,
    /// Move cursor right (prompt)
    CursorRight,
    /// Backspace (prompt)
    Backspace,
    /// Delete (prompt)
    Delete,
    InputSelectLeft,
    InputSelectRight,
    InputSelectUp,
    InputSelectDown,
    InputLineStart,
    InputLineEnd,
    InputSelectLineStart,
    InputSelectLineEnd,
    InputWordBackward,
    InputWordForward,
    InputSelectWordBackward,
    InputSelectWordForward,
    InputDeleteWordBackward,
    InputDeleteWordForward,
    InputDeleteLine,
    InputDeleteToLineStart,
    InputDeleteToLineEnd,
    InputSelectAll,
    InputUndo,
    InputRedo,
    PromptStash,
    PromptStashPop,
    PromptStashList,
    PromptStashDeleteSelected,
    QueuedPrompts,
    /// Character input (prompt)
    Char(char),
}

impl Action {
    fn metadata_id(self) -> Option<&'static str> {
        match self {
            Action::Quit => Some("quit"),
            Action::FocusNext => Some("focus_next"),
            Action::FocusPrev => Some("focus_prev"),
            Action::Palette => Some("palette"),
            Action::NewSession => Some("new_session"),
            Action::ResumeSession => Some("resume_session"),
            Action::ReplaySession => Some("replay_session"),
            Action::SwitchModel => Some("switch_model"),
            Action::OpenToggles => Some("toggles"),
            Action::OpenStatusDialog => Some("slash_status"),
            Action::OpenLineageBrowser => Some("slash_tree"),
            Action::OpenChildSessions => Some("child_sessions"),
            Action::ShowLastError => Some("show_last_error"),
            Action::CompactSession => Some("slash_compact"),
            Action::Help => Some("help"),
            Action::ToggleOperatorSidebar => Some("toggle_operator_sidebar"),
            Action::ToggleTerminalPanel => Some("toggle_terminal_panel"),
            Action::ToggleFollow => Some("toggle_follow"),
            Action::SubmitPrompt => Some("submit_prompt"),
            Action::InsertNewline => Some("insert_newline"),
            Action::ClearPrompt => Some("clear_prompt"),
            Action::ScrollUp => None,
            Action::ScrollDown => None,
            Action::CloseReviewSurface => Some("close_review_surface"),
            Action::OpenEventLog => Some("open_event_log"),
            Action::MoveDown => Some("move_down"),
            Action::MoveUp => Some("move_up"),
            Action::Reload => Some("reload"),
            Action::SessionChildFirst => None,
            Action::SessionChildCycle => None,
            Action::SessionChildCycleReverse => None,
            Action::SessionParent => None,
            Action::DiffHunkNext => Some("diff_hunk_next"),
            Action::DiffHunkPrevious => Some("diff_hunk_previous"),
            Action::MessagesPageUp => Some("messages_page_up"),
            Action::MessagesPageDown => Some("messages_page_down"),
            Action::MessagesHalfPageUp => Some("messages_half_page_up"),
            Action::MessagesHalfPageDown => Some("messages_half_page_down"),
            Action::MessagesLineUp => Some("messages_line_up"),
            Action::MessagesLineDown => Some("messages_line_down"),
            Action::MessagesFirst => Some("messages_first"),
            Action::MessagesLast => Some("messages_last"),
            Action::MessagesPrevious => Some("messages_previous"),
            Action::MessagesNext => Some("messages_next"),
            Action::MessagesLastUserMessage => Some("messages_last_user_message"),
            Action::CopyMessage => Some("copy_message"),
            Action::CopySession => Some("copy_session"),
            Action::ExportSession => Some("export_session"),
            Action::ToggleTranscriptScrollbar => Some("toggle_transcript_scrollbar"),
            Action::AgentCycle => Some("agent_cycle"),
            Action::AgentCycleReverse => Some("agent_cycle_reverse"),
            Action::VariantCycle => Some("cycle_variant"),
            Action::OpenVariantDialog => Some("variant_list"),
            Action::OpenAgentDialog => Some("agent_list"),
            Action::RecentModelNext => Some("recent_model_next"),
            Action::RecentModelPrevious => Some("recent_model_previous"),
            Action::AllowPermission => Some("allow_permission"),
            Action::DenyPermission => Some("deny_permission"),
            Action::DismissModal => Some("dismiss_modal"),
            Action::HistoryUp => Some("history_up"),
            Action::HistoryDown => Some("history_down"),
            Action::CursorLeft => None,
            Action::CursorRight => None,
            Action::Backspace => None,
            Action::Delete => None,
            Action::InputSelectLeft
            | Action::InputSelectRight
            | Action::InputSelectUp
            | Action::InputSelectDown
            | Action::InputLineStart
            | Action::InputLineEnd
            | Action::InputSelectLineStart
            | Action::InputSelectLineEnd
            | Action::InputWordBackward
            | Action::InputWordForward
            | Action::InputSelectWordBackward
            | Action::InputSelectWordForward
            | Action::InputDeleteWordBackward
            | Action::InputDeleteWordForward
            | Action::InputDeleteLine
            | Action::InputDeleteToLineStart
            | Action::InputDeleteToLineEnd
            | Action::InputSelectAll
            | Action::InputUndo
            | Action::InputRedo
            | Action::PromptStash
            | Action::PromptStashPop
            | Action::PromptStashList
            | Action::PromptStashDeleteSelected
            | Action::QueuedPrompts => None,
            Action::Char(_) => None,
        }
    }

    pub fn metadata_label(self) -> &'static str {
        self.metadata_id()
            .and_then(command_metadata)
            .map(|metadata| metadata.label)
            .unwrap_or("")
    }

    pub fn metadata_description(self) -> &'static str {
        self.metadata_id()
            .and_then(command_metadata)
            .map(|metadata| metadata.description)
            .unwrap_or("")
    }

    fn grouped_palette_commands() -> &'static [PaletteCommand] {
        command_registry::palette_commands()
    }

    /// Convert action to its string identifier used in config.
    pub fn as_str(&self) -> &'static str {
        match self {
            Action::Quit => "quit",
            Action::FocusNext => "focus_next",
            Action::FocusPrev => "focus_prev",
            Action::Palette => "palette",
            Action::NewSession => "new_session",
            Action::ResumeSession => "resume_session",
            Action::ReplaySession => "replay_session",
            Action::SwitchModel => "switch_model",
            Action::OpenToggles => "open_toggles",
            Action::OpenStatusDialog => "open_status_dialog",
            Action::OpenLineageBrowser => "open_lineage_browser",
            Action::OpenChildSessions => "open_child_sessions",
            Action::ShowLastError => "show_last_error",
            Action::CompactSession => "compact_session",
            Action::Help => "help",
            Action::ToggleOperatorSidebar => "toggle_operator_sidebar",
            Action::ToggleTerminalPanel => "toggle_terminal_panel",
            Action::ToggleFollow => "toggle_follow",
            Action::SubmitPrompt => "submit_prompt",
            Action::InsertNewline => "insert_newline",
            Action::ClearPrompt => "clear_prompt",
            Action::ScrollUp => "scroll_up",
            Action::ScrollDown => "scroll_down",
            Action::CloseReviewSurface => "close_review_surface",
            Action::OpenEventLog => "open_event_log",
            Action::MoveDown => "move_down",
            Action::MoveUp => "move_up",
            Action::Reload => "reload",
            Action::SessionChildFirst => "session_child_first",
            Action::SessionChildCycle => "session_child_cycle",
            Action::SessionChildCycleReverse => "session_child_cycle_reverse",
            Action::SessionParent => "session_parent",
            Action::DiffHunkNext => "diff_hunk_next",
            Action::DiffHunkPrevious => "diff_hunk_previous",
            Action::MessagesPageUp => "messages_page_up",
            Action::MessagesPageDown => "messages_page_down",
            Action::MessagesHalfPageUp => "messages_half_page_up",
            Action::MessagesHalfPageDown => "messages_half_page_down",
            Action::MessagesLineUp => "messages_line_up",
            Action::MessagesLineDown => "messages_line_down",
            Action::MessagesFirst => "messages_first",
            Action::MessagesLast => "messages_last",
            Action::MessagesPrevious => "messages_previous",
            Action::MessagesNext => "messages_next",
            Action::MessagesLastUserMessage => "messages_last_user_message",
            Action::CopyMessage => "copy_message",
            Action::CopySession => "copy_session",
            Action::ExportSession => "export_session",
            Action::ToggleTranscriptScrollbar => "toggle_transcript_scrollbar",
            Action::AgentCycle => "agent_cycle",
            Action::AgentCycleReverse => "agent_cycle_reverse",
            Action::VariantCycle => "variant_cycle",
            Action::OpenVariantDialog => "variant_list",
            Action::OpenAgentDialog => "agent_list",
            Action::RecentModelNext => "recent_model_next",
            Action::RecentModelPrevious => "recent_model_previous",
            Action::AllowPermission => "allow_permission",
            Action::DenyPermission => "deny_permission",
            Action::DismissModal => "dismiss_modal",
            Action::HistoryUp => "history_up",
            Action::HistoryDown => "history_down",
            Action::CursorLeft => "cursor_left",
            Action::CursorRight => "cursor_right",
            Action::Backspace => "backspace",
            Action::Delete => "delete",
            Action::InputSelectLeft => "input_select_left",
            Action::InputSelectRight => "input_select_right",
            Action::InputSelectUp => "input_select_up",
            Action::InputSelectDown => "input_select_down",
            Action::InputLineStart => "input_line_start",
            Action::InputLineEnd => "input_line_end",
            Action::InputSelectLineStart => "input_select_line_start",
            Action::InputSelectLineEnd => "input_select_line_end",
            Action::InputWordBackward => "input_word_backward",
            Action::InputWordForward => "input_word_forward",
            Action::InputSelectWordBackward => "input_select_word_backward",
            Action::InputSelectWordForward => "input_select_word_forward",
            Action::InputDeleteWordBackward => "input_delete_word_backward",
            Action::InputDeleteWordForward => "input_delete_word_forward",
            Action::InputDeleteLine => "input_delete_line",
            Action::InputDeleteToLineStart => "input_delete_to_line_start",
            Action::InputDeleteToLineEnd => "input_delete_to_line_end",
            Action::InputSelectAll => "input_select_all",
            Action::InputUndo => "input_undo",
            Action::InputRedo => "input_redo",
            Action::PromptStash => "prompt_stash",
            Action::PromptStashPop => "prompt_stash_pop",
            Action::PromptStashList => "prompt_stash_list",
            Action::PromptStashDeleteSelected => "prompt_stash_delete_selected",
            Action::QueuedPrompts => "queued_prompts",
            Action::Char(_) => "char",
        }
    }

    /// Get the list of all palette-executable actions.
    pub fn palette_commands() -> &'static [PaletteCommand] {
        Self::grouped_palette_commands()
    }

    pub fn palette_command_label(command: &str) -> &'static str {
        Self::palette_command(command)
            .and_then(|palette_command| command_metadata(palette_command.metadata_id))
            .map(|metadata| metadata.label)
            .unwrap_or("")
    }

    pub fn palette_command_description(command: &str) -> &'static str {
        Self::palette_command(command)
            .and_then(|palette_command| command_metadata(palette_command.metadata_id))
            .map(|metadata| metadata.description)
            .unwrap_or("")
    }

    pub fn palette_command_shortcut(command: &str) -> &'static str {
        Self::palette_command(command)
            .map(|palette_command| palette_command.shortcut)
            .unwrap_or("")
    }

    pub fn from_palette_command(command: &str) -> Option<Self> {
        match command {
            "new_session" => Some(Action::NewSession),
            "resume_session" => Some(Action::ResumeSession),
            "replay_session" => Some(Action::ReplaySession),
            "switch_model" => Some(Action::SwitchModel),
            "toggles" => Some(Action::OpenToggles),
            "cycle_variant" => Some(Action::VariantCycle),
            "variant_list" => Some(Action::OpenVariantDialog),
            "agent_list" => Some(Action::OpenAgentDialog),
            "recent_model_next" => Some(Action::RecentModelNext),
            "recent_model_previous" => Some(Action::RecentModelPrevious),
            "close_review_surface" => Some(Action::CloseReviewSurface),
            "open_event_log" => Some(Action::OpenEventLog),
            "toggle_terminal_panel" => Some(Action::ToggleTerminalPanel),
            "toggle_follow" => Some(Action::ToggleFollow),
            "child_sessions" => Some(Action::OpenChildSessions),
            "show_last_error" => Some(Action::ShowLastError),
            "copy_message" => Some(Action::CopyMessage),
            "copy_session" => Some(Action::CopySession),
            "export_session" => Some(Action::ExportSession),
            "toggle_transcript_scrollbar" => Some(Action::ToggleTranscriptScrollbar),
            "help" => Some(Action::Help),
            "quit" => Some(Action::Quit),
            _ => None,
        }
    }

    pub fn palette_command_section(command: &str) -> Option<PaletteCommandSection> {
        Self::palette_command(command).map(|palette_command| palette_command.section)
    }

    fn palette_command(command: &str) -> Option<&'static PaletteCommand> {
        Self::grouped_palette_commands()
            .iter()
            .find(|palette_command| palette_command.id == command)
    }

    pub fn grouped_palette_commands_for_overlay() -> &'static [PaletteCommand] {
        Self::grouped_palette_commands()
    }
}

impl FromStr for Action {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "quit" => Ok(Action::Quit),
            "focus_next" => Ok(Action::FocusNext),
            "focus_prev" => Ok(Action::FocusPrev),
            "palette" => Ok(Action::Palette),
            "new_session" => Ok(Action::NewSession),
            "resume_session" => Ok(Action::ResumeSession),
            "replay_session" => Ok(Action::ReplaySession),
            "switch_model" => Ok(Action::SwitchModel),
            "open_toggles" | "toggles" => Ok(Action::OpenToggles),
            "open_status_dialog" | "status" => Ok(Action::OpenStatusDialog),
            "open_lineage_browser" | "tree" => Ok(Action::OpenLineageBrowser),
            "open_child_sessions" | "child_sessions" => Ok(Action::OpenChildSessions),
            "show_last_error" => Ok(Action::ShowLastError),
            "compact_session" | "compact" => Ok(Action::CompactSession),
            "help" => Ok(Action::Help),
            "toggle_operator_sidebar" => Ok(Action::ToggleOperatorSidebar),
            "toggle_terminal_panel" => Ok(Action::ToggleTerminalPanel),
            "toggle_follow" => Ok(Action::ToggleFollow),
            "submit_prompt" => Ok(Action::SubmitPrompt),
            "insert_newline" => Ok(Action::InsertNewline),
            "clear_prompt" => Ok(Action::ClearPrompt),
            "scroll_up" => Ok(Action::ScrollUp),
            "scroll_down" => Ok(Action::ScrollDown),
            "close_review_surface" => Ok(Action::CloseReviewSurface),
            "open_event_log" => Ok(Action::OpenEventLog),
            "move_down" => Ok(Action::MoveDown),
            "move_up" => Ok(Action::MoveUp),
            "reload" => Ok(Action::Reload),
            "session_child_first" => Ok(Action::SessionChildFirst),
            "session_child_cycle" => Ok(Action::SessionChildCycle),
            "session_child_cycle_reverse" => Ok(Action::SessionChildCycleReverse),
            "session_parent" => Ok(Action::SessionParent),
            "diff_hunk_next" => Ok(Action::DiffHunkNext),
            "diff_hunk_previous" => Ok(Action::DiffHunkPrevious),
            "messages_page_up" => Ok(Action::MessagesPageUp),
            "messages_page_down" => Ok(Action::MessagesPageDown),
            "messages_half_page_up" => Ok(Action::MessagesHalfPageUp),
            "messages_half_page_down" => Ok(Action::MessagesHalfPageDown),
            "messages_line_up" => Ok(Action::MessagesLineUp),
            "messages_line_down" => Ok(Action::MessagesLineDown),
            "messages_first" => Ok(Action::MessagesFirst),
            "messages_last" => Ok(Action::MessagesLast),
            "messages_previous" => Ok(Action::MessagesPrevious),
            "messages_next" => Ok(Action::MessagesNext),
            "messages_last_user_message" => Ok(Action::MessagesLastUserMessage),
            "copy_message" => Ok(Action::CopyMessage),
            "copy_session" => Ok(Action::CopySession),
            "export_session" => Ok(Action::ExportSession),
            "toggle_transcript_scrollbar" => Ok(Action::ToggleTranscriptScrollbar),
            "agent_cycle" => Ok(Action::AgentCycle),
            "agent_cycle_reverse" => Ok(Action::AgentCycleReverse),
            "variant_cycle" => Ok(Action::VariantCycle),
            "variant_list" => Ok(Action::OpenVariantDialog),
            "agent_list" => Ok(Action::OpenAgentDialog),
            "recent_model_next" => Ok(Action::RecentModelNext),
            "recent_model_previous" => Ok(Action::RecentModelPrevious),
            "allow_permission" => Ok(Action::AllowPermission),
            "deny_permission" => Ok(Action::DenyPermission),
            "dismiss_modal" => Ok(Action::DismissModal),
            "history_up" => Ok(Action::HistoryUp),
            "history_down" => Ok(Action::HistoryDown),
            "cursor_left" => Ok(Action::CursorLeft),
            "cursor_right" => Ok(Action::CursorRight),
            "backspace" => Ok(Action::Backspace),
            "delete" => Ok(Action::Delete),
            "input_select_left" => Ok(Action::InputSelectLeft),
            "input_select_right" => Ok(Action::InputSelectRight),
            "input_select_up" => Ok(Action::InputSelectUp),
            "input_select_down" => Ok(Action::InputSelectDown),
            "input_line_start" => Ok(Action::InputLineStart),
            "input_line_end" => Ok(Action::InputLineEnd),
            "input_select_line_start" => Ok(Action::InputSelectLineStart),
            "input_select_line_end" => Ok(Action::InputSelectLineEnd),
            "input_word_backward" => Ok(Action::InputWordBackward),
            "input_word_forward" => Ok(Action::InputWordForward),
            "input_select_word_backward" => Ok(Action::InputSelectWordBackward),
            "input_select_word_forward" => Ok(Action::InputSelectWordForward),
            "input_delete_word_backward" => Ok(Action::InputDeleteWordBackward),
            "input_delete_word_forward" => Ok(Action::InputDeleteWordForward),
            "input_delete_line" => Ok(Action::InputDeleteLine),
            "input_delete_to_line_start" => Ok(Action::InputDeleteToLineStart),
            "input_delete_to_line_end" => Ok(Action::InputDeleteToLineEnd),
            "input_select_all" => Ok(Action::InputSelectAll),
            "input_undo" => Ok(Action::InputUndo),
            "input_redo" => Ok(Action::InputRedo),
            "prompt_stash" => Ok(Action::PromptStash),
            "prompt_stash_pop" => Ok(Action::PromptStashPop),
            "prompt_stash_list" => Ok(Action::PromptStashList),
            "prompt_stash_delete_selected" => Ok(Action::PromptStashDeleteSelected),
            "queued_prompts" => Ok(Action::QueuedPrompts),
            _ => Err(format!("unknown action: {s}")),
        }
    }
}

/// Manages key bindings mapping keys to actions.
#[derive(Debug, Clone)]
pub struct KeyMap {
    bindings: HashMap<KeySequence, Action>,
    reverse: HashMap<Action, Vec<KeySequence>>,
    leader: KeyBinding,
}

impl Default for KeyMap {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl KeyMap {
    pub fn with_defaults() -> Self {
        let mut keymap = Self {
            bindings: HashMap::new(),
            reverse: HashMap::new(),
            leader: KeyBinding::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        };

        keymap.bind(
            KeyBinding::new(KeyCode::Char('j'), KeyModifiers::NONE),
            Action::MoveDown,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Down, KeyModifiers::NONE),
            Action::MoveDown,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('k'), KeyModifiers::NONE),
            Action::MoveUp,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Up, KeyModifiers::NONE),
            Action::MoveUp,
        );

        keymap.bind(
            KeyBinding::new(KeyCode::Tab, KeyModifiers::NONE),
            Action::AgentCycle,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::BackTab, KeyModifiers::NONE),
            Action::AgentCycleReverse,
        );
        keymap.bind_leader(
            KeyBinding::new(KeyCode::Char('a'), KeyModifiers::NONE),
            Action::OpenAgentDialog,
        );

        // Focus cycling remains available on explicit control chords.
        keymap.bind(
            KeyBinding::new(KeyCode::Tab, KeyModifiers::CONTROL),
            Action::FocusNext,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::BackTab, KeyModifiers::CONTROL),
            Action::FocusPrev,
        );

        keymap.bind_leader(
            KeyBinding::new(KeyCode::Char('n'), KeyModifiers::NONE),
            Action::NewSession,
        );
        keymap.bind_leader(
            KeyBinding::new(KeyCode::Char('l'), KeyModifiers::NONE),
            Action::ResumeSession,
        );
        keymap.bind_leader(
            KeyBinding::new(KeyCode::Char('m'), KeyModifiers::NONE),
            Action::SwitchModel,
        );
        keymap.bind_leader(
            KeyBinding::new(KeyCode::Char('s'), KeyModifiers::NONE),
            Action::OpenStatusDialog,
        );
        keymap.bind_leader(
            KeyBinding::new(KeyCode::Char('b'), KeyModifiers::NONE),
            Action::ToggleOperatorSidebar,
        );
        keymap.bind_leader(
            KeyBinding::new(KeyCode::Char('g'), KeyModifiers::NONE),
            Action::OpenLineageBrowser,
        );
        keymap.bind_leader(
            KeyBinding::new(KeyCode::Char('c'), KeyModifiers::NONE),
            Action::CompactSession,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
            Action::Palette,
        );
        keymap.bind_leader(
            KeyBinding::new(KeyCode::Char('p'), KeyModifiers::NONE),
            Action::Palette,
        );

        keymap.bind(
            KeyBinding::new(KeyCode::Char('1'), KeyModifiers::NONE),
            Action::CloseReviewSurface,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('2'), KeyModifiers::NONE),
            Action::ToggleOperatorSidebar,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('3'), KeyModifiers::NONE),
            Action::OpenEventLog,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('4'), KeyModifiers::NONE),
            Action::ToggleTerminalPanel,
        );

        keymap.bind(
            KeyBinding::new(KeyCode::Char('i'), KeyModifiers::NONE),
            Action::ToggleOperatorSidebar,
        );

        keymap.bind(
            KeyBinding::new(KeyCode::Char('q'), KeyModifiers::NONE),
            Action::Quit,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char(' '), KeyModifiers::NONE),
            Action::ToggleFollow,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('r'), KeyModifiers::NONE),
            Action::Reload,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char(']'), KeyModifiers::CONTROL),
            Action::SessionChildFirst,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char(']'), KeyModifiers::NONE),
            Action::SessionChildCycle,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('['), KeyModifiers::NONE),
            Action::SessionChildCycleReverse,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('['), KeyModifiers::CONTROL),
            Action::SessionParent,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('n'), KeyModifiers::ALT),
            Action::DiffHunkNext,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('p'), KeyModifiers::ALT),
            Action::DiffHunkPrevious,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char(']'), KeyModifiers::ALT),
            Action::DiffHunkNext,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('['), KeyModifiers::ALT),
            Action::DiffHunkPrevious,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('b'), KeyModifiers::CONTROL),
            Action::MessagesPageUp,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
            Action::MessagesPageDown,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::PageUp, KeyModifiers::ALT),
            Action::MessagesHalfPageUp,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::PageDown, KeyModifiers::ALT),
            Action::MessagesHalfPageDown,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('k'), KeyModifiers::ALT),
            Action::MessagesLineUp,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('j'), KeyModifiers::ALT),
            Action::MessagesLineDown,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('g'), KeyModifiers::CONTROL),
            Action::MessagesFirst,
        );
        keymap.bind(
            KeyBinding::new(
                KeyCode::Char('g'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
            Action::MessagesLast,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Up, KeyModifiers::ALT),
            Action::MessagesPrevious,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Down, KeyModifiers::ALT),
            Action::MessagesNext,
        );
        keymap.bind(
            KeyBinding::new(
                KeyCode::Char('u'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
            Action::MessagesLastUserMessage,
        );
        keymap.bind_leader(
            KeyBinding::new(KeyCode::Char('y'), KeyModifiers::NONE),
            Action::CopyMessage,
        );
        keymap.bind_leader(
            KeyBinding::new(KeyCode::Char('y'), KeyModifiers::SHIFT),
            Action::CopySession,
        );
        keymap.bind_leader(
            KeyBinding::new(KeyCode::Char('x'), KeyModifiers::NONE),
            Action::ExportSession,
        );
        keymap.bind_leader(
            KeyBinding::new(KeyCode::Char('z'), KeyModifiers::NONE),
            Action::ToggleTranscriptScrollbar,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('?'), KeyModifiers::NONE),
            Action::Help,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('h'), KeyModifiers::NONE),
            Action::Help,
        );

        keymap.bind(
            KeyBinding::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
            Action::VariantCycle,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('v'), KeyModifiers::CONTROL),
            Action::OpenVariantDialog,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::F(2), KeyModifiers::NONE),
            Action::RecentModelNext,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::F(2), KeyModifiers::SHIFT),
            Action::RecentModelPrevious,
        );

        keymap.bind(
            KeyBinding::new(KeyCode::Up, KeyModifiers::NONE),
            Action::HistoryUp,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Down, KeyModifiers::NONE),
            Action::HistoryDown,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Left, KeyModifiers::NONE),
            Action::CursorLeft,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Right, KeyModifiers::NONE),
            Action::CursorRight,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Enter, KeyModifiers::NONE),
            Action::SubmitPrompt,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Enter, KeyModifiers::SHIFT),
            Action::InsertNewline,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
            Action::InsertNewline,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Enter, KeyModifiers::CONTROL),
            Action::InsertNewline,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Enter, KeyModifiers::ALT),
            Action::InsertNewline,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Esc, KeyModifiers::NONE),
            Action::ClearPrompt,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Backspace, KeyModifiers::NONE),
            Action::Backspace,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Delete, KeyModifiers::NONE),
            Action::Delete,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Left, KeyModifiers::SHIFT),
            Action::InputSelectLeft,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Right, KeyModifiers::SHIFT),
            Action::InputSelectRight,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Up, KeyModifiers::SHIFT),
            Action::InputSelectUp,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Down, KeyModifiers::SHIFT),
            Action::InputSelectDown,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            Action::InputLineStart,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Home, KeyModifiers::NONE),
            Action::InputLineStart,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
            Action::InputLineEnd,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::End, KeyModifiers::NONE),
            Action::InputLineEnd,
        );
        keymap.bind(
            KeyBinding::new(
                KeyCode::Char('a'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
            Action::InputSelectLineStart,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Home, KeyModifiers::SHIFT),
            Action::InputSelectLineStart,
        );
        keymap.bind(
            KeyBinding::new(
                KeyCode::Char('e'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
            Action::InputSelectLineEnd,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::End, KeyModifiers::SHIFT),
            Action::InputSelectLineEnd,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Left, KeyModifiers::ALT),
            Action::InputWordBackward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('b'), KeyModifiers::ALT),
            Action::InputWordBackward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Left, KeyModifiers::CONTROL),
            Action::InputWordBackward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Right, KeyModifiers::ALT),
            Action::InputWordForward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('f'), KeyModifiers::ALT),
            Action::InputWordForward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Right, KeyModifiers::CONTROL),
            Action::InputWordForward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Left, KeyModifiers::ALT | KeyModifiers::SHIFT),
            Action::InputSelectWordBackward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('b'), KeyModifiers::ALT | KeyModifiers::SHIFT),
            Action::InputSelectWordBackward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Left, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
            Action::InputSelectWordBackward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Right, KeyModifiers::ALT | KeyModifiers::SHIFT),
            Action::InputSelectWordForward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('f'), KeyModifiers::ALT | KeyModifiers::SHIFT),
            Action::InputSelectWordForward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Right, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
            Action::InputSelectWordForward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Backspace, KeyModifiers::ALT),
            Action::InputDeleteWordBackward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Backspace, KeyModifiers::CONTROL),
            Action::InputDeleteWordBackward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
            Action::InputDeleteWordBackward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Delete, KeyModifiers::ALT),
            Action::InputDeleteWordForward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Delete, KeyModifiers::CONTROL),
            Action::InputDeleteWordForward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('d'), KeyModifiers::ALT),
            Action::InputDeleteWordForward,
        );
        keymap.bind(
            KeyBinding::new(
                KeyCode::Char('d'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
            Action::InputDeleteLine,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
            Action::InputDeleteToLineStart,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
            Action::InputDeleteToLineEnd,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('-'), KeyModifiers::CONTROL),
            Action::InputUndo,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('.'), KeyModifiers::CONTROL),
            Action::InputRedo,
        );

        keymap.bind(
            KeyBinding::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
            Action::AllowPermission,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
            Action::DenyPermission,
        );
        keymap.bind_contextual(
            KeyBinding::new(KeyCode::Esc, KeyModifiers::NONE),
            Action::DismissModal,
        );

        keymap
    }

    pub fn apply_overrides(&mut self, overrides: &BTreeMap<String, String>) {
        let _ = self.try_apply_overrides(overrides);
    }

    pub fn try_apply_overrides(
        &mut self,
        overrides: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        let mut next = self.clone();
        if let Some(leader) = overrides.get("leader") {
            next.leader = leader
                .parse()
                .map_err(|err| format!("invalid leader binding `{leader}`: {err}"))?;
        }

        for (action_str, key_str) in overrides {
            if action_str == "leader" {
                continue;
            }

            let action = Action::from_str(action_str)?;
            let sequences = parse_override_sequences(action_str, key_str)?;
            next.clear_action(action);
            for sequence in sequences {
                next.bind_sequence(sequence, action);
            }
        }

        *self = next;
        Ok(())
    }

    fn bind(&mut self, binding: KeyBinding, action: Action) {
        self.bind_sequence(KeySequence::single(binding), action);
    }

    fn bind_leader(&mut self, binding: KeyBinding, action: Action) {
        self.bind_sequence(KeySequence::leader(binding), action);
    }

    fn bind_contextual(&mut self, binding: KeyBinding, action: Action) {
        let sequence = KeySequence::single(binding);
        let bindings = self.reverse.entry(action).or_default();
        bindings.retain(|binding| *binding != sequence);
        bindings.push(sequence);
    }

    fn bind_sequence(&mut self, sequence: KeySequence, action: Action) {
        if let Some(old_action) = self.bindings.insert(sequence, action) {
            if let Some(bindings) = self.reverse.get_mut(&old_action) {
                bindings.retain(|binding| *binding != sequence);
            }
        }
        let bindings = self.reverse.entry(action).or_default();
        bindings.retain(|binding| *binding != sequence);
        bindings.push(sequence);
    }

    fn clear_action(&mut self, action: Action) {
        if let Some(existing_bindings) = self.reverse.remove(&action) {
            for existing in existing_bindings {
                self.bindings.remove(&existing);
            }
        }
    }

    pub fn is_leader(&self, event: &KeyEvent) -> bool {
        self.leader.matches(event)
    }

    pub fn leader_binding_label(&self) -> String {
        format_key_binding(&self.leader)
    }

    pub fn get_leader_action(&self, event: &KeyEvent) -> Option<Action> {
        let binding = KeyBinding::new(event.code, event.modifiers);
        self.bindings.get(&KeySequence::leader(binding)).copied()
    }

    pub fn get_action(&self, event: &KeyEvent) -> Option<Action> {
        let binding = KeyBinding::new(event.code, event.modifiers);
        self.bindings
            .get(&KeySequence::single(binding))
            .copied()
            .or_else(|| {
                (event.code == KeyCode::Char('\u{1d}') && event.modifiers == KeyModifiers::NONE)
                    .then(|| {
                        self.bindings
                            .get(&KeySequence::single(KeyBinding::new(
                                KeyCode::Char(']'),
                                KeyModifiers::CONTROL,
                            )))
                            .copied()
                    })
                    .flatten()
            })
    }

    pub fn get_bindings(&self, action: Action) -> Vec<&KeySequence> {
        self.reverse
            .get(&action)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    pub fn get_binding_str(&self, action: Action) -> String {
        self.get_binding_strs(action)
            .into_iter()
            .next()
            .unwrap_or_else(|| "-".to_string())
    }

    pub fn get_binding_strs(&self, action: Action) -> Vec<String> {
        self.get_bindings(action)
            .into_iter()
            .map(|binding| binding.display(self.leader))
            .collect()
    }

    pub fn palette_command_shortcut(&self, command: &str) -> String {
        let Some(action) = Action::from_palette_command(command) else {
            return Action::palette_command_shortcut(command).to_string();
        };
        let use_binding_display = matches!(
            action,
            Action::CopyMessage
                | Action::CopySession
                | Action::ExportSession
                | Action::ToggleTranscriptScrollbar
        );
        let labels = self
            .get_bindings(action)
            .into_iter()
            .map(|binding| {
                if use_binding_display {
                    binding.display(self.leader)
                } else {
                    binding.harness_display(self.leader)
                }
            })
            .collect::<Vec<_>>();
        if labels.is_empty() {
            Action::palette_command_shortcut(command).to_string()
        } else {
            labels.join(" / ")
        }
    }

    pub fn get_binding_label(&self, action: Action, label: &str) -> String {
        format!("{} {label}", self.get_binding_str(action))
    }

    pub fn all_bindings(&self) -> Vec<(String, &Action)> {
        let mut all: Vec<(String, &Action)> = self
            .bindings
            .iter()
            .map(|(sequence, action)| (sequence.display(self.leader), action))
            .collect();
        all.sort_by(|(left_key, left_action), (right_key, right_action)| {
            left_key
                .cmp(right_key)
                .then_with(|| left_action.as_str().cmp(right_action.as_str()))
        });
        all
    }
}

fn parse_override_sequences(action: &str, value: &str) -> Result<Vec<KeySequence>, String> {
    let sequences = value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            KeySequence::parse_config(part)
                .map_err(|err| format!("invalid binding `{part}` for {action}: {err}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if sequences.is_empty() {
        Err(format!(
            "invalid binding for {action}: expected at least one key"
        ))
    } else {
        Ok(sequences)
    }
}

#[cfg(test)]
mod leader_tests;
#[cfg(test)]
mod tests;
