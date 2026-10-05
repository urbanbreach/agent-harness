use super::*;
use crate::auth::AuthProviderId;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ProviderConfig {
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible(OpenAiCompatibleProviderConfig),
    #[serde(rename = "anthropic_messages")]
    Anthropic(AnthropicProviderConfig),
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
        if name == "openai-codex" {
            Some(AuthProviderId::codex())
        } else {
            AuthProviderId::parse(name)
        }
    }
    pub fn models(&self) -> &BTreeMap<String, ModelConfig> {
        match self {
            Self::OpenAiCompatible(p) => &p.models,
            Self::Anthropic(p) => &p.models,
        }
    }
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::OpenAiCompatible(p) => p.name.as_deref(),
            Self::Anthropic(p) => p.name.as_deref(),
        }
    }
    pub(super) fn normalize(&mut self) -> Result<(), ConfigError> {
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
        };
        if timeout == 0 || !(base_url.starts_with("http://") || base_url.starts_with("https://")) {
            return Err(ConfigError(
                "providers require an HTTP(S) endpoint and a positive timeout".into(),
            ));
        }
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
