//! Keybinding system for the TUI.
//!
//! Maps KeyEvent to Action with support for configurable overrides.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

mod command_registry;
pub mod palette_model;
pub mod parity_matrix;

use command_registry::command_metadata;
pub use command_registry::{
    slash_command_aliases, slash_command_description, slash_commands, PaletteCommand,
    PaletteCommandSection, SlashCommand,
};

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
    AgentCycle,
    AgentCycleReverse,
    VariantCycle,
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
    /// Character input (prompt)
    Char(char),
    SelectCharLeft,
    SelectCharRight,
    SelectWordLeft,
    SelectWordRight,
    SelectLine,
    SelectAll,
    MoveWordLeft,
    MoveWordRight,
    MoveLineStart,
    MoveLineEnd,
    MoveBufferStart,
    MoveBufferEnd,
    DeleteWordForward,
    DeleteWordBackward,
    DeleteLine,
    KillToLineStart,
    KillToLineEnd,
    Undo,
    Redo,
    RevertWorkspace,
    OpenThemeDialog,
    OpenModelSwitcher,
    FirstMessage,
    LastMessage,
    NextMessage,
    PreviousMessage,
    CopyMessage,
    ExportSession,
    ToggleScrollbar,
    OpenErrorDetails,
    PromptStash,
    PromptStashPop,
    PromptStashList,
    OpenLineageBrowser,
}

impl Action {
    fn metadata_id(self) -> Option<&'static str> {
        match self {
            Action::Quit => Some("quit"),
            Action::FocusNext => Some("focus_next"),
            Action::FocusPrev => Some("focus_prev"),
            Action::Palette => None,
            Action::Help => Some("help"),
            Action::ToggleOperatorSidebar => None,
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
            Action::SessionChildFirst => Some("session_child_first"),
            Action::SessionChildCycle => Some("session_child_cycle"),
            Action::SessionChildCycleReverse => Some("session_child_cycle_reverse"),
            Action::SessionParent => Some("session_parent"),
            Action::DiffHunkNext => Some("diff_hunk_next"),
            Action::DiffHunkPrevious => Some("diff_hunk_previous"),
            Action::AgentCycle => Some("agent_cycle"),
            Action::AgentCycleReverse => Some("agent_cycle_reverse"),
            Action::VariantCycle => Some("cycle_variant"),
            Action::AllowPermission => Some("allow_permission"),
            Action::DenyPermission => Some("deny_permission"),
            Action::DismissModal => Some("dismiss_modal"),
            Action::HistoryUp => Some("history_up"),
            Action::HistoryDown => Some("history_down"),
            Action::CursorLeft => None,
            Action::CursorRight => None,
            Action::Backspace => None,
            Action::Delete => None,
            Action::Char(_) => None,
            Action::SelectCharLeft => Some("select_char_left"),
            Action::SelectCharRight => Some("select_char_right"),
            Action::SelectWordLeft => Some("select_word_left"),
            Action::SelectWordRight => Some("select_word_right"),
            Action::SelectLine => Some("select_line"),
            Action::SelectAll => Some("select_all"),
            Action::MoveWordLeft => Some("move_word_left"),
            Action::MoveWordRight => Some("move_word_right"),
            Action::MoveLineStart => Some("move_line_start"),
            Action::MoveLineEnd => Some("move_line_end"),
            Action::MoveBufferStart => Some("move_buffer_start"),
            Action::MoveBufferEnd => Some("move_buffer_end"),
            Action::DeleteWordForward => Some("delete_word_forward"),
            Action::DeleteWordBackward => Some("delete_word_backward"),
            Action::DeleteLine => Some("delete_line"),
            Action::KillToLineStart => Some("kill_to_line_start"),
            Action::KillToLineEnd => Some("kill_to_line_end"),
            Action::Undo => Some("undo"),
            Action::Redo => Some("redo"),
            Action::RevertWorkspace => None,
            Action::OpenThemeDialog => Some("open_theme_dialog"),
            Action::OpenModelSwitcher => Some("open_model_switcher"),
            Action::FirstMessage => Some("first_message"),
            Action::LastMessage => Some("last_message"),
            Action::NextMessage => Some("next_message"),
            Action::PreviousMessage => Some("previous_message"),
            Action::CopyMessage => Some("copy_message"),
            Action::ExportSession => Some("export_session"),
            Action::ToggleScrollbar => Some("toggle_scrollbar"),
            Action::OpenErrorDetails => Some("open_error_details"),
            Action::PromptStash => Some("prompt_stash"),
            Action::PromptStashPop => Some("prompt_stash_pop"),
            Action::PromptStashList => Some("prompt_stash_list"),
            Action::OpenLineageBrowser => Some("open_lineage_browser"),
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
            Action::AgentCycle => "agent_cycle",
            Action::AgentCycleReverse => "agent_cycle_reverse",
            Action::VariantCycle => "variant_cycle",
            Action::AllowPermission => "allow_permission",
            Action::DenyPermission => "deny_permission",
            Action::DismissModal => "dismiss_modal",
            Action::HistoryUp => "history_up",
            Action::HistoryDown => "history_down",
            Action::CursorLeft => "cursor_left",
            Action::CursorRight => "cursor_right",
            Action::Backspace => "backspace",
            Action::Delete => "delete",
            Action::Char(_) => "char",
            Action::SelectCharLeft => "select_char_left",
            Action::SelectCharRight => "select_char_right",
            Action::SelectWordLeft => "select_word_left",
            Action::SelectWordRight => "select_word_right",
            Action::SelectLine => "select_line",
            Action::SelectAll => "select_all",
            Action::MoveWordLeft => "move_word_left",
            Action::MoveWordRight => "move_word_right",
            Action::MoveLineStart => "move_line_start",
            Action::MoveLineEnd => "move_line_end",
            Action::MoveBufferStart => "move_buffer_start",
            Action::MoveBufferEnd => "move_buffer_end",
            Action::DeleteWordForward => "delete_word_forward",
            Action::DeleteWordBackward => "delete_word_backward",
            Action::DeleteLine => "delete_line",
            Action::KillToLineStart => "kill_to_line_start",
            Action::KillToLineEnd => "kill_to_line_end",
            Action::Undo => "undo",
            Action::Redo => "redo",
            Action::RevertWorkspace => "revert_workspace",
            Action::OpenThemeDialog => "open_theme_dialog",
            Action::OpenModelSwitcher => "open_model_switcher",
            Action::FirstMessage => "first_message",
            Action::LastMessage => "last_message",
            Action::NextMessage => "next_message",
            Action::PreviousMessage => "previous_message",
            Action::CopyMessage => "copy_message",
            Action::ExportSession => "export_session",
            Action::ToggleScrollbar => "toggle_scrollbar",
            Action::OpenErrorDetails => "open_error_details",
            Action::PromptStash => "prompt_stash",
            Action::PromptStashPop => "prompt_stash_pop",
            Action::PromptStashList => "prompt_stash_list",
            Action::OpenLineageBrowser => "open_lineage_browser",
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
            "agent_cycle" => Ok(Action::AgentCycle),
            "agent_cycle_reverse" => Ok(Action::AgentCycleReverse),
            "variant_cycle" => Ok(Action::VariantCycle),
            "allow_permission" => Ok(Action::AllowPermission),
            "deny_permission" => Ok(Action::DenyPermission),
            "dismiss_modal" => Ok(Action::DismissModal),
            "history_up" => Ok(Action::HistoryUp),
            "history_down" => Ok(Action::HistoryDown),
            "cursor_left" => Ok(Action::CursorLeft),
            "cursor_right" => Ok(Action::CursorRight),
            "backspace" => Ok(Action::Backspace),
            "delete" => Ok(Action::Delete),
            "select_char_left" => Ok(Action::SelectCharLeft),
            "select_char_right" => Ok(Action::SelectCharRight),
            "select_word_left" => Ok(Action::SelectWordLeft),
            "select_word_right" => Ok(Action::SelectWordRight),
            "select_line" => Ok(Action::SelectLine),
            "select_all" => Ok(Action::SelectAll),
            "move_word_left" => Ok(Action::MoveWordLeft),
            "move_word_right" => Ok(Action::MoveWordRight),
            "move_line_start" => Ok(Action::MoveLineStart),
            "move_line_end" => Ok(Action::MoveLineEnd),
            "move_buffer_start" => Ok(Action::MoveBufferStart),
            "move_buffer_end" => Ok(Action::MoveBufferEnd),
            "delete_word_forward" => Ok(Action::DeleteWordForward),
            "delete_word_backward" => Ok(Action::DeleteWordBackward),
            "delete_line" => Ok(Action::DeleteLine),
            "kill_to_line_start" => Ok(Action::KillToLineStart),
            "kill_to_line_end" => Ok(Action::KillToLineEnd),
            "undo" => Ok(Action::Undo),
            "redo" => Ok(Action::Redo),
            "revert_workspace" => Ok(Action::RevertWorkspace),
            "open_theme_dialog" => Ok(Action::OpenThemeDialog),
            "open_model_switcher" => Ok(Action::OpenModelSwitcher),
            "first_message" => Ok(Action::FirstMessage),
            "last_message" => Ok(Action::LastMessage),
            "next_message" => Ok(Action::NextMessage),
            "previous_message" => Ok(Action::PreviousMessage),
            "copy_message" => Ok(Action::CopyMessage),
            "export_session" => Ok(Action::ExportSession),
            "toggle_scrollbar" => Ok(Action::ToggleScrollbar),
            "open_error_details" => Ok(Action::OpenErrorDetails),
            "prompt_stash" => Ok(Action::PromptStash),
            "prompt_stash_pop" => Ok(Action::PromptStashPop),
            "prompt_stash_list" => Ok(Action::PromptStashList),
            "open_lineage_browser" => Ok(Action::OpenLineageBrowser),
            _ => Err(format!("unknown action: {s}")),
        }
    }
}

/// A key binding that can match against KeyEvent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyBinding {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyBinding {
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }

    pub fn matches(&self, event: &KeyEvent) -> bool {
        self.code == event.code && self.modifiers == event.modifiers
    }
}

impl FromStr for KeyBinding {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let mut modifiers = KeyModifiers::NONE;
        let mut remaining = s;

        loop {
            let mut found = false;
            for (prefix, modifier) in [
                ("ctrl+", KeyModifiers::CONTROL),
                ("Ctrl+", KeyModifiers::CONTROL),
                ("shift+", KeyModifiers::SHIFT),
                ("Shift+", KeyModifiers::SHIFT),
                ("alt+", KeyModifiers::ALT),
                ("Alt+", KeyModifiers::ALT),
            ] {
                if let Some(key_part) = remaining.strip_prefix(prefix) {
                    modifiers |= modifier;
                    remaining = key_part;
                    found = true;
                    break;
                }
            }
            if !found {
                break;
            }
        }

        let code = parse_key_code(remaining)?;
        Ok(KeyBinding::new(code, modifiers))
    }
}

fn parse_key_code(s: &str) -> Result<KeyCode, String> {
    match s.to_lowercase().as_str() {
        "tab" => Ok(KeyCode::Tab),
        "backtab" | "shift-tab" => Ok(KeyCode::BackTab),
        "enter" => Ok(KeyCode::Enter),
        "esc" | "escape" => Ok(KeyCode::Esc),
        "space" | " " => Ok(KeyCode::Char(' ')),
        "up" => Ok(KeyCode::Up),
        "down" => Ok(KeyCode::Down),
        "left" => Ok(KeyCode::Left),
        "right" => Ok(KeyCode::Right),
        "backspace" => Ok(KeyCode::Backspace),
        "delete" | "del" => Ok(KeyCode::Delete),
        "home" => Ok(KeyCode::Home),
        "end" => Ok(KeyCode::End),
        "pageup" => Ok(KeyCode::PageUp),
        "pagedown" => Ok(KeyCode::PageDown),
        "insert" => Ok(KeyCode::Insert),
        "f1" => Ok(KeyCode::F(1)),
        "f2" => Ok(KeyCode::F(2)),
        "f3" => Ok(KeyCode::F(3)),
        "f4" => Ok(KeyCode::F(4)),
        "f5" => Ok(KeyCode::F(5)),
        "f6" => Ok(KeyCode::F(6)),
        "f7" => Ok(KeyCode::F(7)),
        "f8" => Ok(KeyCode::F(8)),
        "f9" => Ok(KeyCode::F(9)),
        "f10" => Ok(KeyCode::F(10)),
        "f11" => Ok(KeyCode::F(11)),
        "f12" => Ok(KeyCode::F(12)),
        s if s.len() == 1 => {
            let c = s.chars().next().unwrap();
            Ok(KeyCode::Char(c))
        }
        _ => Err(format!("unknown key: {s}")),
    }
}

/// Manages key bindings mapping keys to actions.
#[derive(Debug, Clone)]
pub struct KeyMap {
    bindings: HashMap<KeyBinding, Action>,
    reverse: HashMap<Action, Vec<KeyBinding>>,
    leader_sequences: HashMap<KeyBinding, Action>,
    leader_pending: bool,
}

impl Default for KeyMap {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl KeyMap {
    /// Create a new keymap with default bindings.
    pub fn with_defaults() -> Self {
        let mut keymap = Self {
            bindings: HashMap::new(),
            reverse: HashMap::new(),
            leader_sequences: HashMap::new(),
            leader_pending: false,
        };

        // Navigation
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

        // Focus cycling remains available on explicit control chords.
        keymap.bind(
            KeyBinding::new(KeyCode::Tab, KeyModifiers::CONTROL),
            Action::FocusNext,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::BackTab, KeyModifiers::CONTROL),
            Action::FocusPrev,
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
            KeyBinding::new(KeyCode::Char('4'), KeyModifiers::NONE),
            Action::ToggleTerminalPanel,
        );

        keymap.bind(
            KeyBinding::new(KeyCode::Char('i'), KeyModifiers::NONE),
            Action::ToggleOperatorSidebar,
        );

        // Actions
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
            KeyBinding::new(KeyCode::Char('?'), KeyModifiers::NONE),
            Action::Help,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('h'), KeyModifiers::NONE),
            Action::Help,
        );

        // Palette (Ctrl+P)
        keymap.bind(
            KeyBinding::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
            Action::Palette,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
            Action::VariantCycle,
        );

        // Prompt navigation (when in prompt focus)
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

        // Composer editing vocabulary
        keymap.bind(
            KeyBinding::new(KeyCode::Left, KeyModifiers::SHIFT),
            Action::SelectCharLeft,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Right, KeyModifiers::SHIFT),
            Action::SelectCharRight,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Left, KeyModifiers::SHIFT | KeyModifiers::CONTROL),
            Action::SelectWordLeft,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('b'), KeyModifiers::SHIFT | KeyModifiers::ALT),
            Action::SelectWordLeft,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Right, KeyModifiers::SHIFT | KeyModifiers::CONTROL),
            Action::SelectWordRight,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('f'), KeyModifiers::SHIFT | KeyModifiers::ALT),
            Action::SelectWordRight,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Home, KeyModifiers::SHIFT),
            Action::SelectLine,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::End, KeyModifiers::SHIFT),
            Action::SelectLine,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            Action::SelectAll,
        );
        keymap.bind(
            KeyBinding::new(
                KeyCode::Char('a'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
            Action::SelectAll,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Left, KeyModifiers::CONTROL),
            Action::MoveWordLeft,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('b'), KeyModifiers::ALT),
            Action::MoveWordLeft,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Right, KeyModifiers::CONTROL),
            Action::MoveWordRight,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('f'), KeyModifiers::ALT),
            Action::MoveWordRight,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Home, KeyModifiers::NONE),
            Action::MoveLineStart,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::End, KeyModifiers::NONE),
            Action::MoveLineEnd,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Home, KeyModifiers::CONTROL),
            Action::MoveBufferStart,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::End, KeyModifiers::CONTROL),
            Action::MoveBufferEnd,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('d'), KeyModifiers::ALT),
            Action::DeleteWordForward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Delete, KeyModifiers::CONTROL),
            Action::DeleteWordForward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
            Action::DeleteWordBackward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Backspace, KeyModifiers::ALT),
            Action::DeleteWordBackward,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Backspace, KeyModifiers::CONTROL),
            Action::DeleteWordBackward,
        );
        keymap.bind(
            KeyBinding::new(
                KeyCode::Char('k'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
            Action::DeleteLine,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
            Action::KillToLineStart,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
            Action::KillToLineEnd,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('z'), KeyModifiers::CONTROL),
            Action::Undo,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('-'), KeyModifiers::CONTROL),
            Action::Undo,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
            Action::Redo,
        );
        keymap.bind(
            KeyBinding::new(
                KeyCode::Char('z'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
            Action::Redo,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('.'), KeyModifiers::CONTROL),
            Action::Redo,
        );

        // Permission modal
        keymap.bind(
            KeyBinding::new(KeyCode::Char('y'), KeyModifiers::CONTROL),
            Action::AllowPermission,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
            Action::DenyPermission,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Esc, KeyModifiers::NONE),
            Action::DismissModal,
        );

        keymap.bind(
            KeyBinding::new(KeyCode::Char('g'), KeyModifiers::CONTROL),
            Action::FirstMessage,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Home, KeyModifiers::CONTROL),
            Action::FirstMessage,
        );
        keymap.bind(
            KeyBinding::new(
                KeyCode::Char('g'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
            Action::LastMessage,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::End, KeyModifiers::CONTROL | KeyModifiers::ALT),
            Action::LastMessage,
        );
        keymap.bind(
            KeyBinding::new(
                KeyCode::Char('n'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
            Action::NextMessage,
        );
        keymap.bind(
            KeyBinding::new(
                KeyCode::Char('p'),
                KeyModifiers::CONTROL | KeyModifiers::ALT,
            ),
            Action::PreviousMessage,
        );

        keymap.register_default_leader_sequences();
        keymap
    }

    /// Apply custom keybindings from config, overriding defaults.
    pub fn apply_overrides(&mut self, overrides: &BTreeMap<String, String>) {
        for (action_str, key_str) in overrides {
            if let Ok(action) = Action::from_str(action_str) {
                if let Ok(binding) = KeyBinding::from_str(key_str) {
                    if let Some(existing_bindings) = self.reverse.remove(&action) {
                        for existing in existing_bindings {
                            self.bindings.remove(&existing);
                        }
                    }

                    // Remove any existing binding for this key
                    if let Some(old_action) = self.bindings.remove(&binding) {
                        if let Some(bindings) = self.reverse.get_mut(&old_action) {
                            bindings.retain(|b| b != &binding);
                        }
                    }
                    // Add the new binding
                    self.bind(binding, action);
                }
            }
        }
    }

    /// Bind a key to an action.
    fn bind(&mut self, binding: KeyBinding, action: Action) {
        self.bindings.insert(binding, action);
        self.reverse.entry(action).or_default().push(binding);
    }

    /// Get the action for a key event, if any.
    pub fn get_action(&self, event: &KeyEvent) -> Option<Action> {
        let binding = KeyBinding::new(event.code, event.modifiers);
        self.bindings.get(&binding).copied().or_else(|| {
            (event.code == KeyCode::Char('\u{1d}') && event.modifiers == KeyModifiers::NONE)
                .then(|| {
                    self.bindings
                        .get(&KeyBinding::new(KeyCode::Char(']'), KeyModifiers::CONTROL))
                        .copied()
                })
                .flatten()
        })
    }

    pub fn leader_pending(&self) -> bool {
        self.leader_pending
    }

    pub fn set_leader_pending(&mut self, pending: bool) {
        self.leader_pending = pending;
    }

    pub fn leader_action(&self, event: &KeyEvent) -> Option<Action> {
        let binding = KeyBinding::new(event.code, event.modifiers);
        self.leader_sequences.get(&binding).copied()
    }

    fn bind_sequence(&mut self, binding: KeyBinding, action: Action) {
        self.leader_sequences.insert(binding, action);
    }

    fn register_default_leader_sequences(&mut self) {
        self.bind_sequence(
            KeyBinding::new(KeyCode::Char('t'), KeyModifiers::NONE),
            Action::OpenThemeDialog,
        );
        self.bind_sequence(
            KeyBinding::new(KeyCode::Char('m'), KeyModifiers::NONE),
            Action::OpenModelSwitcher,
        );
        self.bind_sequence(
            KeyBinding::new(KeyCode::Char('y'), KeyModifiers::NONE),
            Action::CopyMessage,
        );
        self.bind_sequence(
            KeyBinding::new(KeyCode::Char('x'), KeyModifiers::NONE),
            Action::ExportSession,
        );
        self.bind_sequence(
            KeyBinding::new(KeyCode::Char('s'), KeyModifiers::NONE),
            Action::ToggleScrollbar,
        );
        self.bind_sequence(
            KeyBinding::new(KeyCode::Char('g'), KeyModifiers::NONE),
            Action::OpenLineageBrowser,
        );
    }

    /// Get all key bindings for an action.
    pub fn get_bindings(&self, action: Action) -> Vec<&KeyBinding> {
        self.reverse
            .get(&action)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Get the primary key binding for an action as a string.
    pub fn get_binding_str(&self, action: Action) -> String {
        self.get_bindings(action)
            .first()
            .map(|b| format_key_binding(b))
            .unwrap_or_else(|| "-".to_string())
    }

    pub fn get_binding_strs(&self, action: Action) -> Vec<String> {
        self.get_bindings(action)
            .into_iter()
            .map(format_key_binding)
            .collect()
    }

    pub fn get_binding_label(&self, action: Action, label: &str) -> String {
        format!("{} {label}", self.get_binding_str(action))
    }

    /// Get all current bindings as a sorted list of (key, action) pairs.
    pub fn all_bindings(&self) -> Vec<(&KeyBinding, &Action)> {
        let mut all: Vec<(&KeyBinding, &Action)> = self.bindings.iter().collect();
        all.sort_by(|(left_key, left_action), (right_key, right_action)| {
            format_key_binding(left_key)
                .cmp(&format_key_binding(right_key))
                .then_with(|| left_action.as_str().cmp(right_action.as_str()))
        });
        all
    }
}

/// Format a key binding as a human-readable string.
fn format_key_binding(binding: &KeyBinding) -> String {
    let mut parts = Vec::new();

    if binding.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl");
    }
    if binding.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("Shift");
    }
    if binding.modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt");
    }

    let key_str = match binding.code {
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "Shift-Tab".to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Up => "↑".to_string(),
        KeyCode::Down => "↓".to_string(),
        KeyCode::Left => "←".to_string(),
        KeyCode::Right => "→".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Delete => "Del".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::PageUp => "PgUp".to_string(),
        KeyCode::PageDown => "PgDn".to_string(),
        KeyCode::Insert => "Ins".to_string(),
        KeyCode::F(n) => format!("F{n}"),
        _ => format!("{:?}", binding.code),
    };

    parts.push(&key_str);
    parts.join("+")
}

#[cfg(test)]
mod tests;
