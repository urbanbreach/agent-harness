use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{AppState, UiIntent};
use harness_core::auth::plugin::{AuthMethodSpec, AuthPluginRegistry};
use harness_core::auth::ProviderId;
use harness_core::provider_catalog::ProviderCatalog;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectDialogStep {
    SelectProvider,
    SelectMethod,
    EnterpriseUrl,
    ApiKeyInput,
    Waiting,
    SelectModel,
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub struct ConnectProviderOption {
    pub id: ProviderId,
    pub label: String,
    pub description: String,
    pub methods: Vec<AuthMethodSpec>,
    pub models: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AuthToastState {
    pub message: String,
    pub is_success: bool,
}

#[derive(Debug, Clone)]
pub struct ConnectDialogState {
    pub visible: bool,
    pub step: ConnectDialogStep,
    pub providers: Vec<ConnectProviderOption>,
    pub selected_provider: Option<usize>,
    pub selected_method: Option<usize>,
    pub selected: usize,
    pub input_buffer: String,
    pub input_cursor: usize,
    pub notice: Option<String>,
    pub error_message: Option<String>,
    pub filter_buffer: String,
    pub models: Vec<String>,
    pub selected_model: Option<usize>,
    pub toast: Option<AuthToastState>,
    pub prompt_index: usize,
}

impl Default for ConnectDialogState {
    fn default() -> Self {
        Self {
            visible: false,
            step: ConnectDialogStep::SelectProvider,
            providers: Vec::new(),
            selected_provider: None,
            selected_method: None,
            selected: 0,
            input_buffer: String::new(),
            input_cursor: 0,
            notice: None,
            error_message: None,
            filter_buffer: String::new(),
            models: Vec::new(),
            selected_model: None,
            toast: None,
            prompt_index: 0,
        }
    }
}

pub fn catalog_providers(
    catalog: &ProviderCatalog,
    registry: &AuthPluginRegistry,
) -> Vec<ConnectProviderOption> {
    catalog
        .sorted_by_priority()
        .into_iter()
        .filter_map(|entry| {
            let provider_id = ProviderId::parse(entry.id.as_str())?;
            let plugin = registry.get(&provider_id)?;
            Some(ConnectProviderOption {
                id: provider_id,
                label: plugin.label().to_string(),
                description: plugin.description().to_string(),
                methods: plugin.auth_methods().to_vec(),
                models: entry.models.values().map(|m| m.name.clone()).collect(),
            })
        })
        .collect()
}

pub fn auth_method_label(method: &AuthMethodSpec) -> &str {
    match method {
        AuthMethodSpec::OAuthAuto { label, .. } => label,
        AuthMethodSpec::OAuthCode { label } => label,
        AuthMethodSpec::ApiKey { label } => label,
        AuthMethodSpec::Prompts { label, .. } => label,
    }
}

impl ConnectDialogState {
    pub fn open() -> Self {
        Self {
            visible: true,
            step: ConnectDialogStep::SelectProvider,
            ..Default::default()
        }
    }

    pub fn set_providers(&mut self, providers: Vec<ConnectProviderOption>) {
        if !providers.is_empty() {
            self.providers = providers;
        }
    }

    pub fn refilter_providers(&self) -> Vec<&ConnectProviderOption> {
        if self.filter_buffer.is_empty() {
            return self.providers.iter().collect();
        }
        let filter = self.filter_buffer.to_lowercase();
        self.providers
            .iter()
            .filter(|p| {
                p.label.to_lowercase().contains(&filter)
                    || p.id.as_str().to_lowercase().contains(&filter)
            })
            .collect()
    }
}

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
        self.connect_dialog.toast = None;
        self.connect_dialog.prompt_index = 0;
    }

    pub fn close_connect_dialog(&mut self) {
        self.connect_dialog.visible = false;
    }

    pub fn set_connect_dialog_providers(&mut self, providers: Vec<ConnectProviderOption>) {
        self.connect_dialog.set_providers(providers);
    }

    pub fn handle_connect_dialog_key(&mut self, key: KeyEvent) {
        let step = self.connect_dialog.step;
        match step {
            ConnectDialogStep::SelectProvider
            | ConnectDialogStep::SelectMethod
            | ConnectDialogStep::SelectModel => {
                self.handle_connect_dialog_select(key);
            }
            ConnectDialogStep::ApiKeyInput | ConnectDialogStep::EnterpriseUrl => {
                self.handle_connect_dialog_input(key);
            }
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
                        .and_then(|i| self.connect_dialog.providers.get(i))
                        .map(|p| p.label.as_str())
                        .unwrap_or("provider");
                    self.connect_dialog.notice = Some(format!("Copied: {provider_label}"));
                }
                self.connect_dialog.toast = None;
                self.close_connect_dialog();
            }
        }
        self.maybe_auto_exit();
    }

    fn handle_connect_dialog_select(&mut self, key: KeyEvent) {
        let count = match self.connect_dialog.step {
            ConnectDialogStep::SelectProvider => self.connect_dialog.providers.len(),
            ConnectDialogStep::SelectMethod => {
                let idx = self.connect_dialog.selected_provider;
                idx.and_then(|i| self.connect_dialog.providers.get(i))
                    .map(|p| p.methods.len())
                    .unwrap_or(0)
            }
            ConnectDialogStep::SelectModel => self.connect_dialog.models.len() + 1,
            _ => 0,
        };
        if count == 0 {
            match key.code {
                KeyCode::Char(c)
                    if c.is_ascii_alphanumeric()
                        && self.connect_dialog.step == ConnectDialogStep::SelectProvider =>
                {
                    self.connect_dialog.filter_buffer.push(c);
                }
                KeyCode::Backspace
                    if self.connect_dialog.step == ConnectDialogStep::SelectProvider =>
                {
                    self.connect_dialog.filter_buffer.pop();
                }
                KeyCode::Esc => {
                    self.close_connect_dialog();
                }
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.connect_dialog.selected = if self.connect_dialog.selected == 0 {
                    count - 1
                } else {
                    self.connect_dialog.selected - 1
                };
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.connect_dialog.selected = (self.connect_dialog.selected + 1) % count;
            }
            KeyCode::Enter => {
                self.connect_dialog_enter_selection();
            }
            KeyCode::Esc => {
                if self.connect_dialog.step == ConnectDialogStep::SelectModel {
                    self.connect_dialog.step = ConnectDialogStep::Success;
                } else {
                    self.close_connect_dialog();
                }
            }
            KeyCode::Char(c)
                if c.is_ascii_alphanumeric()
                    && self.connect_dialog.step == ConnectDialogStep::SelectProvider =>
            {
                self.connect_dialog.filter_buffer.push(c);
            }
            KeyCode::Backspace if self.connect_dialog.step == ConnectDialogStep::SelectProvider => {
                self.connect_dialog.filter_buffer.pop();
            }
            _ => {}
        }
    }

    fn connect_dialog_enter_selection(&mut self) {
        let step = self.connect_dialog.step;
        let selected = self.connect_dialog.selected;
        match step {
            ConnectDialogStep::SelectProvider => {
                if selected >= self.connect_dialog.providers.len() {
                    return;
                }
                self.connect_dialog.selected_provider = Some(selected);
                let provider = &self.connect_dialog.providers[selected];
                let methods = &provider.methods;
                if methods.len() == 1 {
                    self.connect_dialog.selected_method = Some(0);
                    let provider_id = provider.id.clone();
                    let method = methods[0].clone();
                    self.connect_dialog_start_oauth_auth(&provider_id, &method, None);
                } else if methods.len() > 1 {
                    self.connect_dialog.step = ConnectDialogStep::SelectMethod;
                    self.connect_dialog.selected = 0;
                }
            }
            ConnectDialogStep::SelectMethod => {
                let idx = self.connect_dialog.selected_provider;
                let provider = idx.and_then(|i| self.connect_dialog.providers.get(i));
                let Some(provider) = provider else {
                    return;
                };
                if selected >= provider.methods.len() {
                    return;
                }
                let provider_id = provider.id.clone();
                let method = provider.methods[selected].clone();
                self.connect_dialog.selected_method = Some(selected);
                self.connect_dialog_start_oauth_auth(&provider_id, &method, None);
            }
            ConnectDialogStep::SelectModel => {
                let model_count = self.connect_dialog.models.len();
                if selected < model_count {
                    self.connect_dialog.selected_model = Some(selected);
                }
                self.connect_dialog.step = ConnectDialogStep::Success;
            }
            _ => {}
        }
    }

    fn connect_dialog_start_oauth_auth(
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
            }
            AuthMethodSpec::OAuthAuto { .. } | AuthMethodSpec::OAuthCode { .. } => {
                self.connect_dialog_emit_oauth_intent(provider, method, enterprise_url);
            }
            AuthMethodSpec::Prompts { .. } => {
                self.connect_dialog.step = ConnectDialogStep::EnterpriseUrl;
                self.connect_dialog.input_buffer.clear();
                self.connect_dialog.input_cursor = 0;
                self.connect_dialog.prompt_index = 0;
            }
        }
    }

    fn connect_dialog_emit_oauth_intent(
        &mut self,
        provider: &ProviderId,
        method: &AuthMethodSpec,
        enterprise_url: Option<String>,
    ) {
        let provider_str = provider.as_str();
        let method_str = match method {
            AuthMethodSpec::OAuthAuto { .. } => "browser",
            AuthMethodSpec::OAuthCode { .. } => "device",
            AuthMethodSpec::ApiKey { .. } => "api-key",
            AuthMethodSpec::Prompts { .. } => "device",
        };
        let mut args = vec![
            "login".to_string(),
            provider_str.to_string(),
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

    fn handle_connect_dialog_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                if !c.is_control() {
                    self.connect_dialog
                        .input_buffer
                        .insert(self.connect_dialog.input_cursor, c);
                    self.connect_dialog.input_cursor += 1;
                }
            }
            KeyCode::Backspace => {
                if self.connect_dialog.input_cursor > 0 {
                    self.connect_dialog.input_cursor -= 1;
                    self.connect_dialog
                        .input_buffer
                        .remove(self.connect_dialog.input_cursor);
                }
            }
            KeyCode::Enter => {
                let value = self.connect_dialog.input_buffer.trim().to_string();
                let step = self.connect_dialog.step;
                match step {
                    ConnectDialogStep::ApiKeyInput => {
                        if value.is_empty() {
                            return;
                        }
                        let idx = self.connect_dialog.selected_provider;
                        let provider = idx.and_then(|i| self.connect_dialog.providers.get(i));
                        let Some(provider) = provider else {
                            return;
                        };
                        let provider_str = provider.id.as_str().to_string();
                        self.connect_dialog.step = ConnectDialogStep::Waiting;
                        self.connect_dialog.notice = None;
                        self.emit_ui_intent(UiIntent::OpenAuthManager {
                            args: vec![
                                "login".to_string(),
                                provider_str,
                                "--method".to_string(),
                                "api-key".to_string(),
                                "--api-key-stdin".to_string(),
                            ],
                            stdin: Some(value),
                        });
                        self.connect_dialog.input_buffer.clear();
                        self.connect_dialog.input_cursor = 0;
                    }
                    ConnectDialogStep::EnterpriseUrl => {
                        let provider = self
                            .connect_dialog
                            .selected_provider
                            .and_then(|i| self.connect_dialog.providers.get(i));
                        let Some(provider) = provider else {
                            return;
                        };
                        let method_idx = self.connect_dialog.selected_method.unwrap_or(0);
                        let Some(method) = provider.methods.get(method_idx) else {
                            return;
                        };
                        let provider_id = provider.id.clone();
                        let method = method.clone();
                        let url = if value.is_empty() { None } else { Some(value) };
                        self.connect_dialog_emit_oauth_intent(&provider_id, &method, url);
                        self.connect_dialog.input_buffer.clear();
                        self.connect_dialog.input_cursor = 0;
                    }
                    _ => {}
                }
            }
            KeyCode::Esc => {
                self.connect_dialog.step = ConnectDialogStep::SelectProvider;
                self.connect_dialog.selected = 0;
                self.connect_dialog.input_buffer.clear();
                self.connect_dialog.input_cursor = 0;
            }
            _ => {}
        }
    }

    pub fn apply_connect_dialog_auth_result(&mut self, success: bool) {
        if !self.connect_dialog.visible {
            return;
        }
        if self.connect_dialog.step != ConnectDialogStep::Waiting {
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
                .and_then(|i| self.connect_dialog.providers.get(i))
                .map(|p| p.models.clone())
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
            self.connect_dialog.error_message = Some(
                "Authentication failed. Try again or run `harness auth login` in a terminal."
                    .to_string(),
            );
        }
    }
}
