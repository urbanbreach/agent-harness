use crossterm::event::{KeyCode, KeyEvent};
use harness_core::auth::plugin::AuthMethodSpec;
use harness_core::auth::ProviderId;

use super::{is_filter_char, ConnectDialogStep, ConnectProviderMenuItem};
use crate::app::{AppState, UiIntent};

impl AppState {
    pub(in crate::app::auth_dialog) fn handle_connect_dialog_select(&mut self, key: KeyEvent) {
        let count = match self.connect_dialog.step {
            ConnectDialogStep::SelectProvider => self.connect_dialog.provider_menu_items().len(),
            ConnectDialogStep::SelectMethod => self.connect_dialog.filtered_method_indices().len(),
            ConnectDialogStep::SelectModel => {
                self.connect_dialog.filtered_model_indices().len()
                    + usize::from(self.connect_dialog.skip_model_matches_filter())
            }
            _ => 0,
        };
        if count == 0 {
            match key.code {
                KeyCode::Char(c) if is_filter_char(c) => self.push_connect_dialog_filter(c),
                KeyCode::Backspace => self.pop_connect_dialog_filter(),
                KeyCode::Esc => self.close_connect_dialog(),
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_connect_dialog_selection(-1, count),
            KeyCode::Down | KeyCode::Char('j') => self.move_connect_dialog_selection(1, count),
            KeyCode::PageUp => self.move_connect_dialog_selection(-10, count),
            KeyCode::PageDown => self.move_connect_dialog_selection(10, count),
            KeyCode::Home => self.connect_dialog.selected = 0,
            KeyCode::End => self.connect_dialog.selected = count - 1,
            KeyCode::Enter => self.connect_dialog_enter_selection(),
            KeyCode::Esc => self.close_connect_dialog(),
            KeyCode::Char(c) if is_filter_char(c) => self.push_connect_dialog_filter(c),
            KeyCode::Backspace => self.pop_connect_dialog_filter(),
            _ => {}
        }
    }

    pub(in crate::app::auth_dialog) fn connect_dialog_emit_oauth_intent(
        &mut self,
        provider: &ProviderId,
        method: &AuthMethodSpec,
        enterprise_url: Option<String>,
    ) {
        let method_str = match method {
            AuthMethodSpec::OAuthAuto { .. } => "browser",
            AuthMethodSpec::OAuthCode { .. } => "device",
            AuthMethodSpec::ApiKey { .. } => "api-key",
            AuthMethodSpec::Prompts { .. } => "device",
        };
        let mut args = vec![
            "login".to_string(),
            provider.as_str().to_string(),
            "--method".to_string(),
            method_str.to_string(),
        ];
        if let Some(url) = &enterprise_url {
            args.push("--enterprise-url".to_string());
            args.push(url.clone());
        }
        self.connect_dialog.step = ConnectDialogStep::Waiting;
        self.connect_dialog.notice = None;
        self.connect_dialog.error_message = None;
        self.emit_ui_intent(UiIntent::OpenAuthManager { args, stdin: None });
    }

    fn push_connect_dialog_filter(&mut self, c: char) {
        self.connect_dialog.filter_buffer.push(c);
        self.connect_dialog.selected = 0;
    }

    fn pop_connect_dialog_filter(&mut self) {
        self.connect_dialog.filter_buffer.pop();
        self.connect_dialog.selected = 0;
    }

    fn move_connect_dialog_selection(&mut self, delta: isize, count: usize) {
        let selected = isize::try_from(self.connect_dialog.selected).unwrap_or(isize::MAX) + delta;
        self.connect_dialog.selected = if selected < 0 {
            count - 1
        } else if selected >= isize::try_from(count).unwrap_or(isize::MAX) {
            0
        } else {
            usize::try_from(selected).unwrap_or(usize::MAX)
        };
    }

    fn connect_dialog_enter_selection(&mut self) {
        let selected = self.connect_dialog.selected;
        match self.connect_dialog.step {
            ConnectDialogStep::SelectProvider => self.select_connect_provider(selected),
            ConnectDialogStep::SelectMethod => self.select_connect_method(selected),
            ConnectDialogStep::SelectModel => self.select_connect_model(selected),
            _ => {}
        }
    }

    fn select_connect_provider(&mut self, selected: usize) {
        let Some(item) = self
            .connect_dialog
            .provider_menu_items()
            .get(selected)
            .copied()
        else {
            return;
        };
        let ConnectProviderMenuItem::Provider(provider_index) = item else {
            self.connect_dialog.step = ConnectDialogStep::CustomProviderId;
            self.connect_dialog.input_buffer.clear();
            self.connect_dialog.input_cursor = 0;
            self.connect_dialog.error_message = None;
            self.connect_dialog.filter_buffer.clear();
            return;
        };
        self.connect_dialog.selected_provider = Some(provider_index);
        self.connect_dialog.custom_provider = None;
        let provider = &self.connect_dialog.providers[provider_index];
        if provider.methods.len() == 1 {
            self.connect_dialog.selected_method = Some(0);
            let provider_id = provider.id.clone();
            let method = provider.methods[0].clone();
            self.connect_dialog_start_auth(&provider_id, &method, None);
        } else if provider.methods.len() > 1 {
            self.connect_dialog.step = ConnectDialogStep::SelectMethod;
            self.connect_dialog.selected = 0;
            self.connect_dialog.filter_buffer.clear();
        }
    }

    fn select_connect_method(&mut self, selected: usize) {
        let Some(provider) = self
            .connect_dialog
            .selected_provider
            .and_then(|index| self.connect_dialog.providers.get(index))
        else {
            return;
        };
        let Some(method_index) = self
            .connect_dialog
            .filtered_method_indices()
            .get(selected)
            .copied()
        else {
            return;
        };
        let provider_id = provider.id.clone();
        let method = provider.methods[method_index].clone();
        self.connect_dialog.selected_method = Some(method_index);
        self.connect_dialog_start_auth(&provider_id, &method, None);
    }

    fn select_connect_model(&mut self, selected: usize) {
        let model_indices = self.connect_dialog.filtered_model_indices();
        if let Some(model_index) = model_indices.get(selected).copied() {
            self.connect_dialog.selected_model = Some(model_index);
        }
        self.connect_dialog.step = ConnectDialogStep::Success;
    }

    fn connect_dialog_start_auth(
        &mut self,
        provider: &ProviderId,
        method: &AuthMethodSpec,
        enterprise_url: Option<String>,
    ) {
        match method {
            AuthMethodSpec::ApiKey { .. } => {
                self.connect_dialog.step = ConnectDialogStep::ApiKeyInput;
                self.connect_dialog.input_buffer.clear();
                self.connect_dialog.input_cursor = 0;
                self.connect_dialog.filter_buffer.clear();
            }
            AuthMethodSpec::OAuthAuto { .. } | AuthMethodSpec::OAuthCode { .. } => {
                self.connect_dialog_emit_oauth_intent(provider, method, enterprise_url);
            }
            AuthMethodSpec::Prompts { .. } => {
                self.connect_dialog.step = ConnectDialogStep::EnterpriseUrl;
                self.connect_dialog.input_buffer.clear();
                self.connect_dialog.input_cursor = 0;
                self.connect_dialog.filter_buffer.clear();
                self.connect_dialog.prompt_index = 0;
            }
        }
    }
}
