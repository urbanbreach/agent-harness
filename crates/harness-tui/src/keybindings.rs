//! Keybinding system for the TUI.
//!
//! Maps KeyEvent to Action with support for configurable overrides.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;

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
    ToggleDetailsDrawer,
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
    /// Switch to Run tab
    TabRun,
    /// Switch to Events tab
    TabEvents,
    /// Switch to Diff tab
    TabDiff,
    /// Switch to Help tab
    TabHelp,
    /// Move down in the list
    MoveDown,
    /// Move up in the list
    MoveUp,
    /// Reload (replay mode only)
    Reload,
    /// Allow permission in modal
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
    /// Convert action to its string identifier used in config.
    pub fn as_str(&self) -> &'static str {
        match self {
            Action::Quit => "quit",
            Action::FocusNext => "focus_next",
            Action::FocusPrev => "focus_prev",
            Action::Palette => "palette",
            Action::Help => "help",
            Action::ToggleDetailsDrawer => "toggle_details_drawer",
            Action::ToggleFollow => "toggle_follow",
            Action::SubmitPrompt => "submit_prompt",
            Action::InsertNewline => "insert_newline",
            Action::ClearPrompt => "clear_prompt",
            Action::ScrollUp => "scroll_up",
            Action::ScrollDown => "scroll_down",
            Action::TabRun => "tab_run",
            Action::TabEvents => "tab_events",
            Action::TabDiff => "tab_diff",
            Action::TabHelp => "tab_help",
            Action::MoveDown => "move_down",
            Action::MoveUp => "move_up",
            Action::Reload => "reload",
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
            ("help", "Open Help surface"),
            ("run", "Return to conversation surface"),
            ("details", "Toggle live details drawer"),
            ("events", "Open Events surface"),
            ("diff", "Open Diff surface"),
            ("toggle_follow", "Toggle follow mode"),
            ("quit", "Quit the application"),
        ]
    }

    pub fn palette_command_label(command: &str) -> &'static str {
        match command {
            "new_session" => "New session",
            "resume_session" => "Continue session",
            "replay_session" => "Replay session",
            "help" => "Help",
            "run" => "Run",
            "details" => "Details",
            "events" => "Events",
            "diff" => "Diff",
            "toggle_follow" => "Toggle follow",
            "quit" => "Quit",
            _ => "",
        }
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
            "toggle_details_drawer" => Ok(Action::ToggleDetailsDrawer),
            "toggle_follow" => Ok(Action::ToggleFollow),
            "submit_prompt" => Ok(Action::SubmitPrompt),
            "insert_newline" => Ok(Action::InsertNewline),
            "clear_prompt" => Ok(Action::ClearPrompt),
            "scroll_up" => Ok(Action::ScrollUp),
            "scroll_down" => Ok(Action::ScrollDown),
            "tab_run" => Ok(Action::TabRun),
            "tab_events" => Ok(Action::TabEvents),
            "tab_diff" => Ok(Action::TabDiff),
            "tab_help" => Ok(Action::TabHelp),
            "move_down" => Ok(Action::MoveDown),
            "move_up" => Ok(Action::MoveUp),
            "reload" => Ok(Action::Reload),
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
            Action::TabRun,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('i'), KeyModifiers::NONE),
            Action::ToggleDetailsDrawer,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('2'), KeyModifiers::NONE),
            Action::TabEvents,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('3'), KeyModifiers::NONE),
            Action::TabDiff,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('4'), KeyModifiers::NONE),
            Action::TabHelp,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('h'), KeyModifiers::NONE),
            Action::TabHelp,
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
            KeyBinding::new(KeyCode::Char('?'), KeyModifiers::NONE),
            Action::Help,
        );

        // Palette (Ctrl+P)
        keymap.bind(
            KeyBinding::new(KeyCode::Char('p'), KeyModifiers::CONTROL),
            Action::Palette,
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
            KeyBinding::new(KeyCode::Char('a'), KeyModifiers::NONE),
            Action::AllowPermission,
        );
        keymap.bind(
            KeyBinding::new(KeyCode::Char('d'), KeyModifiers::NONE),
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
}
