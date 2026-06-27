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
}
