//! Key hints shown next to compaction controls come from the configured keymap.
use super::AppState;
use crate::keybindings::Action;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

const DEFAULT_EXPAND_HINT: &str = "Ctrl+Alt+O";

impl AppState {
    /// Display text of the key that expands or collapses a compaction card.
    pub(crate) fn compaction_expand_hint(&self) -> String {
        let hint = self.keymap.get_binding_str(Action::ToggleCompactionDetails);
        if hint.is_empty() || hint == "-" {
            DEFAULT_EXPAND_HINT.to_owned()
        } else {
            display_key(&hint)
        }
    }

    /// Display text of the key that interrupts the running turn or compaction.
    pub(crate) fn interrupt_key_hint(&self) -> String {
        display_key(&self.keymap.interrupt_hint())
    }

    /// Whether the key event is the interrupt key: the configured binding or the built-in
    /// Ctrl+C.
    fn is_interrupt_key(&self, key: &KeyEvent) -> bool {
        self.keymap.get_action(key) == Some(Action::InterruptTurn) || is_ctrl_c(key)
    }

    /// Ctrl+C keeps its clear-the-draft-first behavior; a rebound interrupt key only
    /// interrupts.
    pub(in crate::app) fn handle_interrupt_key(&mut self, key: &KeyEvent) -> bool {
        if !self.is_interrupt_key(key) {
            return false;
        }
        if is_ctrl_c(key) {
            self.handle_ctrl_c_clear_or_cancel()
        } else {
            self.interrupt_active_turn()
        }
    }
}

fn is_ctrl_c(key: &KeyEvent) -> bool {
    key.modifiers == KeyModifiers::CONTROL
        && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
}

/// Key chords display with a capitalised final key: `Ctrl+Alt+O`.
fn display_key(chord: &str) -> String {
    match chord.rsplit_once('+') {
        Some((modifiers, key)) if key.chars().count() == 1 => {
            format!("{modifiers}+{}", key.to_uppercase())
        }
        _ => chord.to_owned(),
    }
}
