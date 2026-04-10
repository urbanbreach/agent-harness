//! Keybinding system for the TUI.
//!
//! Maps KeyEvent to Action with support for configurable overrides.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

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
            Self::Session => "Session",
            Self::Agent => "Agent",
            Self::System => "System",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteCommand {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub shortcut: &'static str,
    pub search_terms: &'static [&'static str],
    pub section: PaletteCommandSection,
}

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
    ToggleOrchestration,
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
}

impl Action {
    fn grouped_palette_commands() -> &'static [PaletteCommand] {
        &[
            PaletteCommand {
                id: "new_session",
                label: "New session",
                description: "Start a fresh live session",
                shortcut: "",
                search_terms: &["new", "start", "fresh"],
                section: PaletteCommandSection::Suggested,
            },
            PaletteCommand {
                id: "resume_session",
                label: "Continue session",
                description: "Continue a prior session when resumable",
                shortcut: "",
                search_terms: &["resume", "continue", "reopen", "recover", "saved"],
                section: PaletteCommandSection::Session,
            },
            PaletteCommand {
                id: "replay_session",
                label: "Replay session",
                description: "Replay a previous session as read-only",
                shortcut: "",
                search_terms: &["replay", "history", "saved", "read-only"],
                section: PaletteCommandSection::Session,
            },
            PaletteCommand {
                id: "switch_model",
                label: "Switch model",
                description: "Browse available provider/model options",
                shortcut: "/model",
                search_terms: &["model", "profile", "provider", "agent"],
                section: PaletteCommandSection::Agent,
            },
            PaletteCommand {
                id: "cycle_variant",
                label: "Cycle reasoning preset",
                description: "Cycle the configured model variant/reasoning preset",
                shortcut: "Ctrl+t",
                search_terms: &["cycle", "variant", "reasoning", "preset"],
                section: PaletteCommandSection::Agent,
            },
            PaletteCommand {
                id: "close_review_surface",
                label: "Session shell",
                description: "Return to the transcript-first session shell",
                shortcut: "Esc",
                search_terms: &["session", "shell", "back", "close"],
                section: PaletteCommandSection::Session,
            },
            PaletteCommand {
                id: "open_event_log",
                label: "Event log",
                description: "Open the review event log surface",
                shortcut: "3",
                search_terms: &["event", "events", "log", "review"],
                section: PaletteCommandSection::Session,
            },
            PaletteCommand {
                id: "toggle_operator_sidebar",
                label: "Toggle sidebar",
                description: "Show or hide the operator sidebar",
                shortcut: "i",
                search_terms: &["sidebar", "panel", "details", "files", "tools", "context"],
                section: PaletteCommandSection::Session,
            },
            PaletteCommand {
                id: "pause_orchestration",
                label: "Pause orchestration",
                description: "Pause delegated orchestration for new child-agent spawns",
                shortcut: "o",
                search_terms: &["pause", "orchestration", "delegation", "subagent", "agent"],
                section: PaletteCommandSection::Agent,
            },
            PaletteCommand {
                id: "resume_orchestration",
                label: "Resume orchestration",
                description: "Resume delegated orchestration for new child-agent spawns",
                shortcut: "o",
                search_terms: &["resume", "orchestration", "delegation", "subagent", "agent"],
                section: PaletteCommandSection::Agent,
            },
            PaletteCommand {
                id: "show_shortcuts",
                label: "Shortcuts",
                description: "Open or close the shortcuts review surface",
                shortcut: "?",
                search_terms: &["help", "shortcuts", "keys", "bindings"],
                section: PaletteCommandSection::System,
            },
            PaletteCommand {
                id: "toggle_follow",
                label: "Toggle follow",
                description: "Toggle follow mode",
                shortcut: "Space",
                search_terms: &["follow", "autoscroll", "scroll"],
                section: PaletteCommandSection::Agent,
            },
            PaletteCommand {
                id: "show_thinking",
                label: "Show thinking",
                description: "Restore inline thinking rows in the transcript",
                shortcut: "",
                search_terms: &["show", "thinking", "reasoning", "trace"],
                section: PaletteCommandSection::Agent,
            },
            PaletteCommand {
                id: "hide_thinking",
                label: "Hide thinking",
                description: "Hide inline thinking rows in the transcript",
                shortcut: "",
                search_terms: &["hide", "thinking", "reasoning", "trace"],
                section: PaletteCommandSection::Agent,
            },
            PaletteCommand {
                id: "show_timestamps",
                label: "Show timestamps",
                description: "Reveal user message timestamps in the transcript",
                shortcut: "",
                search_terms: &["show", "timestamps", "time", "clock"],
                section: PaletteCommandSection::Agent,
            },
            PaletteCommand {
                id: "hide_timestamps",
                label: "Hide timestamps",
                description: "Hide user message timestamps in the transcript",
                shortcut: "",
                search_terms: &["hide", "timestamps", "time", "clock"],
                section: PaletteCommandSection::Agent,
            },
            PaletteCommand {
                id: "show_tool_details",
                label: "Show tool details",
                description: "Show completed successful tools in the transcript",
                shortcut: "",
                search_terms: &["show", "tool", "tools", "details"],
                section: PaletteCommandSection::Agent,
            },
            PaletteCommand {
                id: "hide_tool_details",
                label: "Hide tool details",
                description: "Hide completed successful tools in the transcript",
                shortcut: "",
                search_terms: &["hide", "tool", "tools", "details"],
                section: PaletteCommandSection::Agent,
            },
            PaletteCommand {
                id: "show_generic_tool_output",
                label: "Show generic tool output",
                description: "Expand generic tool payload blocks in the transcript",
                shortcut: "",
                search_terms: &["show", "generic", "tool", "output", "payload"],
                section: PaletteCommandSection::Agent,
            },
            PaletteCommand {
                id: "hide_generic_tool_output",
                label: "Hide generic tool output",
                description: "Collapse generic tool payload blocks in the transcript",
                shortcut: "",
                search_terms: &["hide", "generic", "tool", "output", "payload"],
                section: PaletteCommandSection::Agent,
            },
            PaletteCommand {
                id: "expand_selected_turn_results",
                label: "Expand turn results",
                description: "Expand overflow tool output in the selected turn",
                shortcut: "",
                search_terms: &["expand", "turn", "results", "overflow", "tool output"],
                section: PaletteCommandSection::Agent,
            },
            PaletteCommand {
                id: "collapse_selected_turn_results",
                label: "Collapse turn results",
                description: "Collapse overflow tool output in the selected turn",
                shortcut: "",
                search_terms: &["collapse", "turn", "results", "overflow", "tool output"],
                section: PaletteCommandSection::Agent,
            },
            PaletteCommand {
                id: "stack_transcript_diffs",
                label: "Use stacked diffs",
                description: "Force unified stacked transcript diffs",
                shortcut: "",
                search_terms: &["stacked", "diff", "diffs", "unified"],
                section: PaletteCommandSection::Agent,
            },
            PaletteCommand {
                id: "split_transcript_diffs",
                label: "Use split diffs",
                description: "Allow side-by-side transcript diffs when wide",
                shortcut: "",
                search_terms: &["split", "diff", "diffs", "side-by-side"],
                section: PaletteCommandSection::Agent,
            },
            PaletteCommand {
                id: "reload_replay",
                label: "Reload replay",
                description: "Reload the current replay session from disk",
                shortcut: "r",
                search_terms: &["reload", "refresh", "replay"],
                section: PaletteCommandSection::System,
            },
            PaletteCommand {
                id: "quit",
                label: "Quit",
                description: "Quit the application",
                shortcut: "q",
                search_terms: &["quit", "exit", "close"],
                section: PaletteCommandSection::System,
            },
        ]
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
            Action::ToggleOrchestration => "toggle_orchestration",
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
        }
    }

    /// Get the list of all palette-executable actions.
    pub fn palette_commands() -> &'static [(&'static str, &'static str)] {
        &[
            ("new_session", "Start a fresh live session"),
            ("resume_session", "Continue a prior session when resumable"),
            ("replay_session", "Replay a previous session as read-only"),
            ("switch_model", "Browse available provider/model options"),
            (
                "show_shortcuts",
                "Open or close the shortcuts review surface",
            ),
            (
                "cycle_variant",
                "Cycle the configured model variant/reasoning preset",
            ),
            (
                "close_review_surface",
                "Return to the transcript-first session shell",
            ),
            ("open_event_log", "Open the review event log surface"),
            (
                "toggle_operator_sidebar",
                "Show or hide the operator sidebar",
            ),
            (
                "pause_orchestration",
                "Pause delegated orchestration for new child-agent spawns",
            ),
            (
                "resume_orchestration",
                "Resume delegated orchestration for new child-agent spawns",
            ),
            ("toggle_follow", "Toggle follow mode"),
            (
                "show_thinking",
                "Restore inline thinking rows in the transcript",
            ),
            (
                "hide_thinking",
                "Hide inline thinking rows in the transcript",
            ),
            (
                "show_timestamps",
                "Reveal user message timestamps in the transcript",
            ),
            (
                "hide_timestamps",
                "Hide user message timestamps in the transcript",
            ),
            (
                "show_tool_details",
                "Show completed successful tools in the transcript",
            ),
            (
                "hide_tool_details",
                "Hide completed successful tools in the transcript",
            ),
            (
                "show_generic_tool_output",
                "Expand generic tool payload blocks in the transcript",
            ),
            (
                "hide_generic_tool_output",
                "Collapse generic tool payload blocks in the transcript",
            ),
            (
                "expand_selected_turn_results",
                "Expand overflow tool output in the selected turn",
            ),
            (
                "collapse_selected_turn_results",
                "Collapse overflow tool output in the selected turn",
            ),
            (
                "stack_transcript_diffs",
                "Force unified stacked transcript diffs",
            ),
            (
                "split_transcript_diffs",
                "Allow side-by-side transcript diffs when wide",
            ),
            (
                "reload_replay",
                "Reload the current replay session from disk",
            ),
            ("quit", "Quit the application"),
        ]
    }

    pub fn palette_command_label(command: &str) -> &'static str {
        Self::grouped_palette_commands()
            .iter()
            .find_map(|palette_command| {
                (palette_command.id == command).then_some(palette_command.label)
            })
            .unwrap_or("")
    }

    pub fn palette_command_description(command: &str) -> &'static str {
        Self::grouped_palette_commands()
            .iter()
            .find_map(|palette_command| {
                (palette_command.id == command).then_some(palette_command.description)
            })
            .unwrap_or("")
    }

    pub fn palette_command_shortcut(command: &str) -> &'static str {
        Self::grouped_palette_commands()
            .iter()
            .find_map(|palette_command| {
                (palette_command.id == command).then_some(palette_command.shortcut)
            })
            .unwrap_or("")
    }

    pub fn palette_command_section(command: &str) -> Option<PaletteCommandSection> {
        Self::grouped_palette_commands()
            .iter()
            .find_map(|palette_command| {
                (palette_command.id == command).then_some(palette_command.section)
            })
    }

    pub fn palette_command_search_terms(command: &str) -> &'static [&'static str] {
        Self::grouped_palette_commands()
            .iter()
            .find_map(|palette_command| {
                (palette_command.id == command).then_some(palette_command.search_terms)
            })
            .unwrap_or(&[])
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
            "toggle_orchestration" => Ok(Action::ToggleOrchestration),
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

        // Handle Ctrl+ prefix
        if s.starts_with("ctrl+") || s.starts_with("Ctrl+") {
            let key_part = &s[5..];
            let code = parse_key_code(key_part)?;
            return Ok(KeyBinding::new(code, KeyModifiers::CONTROL));
        }

        // Handle Shift+ prefix
        if s.starts_with("shift+") || s.starts_with("Shift+") {
            let key_part = &s[6..];
            let code = parse_key_code(key_part)?;
            return Ok(KeyBinding::new(code, KeyModifiers::SHIFT));
        }

        // Handle Alt+ prefix
        if s.starts_with("alt+") || s.starts_with("Alt+") {
            let key_part = &s[4..];
            let code = parse_key_code(key_part)?;
            return Ok(KeyBinding::new(code, KeyModifiers::ALT));
        }

        // Single key (no modifiers)
        let code = parse_key_code(s)?;
        Ok(KeyBinding::new(code, KeyModifiers::NONE))
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

        // Focus cycling
        keymap.bind(
            KeyBinding::new(KeyCode::Tab, KeyModifiers::NONE),
            Action::FocusNext,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::BackTab, KeyModifiers::NONE),
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
            KeyBinding::new(KeyCode::Char('3'), KeyModifiers::NONE),
            Action::OpenEventLog,
        );

        keymap.bind(
            KeyBinding::new(KeyCode::Char('i'), KeyModifiers::NONE),
            Action::ToggleOperatorSidebar,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('o'), KeyModifiers::NONE),
            Action::ToggleOrchestration,
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
        self.bindings.get(&binding).copied()
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
mod tests {
    use super::*;

    #[test]
    fn action_from_str_roundtrip() {
        let action = Action::Quit;
        let s = action.as_str();
        let parsed = Action::from_str(s).unwrap();
        assert_eq!(action, parsed);
    }

    #[test]
    fn key_binding_parses_ctrl_p() {
        let binding = KeyBinding::from_str("ctrl+p").unwrap();
        assert_eq!(binding.code, KeyCode::Char('p'));
        assert!(binding.modifiers.contains(KeyModifiers::CONTROL));
    }

    #[test]
    fn key_binding_parses_single_char() {
        let binding = KeyBinding::from_str("q").unwrap();
        assert_eq!(binding.code, KeyCode::Char('q'));
        assert_eq!(binding.modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn keymap_finds_default_binding() {
        let keymap = KeyMap::with_defaults();
        let event = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        assert_eq!(keymap.get_action(&event), Some(Action::Quit));
    }

    #[test]
    fn keymap_overrides_binding() {
        let mut overrides = BTreeMap::new();
        overrides.insert("quit".to_string(), "x".to_string());

        let mut keymap = KeyMap::with_defaults();
        keymap.apply_overrides(&overrides);

        let event = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        assert_eq!(keymap.get_action(&event), Some(Action::Quit));

        // Default 'q' should no longer work
        let old_event = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        assert_ne!(keymap.get_action(&old_event), Some(Action::Quit));
    }

    #[test]
    fn keymap_returns_binding_str() {
        let keymap = KeyMap::with_defaults();
        let binding = keymap.get_binding_str(Action::Quit);
        assert_eq!(binding, "q");
    }

    #[test]
    fn keymap_returns_ctrl_binding_str() {
        let keymap = KeyMap::with_defaults();
        let binding = keymap.get_binding_str(Action::Palette);
        assert_eq!(binding, "Ctrl+p");
    }

    #[test]
    fn keymap_formats_binding_labels_from_overrides() {
        let mut overrides = BTreeMap::new();
        overrides.insert("quit".to_string(), "x".to_string());

        let mut keymap = KeyMap::with_defaults();
        keymap.apply_overrides(&overrides);

        assert_eq!(keymap.get_binding_label(Action::Quit, "quit"), "x quit");
    }

    #[test]
    fn keymap_binds_shift_enter_to_insert_newline() {
        let keymap = KeyMap::with_defaults();
        let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
        assert_eq!(keymap.get_action(&event), Some(Action::InsertNewline));
    }

    #[test]
    fn keymap_binds_ctrl_j_to_insert_newline() {
        let keymap = KeyMap::with_defaults();
        let event = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL);
        assert_eq!(keymap.get_action(&event), Some(Action::InsertNewline));
    }

    #[test]
    fn keymap_binds_ctrl_enter_to_insert_newline() {
        let keymap = KeyMap::with_defaults();
        let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL);
        assert_eq!(keymap.get_action(&event), Some(Action::InsertNewline));
    }

    #[test]
    fn keymap_binds_alt_enter_to_insert_newline() {
        let keymap = KeyMap::with_defaults();
        let event = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
        assert_eq!(keymap.get_action(&event), Some(Action::InsertNewline));
    }

    #[test]
    fn keymap_uses_ctrl_y_and_ctrl_n_for_permission_decisions() {
        let keymap = KeyMap::with_defaults();

        let allow = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL);
        let deny = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL);

        assert_eq!(keymap.get_action(&allow), Some(Action::AllowPermission));
        assert_eq!(keymap.get_action(&deny), Some(Action::DenyPermission));
        assert_eq!(keymap.get_binding_str(Action::AllowPermission), "Ctrl+y");
        assert_eq!(keymap.get_binding_str(Action::DenyPermission), "Ctrl+n");
    }

    #[test]
    fn keymap_binds_child_session_navigation_to_opencode_defaults() {
        let keymap = KeyMap::with_defaults();

        assert_eq!(
            keymap.get_action(&KeyEvent::new(KeyCode::Char(']'), KeyModifiers::CONTROL)),
            Some(Action::SessionChildFirst)
        );
        assert_eq!(
            keymap.get_action(&KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE)),
            Some(Action::SessionChildCycle)
        );
        assert_eq!(
            keymap.get_action(&KeyEvent::new(KeyCode::Char('['), KeyModifiers::NONE)),
            Some(Action::SessionChildCycleReverse)
        );
        assert_eq!(
            keymap.get_action(&KeyEvent::new(KeyCode::Char('['), KeyModifiers::CONTROL)),
            Some(Action::SessionParent)
        );
    }

    #[test]
    fn keymap_accepts_variant_cycle_overrides() {
        let mut overrides = BTreeMap::new();
        overrides.insert("variant_cycle".to_string(), "tab".to_string());

        let mut keymap = KeyMap::with_defaults();
        keymap.apply_overrides(&overrides);

        assert_eq!(
            keymap.get_action(&KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            Some(Action::VariantCycle)
        );
    }

    #[test]
    fn keymap_binds_ctrl_t_to_variant_cycle() {
        let keymap = KeyMap::with_defaults();

        assert_eq!(
            keymap.get_action(&KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
            Some(Action::VariantCycle)
        );
        assert_eq!(keymap.get_binding_str(Action::VariantCycle), "Ctrl+t");
    }

    #[test]
    fn palette_commands_keep_search_aliases_separate_from_visible_hints() {
        assert_eq!(Action::palette_command_shortcut("new_session"), "");
        assert_eq!(Action::palette_command_shortcut("switch_model"), "/model");
        assert_eq!(Action::palette_command_shortcut("show_shortcuts"), "?");

        assert!(
            Action::palette_command_search_terms("resume_session").contains(&"resume"),
            "resume_session should stay searchable by resume aliases"
        );
        assert!(
            Action::palette_command_search_terms("reload_replay").contains(&"refresh"),
            "reload_replay should stay searchable by refresh aliases"
        );
    }
}
