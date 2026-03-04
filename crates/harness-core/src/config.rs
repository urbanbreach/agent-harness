use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const CLIPROXY_LOOPBACK_DEFAULT_API_KEY: &str = "sk-zerolimit";

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to read config file {path}: {source}")]
    ReadFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config JSON5: {0}")]
    ParseJson5(String),
    #[error("config must be a JSON object at top level")]
    InvalidRootObject,
    #[error("missing required config sections: {0}")]
    MissingRequiredSections(String),
    #[error("environment variable `{0}` referenced in config is not set")]
    MissingEnvironmentVariable(String),
    #[error("failed to serialize config schema: {0}")]
    SerializeSchema(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HarnessConfig {
    #[serde(rename = "backgroundTask", alias = "background_task")]
    pub background_task: BackgroundTaskSettings,
    pub providers: BTreeMap<String, ProviderConfig>,
    pub categories: BTreeMap<String, CategoryConfig>,
    pub permissions: PermissionsConfig,
    #[serde(default)]
    pub paths: PathsConfig,
    #[serde(default)]
    pub deterministic: DeterministicConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UiConfig {
    #[serde(default, alias = "defaultProfile")]
    pub default_profile: Option<String>,
    #[serde(default)]
    pub keybindings: BTreeMap<String, String>,
    #[serde(
        rename = "maxEventsInMemory",
        alias = "max_events_in_memory",
        default = "default_max_events_in_memory"
    )]
    pub max_events_in_memory: usize,
    #[serde(
        rename = "maxTranscriptCharsInMemory",
        alias = "max_transcript_chars_in_memory",
        default = "default_max_transcript_chars_in_memory"
    )]
    pub max_transcript_chars_in_memory: usize,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            default_profile: None,
            keybindings: BTreeMap::new(),
            max_events_in_memory: default_max_events_in_memory(),
            max_transcript_chars_in_memory: default_max_transcript_chars_in_memory(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LoggingConfig {
    #[serde(default = "default_logging_level")]
    pub level: String,
    #[serde(default)]
    pub file: Option<PathBuf>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_logging_level(),
            file: None,
        }
    }
}

fn default_logging_level() -> String {
    "info".to_string()
}

fn default_max_events_in_memory() -> usize {
    25_000
}

fn default_max_transcript_chars_in_memory() -> usize {
    200_000
}

impl HarnessConfig {
    pub fn apply_session_dir_override(&mut self, session_dir: Option<PathBuf>) {
        if let Some(path) = session_dir {
            self.paths.session_dir = path;
        }
    }

    fn apply_env_substitutions(&mut self) -> Result<(), ConfigError> {
        for provider in self.providers.values_mut() {
            match provider {
                ProviderConfig::OpenAiCompatible(config) => {
                    config.api_key =
                        resolve_openai_compatible_api_key(&config.api_key, &config.base_url)?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundTaskSettings {
    #[serde(rename = "defaultConcurrency", alias = "default_concurrency")]
    pub default_concurrency: usize,
    #[serde(rename = "providerConcurrency", alias = "provider_concurrency")]
    pub provider_concurrency: usize,
    #[serde(rename = "modelConcurrency", alias = "model_concurrency")]
    pub model_concurrency: usize,
    #[serde(rename = "staleTimeoutMs", alias = "stale_timeout_ms")]
    pub stale_timeout_ms: u64,
    #[serde(
        rename = "messageStalenessTimeoutMs",
        alias = "message_staleness_timeout_ms"
    )]
    pub message_staleness_timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum ProviderConfig {
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible(OpenAiCompatibleProviderConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OpenAiCompatibleProviderConfig {
    #[serde(alias = "baseUrl")]
    pub base_url: String,
    #[serde(alias = "apiKey")]
    pub api_key: String,
    #[serde(default = "default_provider_timeout_ms", alias = "timeoutMs")]
    pub timeout_ms: u64,
    #[serde(default, alias = "apiMode")]
    pub api_mode: OpenAiApiMode,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub models: BTreeMap<String, ModelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiApiMode {
    Responses,
    ChatCompletions,
    #[default]
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModelConfig {
    #[serde(alias = "displayName")]
    pub display_name: String,
    #[serde(default, alias = "maxInputTokens")]
    pub max_input_tokens: Option<u32>,
    #[serde(default, alias = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub variants: BTreeMap<String, ModelVariantConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModelVariantConfig {
    #[serde(default, alias = "displayName")]
    pub display_name: Option<String>,
    #[serde(default, alias = "maxInputTokens")]
    pub max_input_tokens: Option<u32>,
    #[serde(default, alias = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CategoryConfig {
    pub description: String,
    #[serde(rename = "model_ref", alias = "modelRef")]
    pub model_ref: String,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub permissions: Option<CategoryPermissions>,
    #[serde(default)]
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct CategoryPermissions {
    #[serde(default)]
    pub edit: Option<PermissionMode>,
    #[serde(default)]
    pub shell: Option<PermissionMode>,
    #[serde(default)]
    pub network: Option<PermissionMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    Allow,
    Deny,
    Ask,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PermissionsConfig {
    pub edit: PermissionMode,
    pub shell: PermissionMode,
    pub network: PermissionMode,
    #[serde(rename = "shell_allowlist", alias = "shellAllowlist", default)]
    pub shell_allowlist: ShellAllowlist,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct ShellAllowlist {
    #[serde(default)]
    pub executables: Vec<String>,
    #[serde(rename = "cwd_roots", alias = "cwdRoots", default)]
    pub cwd_roots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PathsConfig {
    #[serde(default = "default_session_dir", alias = "sessionDir")]
    pub session_dir: PathBuf,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            session_dir: default_session_dir(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct DeterministicConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub seed: u64,
}

fn default_session_dir() -> PathBuf {
    PathBuf::from(".agent-harness/sessions")
}

fn default_provider_timeout_ms() -> u64 {
    60_000
}

fn is_loopback_cliproxy_base_url(base_url: &str) -> bool {
    let lowered = base_url.trim().to_ascii_lowercase();
    lowered.contains("127.0.0.1:8317")
        || lowered.contains("localhost:8317")
        || lowered.contains("[::1]:8317")
}

fn resolve_openai_compatible_api_key(api_key: &str, base_url: &str) -> Result<String, ConfigError> {
    apply_cliproxy_loopback_openai_fallback(resolve_env_reference(api_key), base_url)
}

fn apply_cliproxy_loopback_openai_fallback(
    resolved_api_key: Result<String, ConfigError>,
    base_url: &str,
) -> Result<String, ConfigError> {
    match resolved_api_key {
        Ok(api_key) => Ok(api_key),
        Err(ConfigError::MissingEnvironmentVariable(missing_env))
            if missing_env == "OPENAI_API_KEY" && is_loopback_cliproxy_base_url(base_url) =>
        {
            Ok(CLIPROXY_LOOPBACK_DEFAULT_API_KEY.to_string())
        }
        Err(err) => Err(err),
    }
}

fn resolve_env_reference(value: &str) -> Result<String, ConfigError> {
    if !(value.starts_with("${") && value.ends_with('}')) {
        return Ok(value.to_string());
    }

    let reference = &value[2..value.len() - 1];
    if reference.is_empty() {
        return Ok(value.to_string());
    }

    if let Some((key, fallback)) = reference.split_once(":-") {
        if key.is_empty() {
            return Ok(value.to_string());
        }
        return Ok(env::var(key).unwrap_or_else(|_| fallback.to_string()));
    }

    match env::var(reference) {
        Ok(resolved) => Ok(resolved),
        Err(_) => Err(ConfigError::MissingEnvironmentVariable(
            reference.to_string(),
        )),
    }
}

pub fn load_config_from_file(path: &Path) -> Result<HarnessConfig, ConfigError> {
    let raw = fs::read_to_string(path).map_err(|source| ConfigError::ReadFile {
        path: path.display().to_string(),
        source,
    })?;

    load_config_from_str(&raw)
}

pub fn load_config_from_str(raw: &str) -> Result<HarnessConfig, ConfigError> {
    let root: serde_json::Value =
        json5::from_str(raw).map_err(|err| ConfigError::ParseJson5(err.to_string()))?;

    let object = root.as_object().ok_or(ConfigError::InvalidRootObject)?;
    let mut missing = Vec::new();

    for key in ["backgroundTask", "providers", "categories", "permissions"] {
        if !object.contains_key(key) {
            missing.push(key);
        }
    }

    if !missing.is_empty() {
        missing.sort_unstable();
        return Err(ConfigError::MissingRequiredSections(missing.join(", ")));
    }

    let mut parsed: HarnessConfig =
        json5::from_str(raw).map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
    parsed.apply_env_substitutions()?;
    Ok(parsed)
}

pub fn harness_schema_pretty_json() -> Result<String, ConfigError> {
    let schema = schema_for!(HarnessConfig);
    serde_json::to_string_pretty(&schema)
        .map_err(|err| ConfigError::SerializeSchema(err.to_string()))
}

pub fn resolve_config_path(explicit_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit_path {
        return Some(path.to_path_buf());
    }

    let cwd_candidate = PathBuf::from("harness.jsonc");
    if cwd_candidate.exists() {
        return Some(cwd_candidate);
    }

    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));

    let candidate = base.map(|base| base.join("harness").join("config.jsonc"));
    candidate.filter(|path| path.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_parses() {
        let text = include_str!("../../../configs/harness.example.jsonc");
        let parsed = load_config_from_str(text).expect("example config must parse");

        assert!(parsed.providers.contains_key("default"));
        assert!(parsed.categories.contains_key("deep"));
        assert_eq!(
            parsed.paths.session_dir,
            PathBuf::from(".agent-harness/sessions")
        );
    }

    #[test]
    fn missing_required_sections_are_deterministic() {
        let err = load_config_from_str(r#"{"version":1}"#).expect_err("must fail");
        assert_eq!(
            err.to_string(),
            "missing required config sections: backgroundTask, categories, permissions, providers"
        );
    }

    #[test]
    fn env_var_substitution_works() {
        let expected = env::var("PATH").expect("PATH must exist in test environment");
        let cfg = r#"
        {
          backgroundTask: {
            defaultConcurrency: 2,
            providerConcurrency: 2,
            modelConcurrency: 2,
            staleTimeoutMs: 15000,
            messageStalenessTimeoutMs: 5000,
          },
          providers: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "${PATH}",
              api_mode: "auto",
              timeout_ms: 60000,
              models: {
                "gpt-4o-mini": {
                  display_name: "GPT-4o mini",
                },
              },
            },
          },
          categories: {
            deep: {
              description: "Deep work",
              model_ref: "default:gpt-4o-mini",
              tools: ["fs.read"],
            },
          },
          permissions: {
            edit: "ask",
            shell: "ask",
            network: "deny",
          },
        }
        "#;

        let parsed = load_config_from_str(cfg).expect("config with env reference must parse");
        let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
        assert_eq!(provider.api_key, expected);
    }

    #[test]
    fn env_var_default_fallback_works() {
        let cfg = r#"
        {
          backgroundTask: {
            defaultConcurrency: 2,
            providerConcurrency: 2,
            modelConcurrency: 2,
            staleTimeoutMs: 15000,
            messageStalenessTimeoutMs: 5000,
          },
          providers: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "${HARNESS_CONFIG_TEST_API_KEY_FALLBACK:-fallback-key}",
              api_mode: "responses",
              timeout_ms: 60000,
              models: {
                "gpt-4o-mini": {
                  display_name: "GPT-4o mini",
                },
              },
            },
          },
          categories: {
            deep: {
              description: "Deep work",
              model_ref: "default:gpt-4o-mini",
              tools: ["fs.read"],
            },
          },
          permissions: {
            edit: "ask",
            shell: "ask",
            network: "deny",
          },
        }
        "#;

        let parsed =
            load_config_from_str(cfg).expect("config with fallback env reference must parse");
        let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
        assert_eq!(provider.api_key, "fallback-key");
    }

    #[test]
    fn missing_required_env_var_is_an_error() {
        let cfg = r#"
        {
          backgroundTask: {
            defaultConcurrency: 2,
            providerConcurrency: 2,
            modelConcurrency: 2,
            staleTimeoutMs: 15000,
            messageStalenessTimeoutMs: 5000,
          },
          providers: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "${HARNESS_CONFIG_TEST_API_KEY_REQUIRED}",
              timeout_ms: 60000,
              models: {
                "gpt-4o-mini": {
                  display_name: "GPT-4o mini",
                },
              },
            },
          },
          categories: {
            deep: {
              description: "Deep work",
              model_ref: "default:gpt-4o-mini",
              tools: ["fs.read"],
            },
          },
          permissions: {
            edit: "ask",
            shell: "ask",
            network: "deny",
          },
        }
        "#;

        let err = load_config_from_str(cfg).expect_err("missing required env variable should fail");
        assert_eq!(
            err.to_string(),
            "environment variable `HARNESS_CONFIG_TEST_API_KEY_REQUIRED` referenced in config is not set"
        );
    }

    #[test]
    fn missing_openai_api_key_uses_cliproxy_loopback_fallback() {
        let resolved = apply_cliproxy_loopback_openai_fallback(
            Err(ConfigError::MissingEnvironmentVariable(
                "OPENAI_API_KEY".to_string(),
            )),
            "http://127.0.0.1:8317/v1",
        )
        .expect("local CLIProxy should use the default subscription placeholder key");

        assert_eq!(resolved, CLIPROXY_LOOPBACK_DEFAULT_API_KEY);
    }

    #[test]
    fn missing_openai_api_key_still_errors_for_non_cliproxy_base_url() {
        let err = apply_cliproxy_loopback_openai_fallback(
            Err(ConfigError::MissingEnvironmentVariable(
                "OPENAI_API_KEY".to_string(),
            )),
            "https://api.openai.com/v1",
        )
        .expect_err(
            "non-local endpoints should still require OPENAI_API_KEY when no fallback is set",
        );
        assert_eq!(
            err.to_string(),
            "environment variable `OPENAI_API_KEY` referenced in config is not set"
        );
    }
}
