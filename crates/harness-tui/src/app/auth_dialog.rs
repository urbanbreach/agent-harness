use harness_core::auth::plugin::{AuthMethodSpec, AuthPluginRegistry};
use harness_core::auth::ProviderId;
use harness_core::provider_catalog::ProviderCatalog;

mod input;
mod lifecycle;
mod provider_menu;
mod selection;
#[cfg(test)]
mod tests;

pub(crate) use provider_menu::{
    is_filter_char, is_popular_connect_provider, normalize_custom_provider_id,
    ConnectProviderMenuItem,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectDialogStep {
    SelectProvider,
    SelectMethod,
    CustomProviderId,
    EnterpriseUrl,
    ApiKeyInput,
    Waiting,
    SelectModel,
    Success,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthorizationDetail {
    Url,
    Code,
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
    pub custom_provider: Option<ProviderId>,
    pub models: Vec<String>,
    pub selected_model: Option<usize>,
    pub toast: Option<AuthToastState>,
    pub prompt_index: usize,
    pub(crate) pointer_down: Option<AuthorizationDetail>,
    pub(crate) pointer_dragged: bool,
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
            custom_provider: None,
            models: Vec::new(),
            selected_model: None,
            toast: None,
            prompt_index: 0,
            pointer_down: None,
            pointer_dragged: false,
        }
    }
}

pub fn catalog_providers(
    catalog: &ProviderCatalog,
    registry: &AuthPluginRegistry,
) -> Vec<ConnectProviderOption> {
    let mut providers = catalog_entries(catalog, registry);
    // Claude Pro/Max runs through Claude Code, so it has no catalog entry of its own.
    if let Some(plugin) = registry.get(&ProviderId::anthropic_subscription()) {
        let at = providers
            .iter()
            .position(|p| p.id.as_str() == "anthropic")
            .map_or(0, |i| i + 1);
        providers.insert(
            at,
            ConnectProviderOption {
                id: ProviderId::anthropic_subscription(),
                label: plugin.label().to_string(),
                description: plugin.description().to_string(),
                methods: plugin.auth_methods().to_vec(),
                models: harness_core::config::anthropic_subscription_models()
                    .values()
                    .map(|m| m.display_name.clone())
                    .collect(),
            },
        );
    }
    providers
}

fn catalog_entries(
    catalog: &ProviderCatalog,
    registry: &AuthPluginRegistry,
) -> Vec<ConnectProviderOption> {
    catalog
        .sorted_by_priority()
        .into_iter()
        .filter_map(|entry| {
            let provider_id = ProviderId::parse(entry.id.as_str())?;
            let plugin = registry.get(&provider_id).or_else(|| {
                (provider_id.as_str() == "openai")
                    .then(ProviderId::codex)
                    .and_then(|codex| registry.get(&codex))
            });
            let (label, description, methods) = if let Some(plugin) = plugin {
                (
                    plugin.label().to_string(),
                    plugin.description().to_string(),
                    plugin.auth_methods().to_vec(),
                )
            } else {
                (
                    entry.name.clone(),
                    "API key".to_string(),
                    vec![AuthMethodSpec::ApiKey {
                        label: "Manually enter API Key".to_string(),
                    }],
                )
            };
            Some(ConnectProviderOption {
                id: provider_id,
                label,
                description,
                methods,
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

    /// The subscription login reads pasted input while it waits.
    pub fn accepts_waiting_input(&self) -> bool {
        self.selected_provider
            .and_then(|index| self.providers.get(index))
            .is_some_and(|provider| provider.id == ProviderId::anthropic_subscription())
    }

    /// The login's account-name question, once the sign-in itself has finished.
    pub fn naming_prompt(&self) -> Option<&str> {
        self.notice
            .as_deref()?
            .lines()
            .last()
            .filter(|line| line.starts_with("Name for this account"))
    }

    pub fn authorization_url(&self) -> Option<&str> {
        self.notice.as_deref()?.lines().find_map(|line| {
            let url = line
                .strip_prefix("Open ")
                .unwrap_or(line)
                .split_whitespace()
                .next()?;
            (url.starts_with("https://") || url.starts_with("http://")).then_some(url)
        })
    }

    pub fn authorization_code(&self) -> Option<&str> {
        self.authorization_detail("Enter code ").or_else(|| {
            self.authorization_detail("Open ")?
                .split_once(" and enter ")
                .map(|(_, code)| code)
        })
    }

    fn authorization_detail(&self, prefix: &str) -> Option<&str> {
        self.notice
            .as_deref()?
            .lines()
            .find_map(|line| line.strip_prefix(prefix))
            .filter(|value| !value.is_empty())
    }
}
