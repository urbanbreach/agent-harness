use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use thiserror::Error;

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
    #[serde(default = "default_keybindings")]
    pub keybindings: KeybindingsConfig,
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
                    config.api_key = resolve_env_reference(&config.api_key)?;
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
    #[serde(default = "default_openai_api_mode", alias = "apiMode")]
    pub api_mode: OpenAiApiMode,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub models: BTreeMap<String, ModelConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiApiMode {
    Responses,
    ChatCompletions,
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UiConfig {
    #[serde(default)]
    pub theme: UiTheme,
    #[serde(default)]
    pub layout: UiLayoutConfig,
    #[serde(default)]
    pub default_profile: Option<String>,
    #[serde(default = "default_max_events_in_memory")]
    pub max_events_in_memory: usize,
    #[serde(default = "default_max_transcript_chars_in_memory")]
    pub max_transcript_chars_in_memory: usize,
    #[serde(default)]
    pub disable_animations: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: UiTheme::default(),
            layout: UiLayoutConfig::default(),
            default_profile: None,
            max_events_in_memory: default_max_events_in_memory(),
            max_transcript_chars_in_memory: default_max_transcript_chars_in_memory(),
            disable_animations: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiTheme {
    Mono,
    OpencodeDark,
    Default,
}

impl Default for UiTheme {
    fn default() -> Self {
        Self::Default
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UiLayoutConfig {
    #[serde(default = "default_activity_width_pct")]
    pub activity_width_pct: u16,
    #[serde(default = "default_inspector_width_pct")]
    pub inspector_width_pct: u16,
    #[serde(default = "default_input_height_rows")]
    pub input_height_rows: u16,
}

impl Default for UiLayoutConfig {
    fn default() -> Self {
        Self {
            activity_width_pct: default_activity_width_pct(),
            inspector_width_pct: default_inspector_width_pct(),
            input_height_rows: default_input_height_rows(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LoggingConfig {
    #[serde(default = "default_logging_level")]
    pub level: String,
    #[serde(default)]
    pub file: Option<PathBuf>,
    #[serde(default)]
    pub redact: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_logging_level(),
            file: None,
            redact: true,
        }
    }
}

pub type KeybindingsConfig = BTreeMap<String, String>;

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

fn default_openai_api_mode() -> OpenAiApiMode {
    OpenAiApiMode::ChatCompletions
}

fn default_activity_width_pct() -> u16 {
    32
}

fn default_inspector_width_pct() -> u16 {
    38
}

fn default_input_height_rows() -> u16 {
    6
}

fn default_max_events_in_memory() -> usize {
    2_000
}

fn default_max_transcript_chars_in_memory() -> usize {
    2_000_000
}

fn default_logging_level() -> String {
    "info".to_string()
}

fn default_keybindings() -> KeybindingsConfig {
    [
        ("quit", "q"),
        ("focus_next", "tab"),
        ("focus_prev", "shift+tab"),
        ("palette", "ctrl+p"),
        ("help", "?"),
        ("toggle_follow", "f"),
        ("submit_prompt", "enter"),
        ("clear_prompt", "ctrl+u"),
        ("scroll_up", "k"),
        ("scroll_down", "j"),
        ("tab_run", "1"),
        ("tab_events", "2"),
        ("tab_diff", "3"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

fn resolve_env_reference(value: &str) -> Result<String, ConfigError> {
    if !(value.starts_with("${") && value.ends_with('}')) {
        return Ok(value.to_string());
    }

    let key = &value[2..value.len() - 1];
    if key.is_empty() {
        return Ok(value.to_string());
    }

    match env::var(key) {
        Ok(resolved) => Ok(resolved),
        Err(_) => Ok(value.to_string()),
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
              tools: ["read"],
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
}
