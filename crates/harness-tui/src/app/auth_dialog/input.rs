use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{normalize_custom_provider_id, ConnectDialogStep};
use crate::app::{AppState, UiIntent};

impl AppState {
    pub fn handle_connect_dialog_key(&mut self, key: KeyEvent) {
        match self.connect_dialog.step {
            ConnectDialogStep::SelectProvider
            | ConnectDialogStep::SelectMethod
            | ConnectDialogStep::SelectModel => self.handle_connect_dialog_select(key),
            ConnectDialogStep::CustomProviderId
            | ConnectDialogStep::ApiKeyInput
            | ConnectDialogStep::EnterpriseUrl => self.handle_connect_dialog_input(key),
            ConnectDialogStep::Waiting => {
                if key.code == KeyCode::Esc {
                    self.close_connect_dialog();
                }
            }
            ConnectDialogStep::Success | ConnectDialogStep::Error => {
                if key.code == KeyCode::Char('c') {
                    let provider_label = self
                        .connect_dialog
                        .selected_provider
                        .and_then(|index| self.connect_dialog.providers.get(index))
                        .map(|provider| provider.label.as_str())
                        .unwrap_or("provider");
                    self.connect_dialog.notice = Some(format!("Copied: {provider_label}"));
                }
                self.connect_dialog.toast = None;
                self.close_connect_dialog();
            }
        }
        self.maybe_auto_exit();
    }

    fn handle_connect_dialog_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if !c.is_control() {
                    self.connect_dialog
                        .input_buffer
                        .insert(self.connect_dialog.input_cursor, c);
                    self.connect_dialog.input_cursor += c.len_utf8();
                }
            }
            KeyCode::Backspace => self.backspace_connect_dialog_input(),
            KeyCode::Delete => self.delete_connect_dialog_input(),
            KeyCode::Left => self.move_connect_dialog_input_left(),
            KeyCode::Right => self.move_connect_dialog_input_right(),
            KeyCode::Home => self.connect_dialog.input_cursor = 0,
            KeyCode::End => {
                self.connect_dialog.input_cursor = self.connect_dialog.input_buffer.len()
            }
            KeyCode::Enter => self.submit_connect_dialog_input(),
            KeyCode::Esc => self.close_connect_dialog(),
            _ => {}
        }
    }

    fn backspace_connect_dialog_input(&mut self) {
        let cursor = self.connect_dialog.input_cursor;
        if cursor == 0 {
            return;
        }
        let previous = previous_char_boundary(&self.connect_dialog.input_buffer, cursor);
        self.connect_dialog.input_buffer.drain(previous..cursor);
        self.connect_dialog.input_cursor = previous;
    }

    fn delete_connect_dialog_input(&mut self) {
        let cursor = self.connect_dialog.input_cursor;
        if cursor >= self.connect_dialog.input_buffer.len() {
            return;
        }
        let next = next_char_boundary(&self.connect_dialog.input_buffer, cursor);
        self.connect_dialog.input_buffer.drain(cursor..next);
    }

    fn move_connect_dialog_input_left(&mut self) {
        self.connect_dialog.input_cursor = previous_char_boundary(
            &self.connect_dialog.input_buffer,
            self.connect_dialog.input_cursor,
        );
    }

    fn move_connect_dialog_input_right(&mut self) {
        self.connect_dialog.input_cursor = next_char_boundary(
            &self.connect_dialog.input_buffer,
            self.connect_dialog.input_cursor,
        );
    }

    fn submit_connect_dialog_input(&mut self) {
        let value = self.connect_dialog.input_buffer.trim().to_string();
        match self.connect_dialog.step {
            ConnectDialogStep::CustomProviderId => self.submit_custom_provider_id(&value),
            ConnectDialogStep::ApiKeyInput => self.submit_api_key(&value),
            ConnectDialogStep::EnterpriseUrl => self.submit_enterprise_url(value),
            _ => {}
        }
    }

    fn submit_custom_provider_id(&mut self, value: &str) {
        let Some(provider_id) = normalize_custom_provider_id(value) else {
            self.connect_dialog.error_message = Some(
                "Provider ids must start with a lowercase letter or number and only use lowercase letters, numbers, hyphens, and underscores".to_string(),
            );
            return;
        };
        self.connect_dialog.custom_provider = Some(provider_id);
        self.connect_dialog.selected_provider = None;
        self.connect_dialog.selected_method = None;
        self.connect_dialog.step = ConnectDialogStep::ApiKeyInput;
        self.connect_dialog.input_buffer.clear();
        self.connect_dialog.input_cursor = 0;
        self.connect_dialog.error_message = None;
    }

    fn submit_api_key(&mut self, value: &str) {
        if value.is_empty() {
            return;
        }
        let provider_id = self
            .connect_dialog
            .selected_provider
            .and_then(|index| self.connect_dialog.providers.get(index))
            .map(|provider| provider.id.clone())
            .or_else(|| self.connect_dialog.custom_provider.clone());
        let Some(provider_id) = provider_id else {
            return;
        };
        self.connect_dialog.step = ConnectDialogStep::Waiting;
        self.connect_dialog.notice = None;
        self.emit_ui_intent(UiIntent::OpenAuthManager {
            args: vec![
                "login".to_string(),
                provider_id.as_str().to_string(),
                "--method".to_string(),
                "api-key".to_string(),
                "--api-key-stdin".to_string(),
            ],
            stdin: Some(value.to_string()),
        });
        self.connect_dialog.input_buffer.clear();
        self.connect_dialog.input_cursor = 0;
    }

    fn submit_enterprise_url(&mut self, value: String) {
        let Some(provider) = self
            .connect_dialog
            .selected_provider
            .and_then(|index| self.connect_dialog.providers.get(index))
        else {
            return;
        };
        let method_index = self.connect_dialog.selected_method.unwrap_or(0);
        let Some(method) = provider.methods.get(method_index) else {
            return;
        };
        let provider_id = provider.id.clone();
        let method = method.clone();
        let url = if value.is_empty() { None } else { Some(value) };
        self.connect_dialog_emit_oauth_intent(&provider_id, &method, url);
        self.connect_dialog.input_buffer.clear();
        self.connect_dialog.input_cursor = 0;
    }
}

fn previous_char_boundary(value: &str, cursor: usize) -> usize {
    value[..cursor]
        .char_indices()
        .last()
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn next_char_boundary(value: &str, cursor: usize) -> usize {
    value[cursor..]
        .chars()
        .next()
        .map(|character| cursor + character.len_utf8())
        .unwrap_or(value.len())
}
