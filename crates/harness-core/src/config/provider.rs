use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::auth::AuthProviderId;

use super::aliases::{merge_map_alias, merge_option_alias, merge_string_alias, merge_vec_alias};
use super::defaults::default_provider_timeout_ms;
use super::{ConfigError, ModelConfig};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum ProviderConfig {
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible(OpenAiCompatibleProviderConfig),
    #[serde(rename = "anthropic_messages")]
    Anthropic(AnthropicProviderConfig),
}

impl ProviderConfig {
    pub(super) fn models(&self) -> &BTreeMap<String, ModelConfig> {
        match self {
            Self::OpenAiCompatible(config) => &config.models,
            Self::Anthropic(config) => &config.models,
        }
    }

    pub(super) fn display_label(&self, provider_name: &str) -> String {
        match self {
            Self::OpenAiCompatible(config) => config
                .name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(provider_name)
                .to_string(),
            Self::Anthropic(config) => config
                .name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(provider_name)
                .to_string(),
        }
    }

    pub(super) fn normalize_public_config_aliases(&mut self) -> Result<(), ConfigError> {
        match self {
            Self::OpenAiCompatible(config) => config.normalize_public_config_aliases(),
            Self::Anthropic(config) => config.normalize_public_config_aliases(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenAiCompatibleProviderConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(rename = "authProvider", default, alias = "auth_provider")]
    pub auth_provider: Option<AuthProviderId>,
    #[serde(rename = "baseURL", default, alias = "base_url", alias = "baseUrl")]
    pub base_url: String,
    #[serde(rename = "apiKey", default, alias = "api_key")]
    pub api_key: String,
    #[serde(
        rename = "apiKeyEnv",
        default,
        alias = "api_key_env",
        alias = "apiKeyEnvironment"
    )]
    pub api_key_env: Vec<String>,
    #[serde(
        rename = "timeoutMs",
        default = "default_provider_timeout_ms",
        alias = "timeout_ms"
    )]
    pub timeout_ms: u64,
    #[serde(rename = "apiMode", default, alias = "api_mode")]
    pub api_mode: OpenAiApiMode,
    #[serde(rename = "cacheRetention", default, alias = "cache_retention")]
    pub cache_retention: harness_providers::CacheRetention,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub options: OpenAiCompatibleProviderOptions,
    #[serde(default)]
    pub models: BTreeMap<String, ModelConfig>,
}

impl OpenAiCompatibleProviderConfig {
    fn normalize_public_config_aliases(&mut self) -> Result<(), ConfigError> {
        merge_string_alias(
            &mut self.base_url,
            self.options.base_url.take(),
            "provider openai_compatible.base_url",
            "provider openai_compatible.options.baseURL",
        )?;
        merge_string_alias(
            &mut self.api_key,
            self.options.api_key.take(),
            "provider openai_compatible.api_key",
            "provider openai_compatible.options.apiKey",
        )?;
        merge_vec_alias(
            &mut self.api_key_env,
            std::mem::take(&mut self.options.api_key_env),
            "provider openai_compatible.api_key_env",
            "provider openai_compatible.options.apiKeyEnv",
        )?;
        merge_string_alias(
            &mut self.name,
            self.options.name.take(),
            "provider openai_compatible.name",
            "provider openai_compatible.options.name",
        )?;
        merge_option_alias(
            &mut self.auth_provider,
            self.options.auth_provider.take(),
            "provider openai_compatible.auth_provider",
            "provider openai_compatible.options.authProvider",
        )?;
        merge_map_alias(
            &mut self.headers,
            std::mem::take(&mut self.options.headers),
            "provider openai_compatible.headers",
            "provider openai_compatible.options.headers",
        )?;

        if let Some(api_mode) = self.options.api_mode.take() {
            if matches!(self.api_mode, OpenAiApiMode::Auto) {
                self.api_mode = api_mode;
            } else if self.api_mode != api_mode {
                return Err(ConfigError::InvalidReference(
                    "provider openai_compatible.api_mode conflicts with provider openai_compatible.options.apiMode; use one value"
                        .to_string(),
                ));
            }
        }

        if let Some(cache_retention) = self.options.cache_retention.take() {
            if self.cache_retention == harness_providers::CacheRetention::Short {
                self.cache_retention = cache_retention;
            } else if self.cache_retention != cache_retention {
                return Err(ConfigError::InvalidReference(
                    "provider openai_compatible.cache_retention conflicts with provider openai_compatible.options.cacheRetention; use one value"
                        .to_string(),
                ));
            }
        }

        if let Some(timeout_ms) = self.options.timeout_ms.take() {
            if self.timeout_ms == default_provider_timeout_ms() {
                self.timeout_ms = timeout_ms;
            } else if self.timeout_ms != timeout_ms {
                return Err(ConfigError::InvalidReference(
                    "provider openai_compatible.timeout_ms conflicts with provider openai_compatible.options.timeoutMs; use one value"
                        .to_string(),
                ));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct OpenAiCompatibleProviderOptions {
    #[serde(rename = "authProvider", default, alias = "auth_provider")]
    pub auth_provider: Option<AuthProviderId>,
    #[serde(rename = "baseURL", default, alias = "base_url", alias = "baseUrl")]
    pub base_url: Option<String>,
    #[serde(rename = "apiKey", default, alias = "api_key")]
    pub api_key: Option<String>,
    #[serde(
        rename = "apiKeyEnv",
        default,
        alias = "api_key_env",
        alias = "apiKeyEnvironment"
    )]
    pub api_key_env: Vec<String>,
    #[serde(rename = "apiMode", default, alias = "api_mode")]
    pub api_mode: Option<OpenAiApiMode>,
    #[serde(rename = "cacheRetention", default, alias = "cache_retention")]
    pub cache_retention: Option<harness_providers::CacheRetention>,
    #[serde(rename = "timeoutMs", default, alias = "timeout_ms")]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiApiMode {
    Responses,
    ChatCompletions,
    #[default]
    Auto,
}

/// Configuration for an Anthropic Messages API provider.
///
/// Uses `x-api-key` authentication and the Anthropic `/v1/messages` endpoint.
/// The Anthropic transport is implemented in `harness_providers::anthropic`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnthropicProviderConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(
        rename = "baseURL",
        default = "default_anthropic_base_url",
        alias = "base_url",
        alias = "baseUrl"
    )]
    pub base_url: String,
    #[serde(rename = "apiKey", default, alias = "api_key")]
    pub api_key: String,
    #[serde(
        rename = "apiKeyEnv",
        default,
        alias = "api_key_env",
        alias = "apiKeyEnvironment"
    )]
    pub api_key_env: Vec<String>,
    #[serde(
        rename = "timeoutMs",
        default = "default_provider_timeout_ms",
        alias = "timeout_ms"
    )]
    pub timeout_ms: u64,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub options: AnthropicProviderOptions,
    #[serde(default)]
    pub models: BTreeMap<String, ModelConfig>,
}

impl AnthropicProviderConfig {
    fn normalize_public_config_aliases(&mut self) -> Result<(), ConfigError> {
        merge_string_alias(
            &mut self.base_url,
            self.options.base_url.take(),
            "provider anthropic_messages.base_url",
            "provider anthropic_messages.options.baseURL",
        )?;
        merge_string_alias(
            &mut self.api_key,
            self.options.api_key.take(),
            "provider anthropic_messages.api_key",
            "provider anthropic_messages.options.apiKey",
        )?;
        merge_vec_alias(
            &mut self.api_key_env,
            std::mem::take(&mut self.options.api_key_env),
            "provider anthropic_messages.api_key_env",
            "provider anthropic_messages.options.apiKeyEnv",
        )?;
        merge_string_alias(
            &mut self.name,
            self.options.name.take(),
            "provider anthropic_messages.name",
            "provider anthropic_messages.options.name",
        )?;
        merge_map_alias(
            &mut self.headers,
            std::mem::take(&mut self.options.headers),
            "provider anthropic_messages.headers",
            "provider anthropic_messages.options.headers",
        )?;
        if let Some(timeout_ms) = self.options.timeout_ms.take() {
            if self.timeout_ms == default_provider_timeout_ms() {
                self.timeout_ms = timeout_ms;
            } else if self.timeout_ms != timeout_ms {
                return Err(ConfigError::InvalidReference(
                    "provider anthropic_messages.timeout_ms conflicts with provider anthropic_messages.options.timeoutMs; use one value"
                        .to_string(),
                ));
            }
        }
        Ok(())
    }
}

fn default_anthropic_base_url() -> String {
    "https://api.anthropic.com".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct AnthropicProviderOptions {
    #[serde(rename = "baseURL", default, alias = "base_url", alias = "baseUrl")]
    pub base_url: Option<String>,
    #[serde(rename = "apiKey", default, alias = "api_key")]
    pub api_key: Option<String>,
    #[serde(
        rename = "apiKeyEnv",
        default,
        alias = "api_key_env",
        alias = "apiKeyEnvironment"
    )]
    pub api_key_env: Vec<String>,
    #[serde(rename = "timeoutMs", default, alias = "timeout_ms")]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub name: Option<String>,
}
