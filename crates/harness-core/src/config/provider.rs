use super::*;
use crate::auth::AuthProviderId;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ProviderConfig {
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible(OpenAiCompatibleProviderConfig),
    #[serde(rename = "anthropic_messages")]
    Anthropic(AnthropicProviderConfig),
    /// Claude Pro/Max through the Claude Code binary (`anthropicSubscriptionProvider`).
    #[serde(rename = "anthropic_subscription")]
    AnthropicSubscription(AnthropicSubscriptionProviderConfig),
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum SubscriptionSystemPromptMode {
    PresetAppend,
    Full,
    Override,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum SubscriptionResumeMode {
    Auto,
    Off,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum SubscriptionTokenInjection {
    OauthSlots,
    ConfigDir,
    Ambient,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionSettingSource {
    User,
    Project,
    Local,
}

/// Settings of the `anthropic-subscription` lane; unset fields keep senpi's defaults.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(default, deny_unknown_fields)]
pub struct AnthropicSubscriptionProviderConfig {
    pub name: Option<String>,
    /// Explicit opt-in for the host `claude` login (ambient lane).
    pub enabled: Option<bool>,
    #[serde(rename = "appendSystemPrompt", alias = "append_system_prompt")]
    pub append_system_prompt: Option<bool>,
    #[serde(rename = "systemPromptMode", alias = "system_prompt_mode")]
    pub system_prompt_mode: Option<SubscriptionSystemPromptMode>,
    #[serde(rename = "systemPromptFile", alias = "system_prompt_file")]
    pub system_prompt_file: Option<String>,
    #[serde(rename = "resumeMode", alias = "resume_mode")]
    pub resume_mode: Option<SubscriptionResumeMode>,
    #[serde(rename = "settingSources", alias = "setting_sources")]
    pub setting_sources: Option<Vec<SubscriptionSettingSource>>,
    #[serde(rename = "strictMcpConfig", alias = "strict_mcp_config")]
    pub strict_mcp_config: Option<bool>,
    #[serde(rename = "pinnedAccount", alias = "pinned_account")]
    pub pinned_account: Option<String>,
    #[serde(rename = "tokenInjection", alias = "token_injection")]
    pub token_injection: Option<SubscriptionTokenInjection>,
    pub models: BTreeMap<String, ModelConfig>,
}

impl AnthropicSubscriptionProviderConfig {
    pub fn settings(
        &self,
    ) -> harness_providers::anthropic_subscription::AnthropicSubscriptionSettings {
        use harness_providers::anthropic_subscription::{
            ResumeMode, SystemPromptMode, TokenInjection,
        };
        harness_providers::anthropic_subscription::AnthropicSubscriptionSettings {
            enabled: self.enabled,
            append_system_prompt: self.append_system_prompt,
            system_prompt_mode: self.system_prompt_mode.map(|mode| match mode {
                SubscriptionSystemPromptMode::PresetAppend => SystemPromptMode::PresetAppend,
                SubscriptionSystemPromptMode::Full => SystemPromptMode::Full,
                SubscriptionSystemPromptMode::Override => SystemPromptMode::Override,
            }),
            system_prompt_file: self.system_prompt_file.clone().filter(|f| !f.is_empty()),
            resume_mode: self.resume_mode.map(|mode| match mode {
                SubscriptionResumeMode::Auto => ResumeMode::Auto,
                SubscriptionResumeMode::Off => ResumeMode::Off,
            }),
            setting_sources: self.setting_sources.as_ref().map(|sources| {
                sources
                    .iter()
                    .map(|s| {
                        match s {
                            SubscriptionSettingSource::User => "user",
                            SubscriptionSettingSource::Project => "project",
                            SubscriptionSettingSource::Local => "local",
                        }
                        .to_owned()
                    })
                    .collect()
            }),
            strict_mcp_config: self.strict_mcp_config,
            pinned_account: self.pinned_account.clone().filter(|p| !p.is_empty()),
            token_injection: self.token_injection.map(|lane| match lane {
                SubscriptionTokenInjection::OauthSlots => TokenInjection::OauthSlots,
                SubscriptionTokenInjection::ConfigDir => TokenInjection::ConfigDir,
                SubscriptionTokenInjection::Ambient => TokenInjection::Ambient,
            }),
            system_prompt_mode_from_env: false,
        }
    }
}

/// The models the lane serves: senpi's `anthropic` model table (see `configs/anthropic-subscription-models.json`).
pub fn anthropic_subscription_models() -> BTreeMap<String, ModelConfig> {
    serde_json::from_str(include_str!(
        "../../../../configs/anthropic-subscription-models.json"
    ))
    .unwrap_or_default()
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct OpenAiCompatibleProviderConfig {
    pub name: Option<String>,
    #[serde(rename = "authProvider", alias = "auth_provider")]
    pub auth_provider: Option<AuthProviderId>,
    #[serde(rename = "baseURL", alias = "base_url", alias = "baseUrl")]
    pub base_url: String,
    #[serde(rename = "apiKey", alias = "api_key")]
    pub api_key: String,
    #[serde(
        rename = "apiKeyEnv",
        alias = "api_key_env",
        alias = "apiKeyEnvironment"
    )]
    pub api_key_env: Vec<String>,
    #[serde(rename = "timeoutMs", alias = "timeout_ms")]
    pub timeout_ms: u64,
    #[serde(rename = "apiMode", alias = "api_mode")]
    pub api_mode: OpenAiApiMode,
    #[serde(rename = "cacheRetention", alias = "cache_retention")]
    pub cache_retention: harness_providers::CacheRetention,
    pub headers: BTreeMap<String, String>,
    pub options: OpenAiCompatibleProviderOptions,
    pub models: BTreeMap<String, ModelConfig>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Debug, Default)]
#[serde(default, deny_unknown_fields)]
pub struct OpenAiCompatibleProviderOptions {
    #[serde(rename = "authProvider", alias = "auth_provider")]
    pub auth_provider: Option<AuthProviderId>,
    #[serde(rename = "baseURL", alias = "base_url", alias = "baseUrl")]
    pub base_url: Option<String>,
    #[serde(rename = "apiKey", alias = "api_key")]
    pub api_key: Option<String>,
    #[serde(
        rename = "apiKeyEnv",
        alias = "api_key_env",
        alias = "apiKeyEnvironment"
    )]
    pub api_key_env: Vec<String>,
    #[serde(rename = "apiMode", alias = "api_mode")]
    pub api_mode: Option<OpenAiApiMode>,
    #[serde(rename = "cacheRetention", alias = "cache_retention")]
    pub cache_retention: Option<harness_providers::CacheRetention>,
    #[serde(rename = "timeoutMs", alias = "timeout_ms")]
    pub timeout_ms: Option<u64>,
    pub headers: BTreeMap<String, String>,
    pub name: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Debug, Default, Copy)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiApiMode {
    Responses,
    ChatCompletions,
    #[default]
    Auto,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AnthropicProviderConfig {
    pub name: Option<String>,
    #[serde(rename = "baseURL", alias = "base_url", alias = "baseUrl")]
    pub base_url: String,
    #[serde(rename = "apiKey", alias = "api_key")]
    pub api_key: String,
    #[serde(
        rename = "apiKeyEnv",
        alias = "api_key_env",
        alias = "apiKeyEnvironment"
    )]
    pub api_key_env: Vec<String>,
    #[serde(rename = "timeoutMs", alias = "timeout_ms")]
    pub timeout_ms: u64,
    pub headers: BTreeMap<String, String>,
    pub options: AnthropicProviderOptions,
    pub models: BTreeMap<String, ModelConfig>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Debug, Default)]
#[serde(default, deny_unknown_fields)]
pub struct AnthropicProviderOptions {
    #[serde(rename = "baseURL", alias = "base_url", alias = "baseUrl")]
    pub base_url: Option<String>,
    #[serde(rename = "apiKey", alias = "api_key")]
    pub api_key: Option<String>,
    #[serde(
        rename = "apiKeyEnv",
        alias = "api_key_env",
        alias = "apiKeyEnvironment"
    )]
    pub api_key_env: Vec<String>,
    #[serde(rename = "timeoutMs", alias = "timeout_ms")]
    pub timeout_ms: Option<u64>,
    pub headers: BTreeMap<String, String>,
    pub name: Option<String>,
}

impl std::fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenAiCompatible(provider) => f
                .debug_struct("OpenAiCompatible")
                .field("models", &provider.models.keys())
                .finish_non_exhaustive(),
            Self::Anthropic(provider) => f
                .debug_struct("Anthropic")
                .field("models", &provider.models.keys())
                .finish_non_exhaustive(),
            Self::AnthropicSubscription(provider) => f
                .debug_struct("AnthropicSubscription")
                .field("models", &provider.models.keys())
                .finish_non_exhaustive(),
        }
    }
}

impl ProviderConfig {
    pub fn credential_provider(&self, name: &str) -> Option<AuthProviderId> {
        if let Self::OpenAiCompatible(provider) = self
            && let Some(id) = &provider.auth_provider
        {
            return Some(id.clone());
        }
        if matches!(self, Self::AnthropicSubscription(_)) {
            Some(AuthProviderId::anthropic_subscription())
        } else if name == "openai-codex" {
            Some(AuthProviderId::codex())
        } else {
            AuthProviderId::parse(name)
        }
    }
    pub fn models(&self) -> &BTreeMap<String, ModelConfig> {
        match self {
            Self::OpenAiCompatible(p) => &p.models,
            Self::Anthropic(p) => &p.models,
            Self::AnthropicSubscription(p) => &p.models,
        }
    }
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::OpenAiCompatible(p) => p.name.as_deref(),
            Self::Anthropic(p) => p.name.as_deref(),
            Self::AnthropicSubscription(p) => p.name.as_deref(),
        }
    }
    pub(super) fn normalize(&mut self) -> Result<(), ConfigError> {
        if let Self::AnthropicSubscription(p) = self {
            if p.models.is_empty() {
                p.models = anthropic_subscription_models();
            }
            return self.validate_model_limits();
        }
        let (base_url, timeout) = match self {
            Self::OpenAiCompatible(p) => {
                let options = &mut p.options;
                if let Some(value) = options.auth_provider.take() {
                    p.auth_provider = Some(value);
                }
                if let Some(value) = options.api_mode.take() {
                    p.api_mode = value;
                }
                if let Some(value) = options.cache_retention.take() {
                    p.cache_retention = value;
                }
                if let Some(value) = options.base_url.take() {
                    p.base_url = value;
                }
                if let Some(value) = options.api_key.take() {
                    p.api_key = value;
                }
                if !options.api_key_env.is_empty() {
                    p.api_key_env = std::mem::take(&mut options.api_key_env);
                }
                if let Some(value) = options.timeout_ms.take() {
                    p.timeout_ms = value;
                }
                if let Some(value) = options.name.take() {
                    p.name = Some(value);
                }
                p.headers.append(&mut options.headers);
                (&p.base_url, p.timeout_ms)
            }
            Self::Anthropic(p) => {
                let options = &mut p.options;
                if let Some(value) = options.base_url.take() {
                    p.base_url = value;
                }
                if let Some(value) = options.api_key.take() {
                    p.api_key = value;
                }
                if !options.api_key_env.is_empty() {
                    p.api_key_env = std::mem::take(&mut options.api_key_env);
                }
                if let Some(value) = options.timeout_ms.take() {
                    p.timeout_ms = value;
                }
                if let Some(value) = options.name.take() {
                    p.name = Some(value);
                }
                p.headers.append(&mut options.headers);
                (&p.base_url, p.timeout_ms)
            }
            Self::AnthropicSubscription(_) => return Ok(()),
        };
        if timeout == 0 || !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
            return Err(ConfigError(
                "providers require an HTTP(S) endpoint and a positive timeout".into(),
            ));
        }
        self.validate_model_limits()
    }
    fn validate_model_limits(&self) -> Result<(), ConfigError> {
        for model in self.models().values() {
            let limits = ResolvedModelLimits::from_values(
                model.limit.context.or(model.metadata.context_window_tokens),
                model.limit.input.or(model.max_input_tokens),
                model.limit.output.or(model.max_output_tokens),
                ModelLimitProvenance::explicit("model configuration"),
            );
            limits
                .validate(&model.display_name)
                .map_err(normalize::parse_error)?;
        }
        Ok(())
    }
}
