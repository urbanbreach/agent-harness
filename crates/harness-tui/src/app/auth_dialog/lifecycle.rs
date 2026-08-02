use super::{AuthToastState, ConnectDialogStep, ConnectProviderOption};
use crate::app::AppState;

impl AppState {
    pub fn open_connect_dialog(&mut self) {
        self.connect_dialog.visible = true;
        self.connect_dialog.step = ConnectDialogStep::SelectProvider;
        self.connect_dialog.selected_provider = None;
        self.connect_dialog.selected_method = None;
        self.connect_dialog.selected = 0;
        self.connect_dialog.input_buffer.clear();
        self.connect_dialog.input_cursor = 0;
        self.connect_dialog.filter_buffer.clear();
        self.connect_dialog.notice = None;
        self.connect_dialog.error_message = None;
        self.connect_dialog.models.clear();
        self.connect_dialog.selected_model = None;
        self.connect_dialog.custom_provider = None;
        self.connect_dialog.toast = None;
        self.connect_dialog.prompt_index = 0;
    }

    pub fn close_connect_dialog(&mut self) {
        self.connect_dialog.visible = false;
    }

    pub fn set_connect_dialog_providers(&mut self, providers: Vec<ConnectProviderOption>) {
        self.connect_dialog.set_providers(providers);
    }

    pub fn append_connect_dialog_authorization_detail(&mut self, message: &str) {
        if !self.connect_dialog.visible || self.connect_dialog.step != ConnectDialogStep::Waiting {
            return;
        }
        let Some(detail) = message.strip_prefix("auth backend output: ") else {
            return;
        };
        if !detail.starts_with("Open ") && !detail.starts_with("Enter code ") {
            return;
        }
        self.connect_dialog.notice = Some(match self.connect_dialog.notice.take() {
            Some(notice) => format!("{notice}\n{detail}"),
            None => detail.to_string(),
        });
    }

    pub fn apply_connect_dialog_auth_result(&mut self, success: bool, message: &str) {
        if !self.connect_dialog.visible || self.connect_dialog.step != ConnectDialogStep::Waiting {
            return;
        }
        if success {
            self.connect_dialog.toast = Some(AuthToastState {
                message: "Connected successfully".to_string(),
                is_success: true,
            });
            self.connect_dialog.error_message = None;
            let models = self
                .connect_dialog
                .selected_provider
                .and_then(|index| self.connect_dialog.providers.get(index))
                .map(|provider| provider.models.clone())
                .unwrap_or_default();
            if models.is_empty() {
                self.connect_dialog.step = ConnectDialogStep::Success;
            } else {
                self.connect_dialog.models = models;
                self.connect_dialog.selected = 0;
                self.connect_dialog.selected_model = None;
                self.connect_dialog.step = ConnectDialogStep::SelectModel;
            }
        } else {
            self.connect_dialog.toast = Some(AuthToastState {
                message: "Authentication failed".to_string(),
                is_success: false,
            });
            self.connect_dialog.step = ConnectDialogStep::Error;
            self.connect_dialog.error_message = Some(if message.trim().is_empty() {
                "Authentication failed. Try again or run `harness auth login` in a terminal."
                    .to_string()
            } else {
                message.to_string()
            });
        }
    }
}
