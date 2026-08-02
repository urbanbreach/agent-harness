use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{AppState, UiIntent};

const MAX_WORKTREE_NAME_BYTES: usize = 100;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct NewWorktreeDialogState {
    pub(crate) visible: bool,
    pub(crate) input: String,
    pub(crate) cursor: usize,
}

impl AppState {
    pub(in crate::app) fn open_new_worktree_dialog(&mut self) {
        self.new_worktree_dialog.visible = true;
        self.new_worktree_dialog.input.clear();
        self.new_worktree_dialog.cursor = 0;
    }

    fn close_new_worktree_dialog(&mut self) {
        self.new_worktree_dialog.visible = false;
        self.new_worktree_dialog.input.clear();
        self.new_worktree_dialog.cursor = 0;
    }

    pub(in crate::app) fn handle_new_worktree_dialog_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'd' | 'q'))
        {
            self.close_new_worktree_dialog();
            return;
        }
        match key.code {
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if !character.is_control()
                    && self.new_worktree_dialog.input.len() + character.len_utf8()
                        <= MAX_WORKTREE_NAME_BYTES
                {
                    self.new_worktree_dialog
                        .input
                        .insert(self.new_worktree_dialog.cursor, character);
                    self.new_worktree_dialog.cursor += character.len_utf8();
                }
            }
            KeyCode::Backspace => {
                let cursor = self.new_worktree_dialog.cursor;
                if cursor > 0 {
                    let previous = self.new_worktree_dialog.input[..cursor]
                        .char_indices()
                        .last()
                        .map(|(index, _)| index)
                        .unwrap_or(0);
                    self.new_worktree_dialog.input.drain(previous..cursor);
                    self.new_worktree_dialog.cursor = previous;
                }
            }
            KeyCode::Delete => {
                let cursor = self.new_worktree_dialog.cursor;
                if cursor < self.new_worktree_dialog.input.len() {
                    let next = self.new_worktree_dialog.input[cursor..]
                        .chars()
                        .next()
                        .map(|character| cursor + character.len_utf8())
                        .unwrap_or(self.new_worktree_dialog.input.len());
                    self.new_worktree_dialog.input.drain(cursor..next);
                }
            }
            KeyCode::Left => {
                let cursor = self.new_worktree_dialog.cursor;
                self.new_worktree_dialog.cursor = self.new_worktree_dialog.input[..cursor]
                    .char_indices()
                    .last()
                    .map(|(index, _)| index)
                    .unwrap_or(0);
            }
            KeyCode::Right => {
                let cursor = self.new_worktree_dialog.cursor;
                self.new_worktree_dialog.cursor = self.new_worktree_dialog.input[cursor..]
                    .chars()
                    .next()
                    .map(|character| cursor + character.len_utf8())
                    .unwrap_or(self.new_worktree_dialog.input.len());
            }
            KeyCode::Home => self.new_worktree_dialog.cursor = 0,
            KeyCode::End => {
                self.new_worktree_dialog.cursor = self.new_worktree_dialog.input.len();
            }
            KeyCode::Enter if key.modifiers.is_empty() => {
                let name = self.new_worktree_dialog.input.trim();
                let name = (!name.is_empty()).then(|| name.to_string());
                self.close_new_worktree_dialog();
                self.apply_fresh_session_launcher_selection(UiIntent::NewWorktreeSession { name });
            }
            KeyCode::Esc => self.close_new_worktree_dialog(),
            _ => {}
        }
    }

    pub(in crate::app) fn handle_new_worktree_dialog_paste(&mut self, text: &str) -> bool {
        if !self.new_worktree_dialog.visible {
            return false;
        }
        for character in text.chars().filter(|character| !character.is_control()) {
            if self.new_worktree_dialog.input.len() + character.len_utf8() > MAX_WORKTREE_NAME_BYTES
            {
                break;
            }
            self.new_worktree_dialog
                .input
                .insert(self.new_worktree_dialog.cursor, character);
            self.new_worktree_dialog.cursor += character.len_utf8();
        }
        true
    }
}
