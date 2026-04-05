use std::{
    collections::BTreeMap,
    env, fs,
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::tool::ToolSurface;

const CLIPROXY_LOOPBACK_DEFAULT_API_KEY: &str = "sk-zerolimit";

static PROFILE_MODEL_METADATA_REGISTRY: OnceLock<
    Mutex<BTreeMap<String, ResolvedProfileModelMetadata>>,
> = OnceLock::new();
static HOOK_RUNTIME_CONFIG_REGISTRY: OnceLock<Mutex<HookRuntimeConfig>> = OnceLock::new();
static SKILLS_CONFIG_REGISTRY: OnceLock<Mutex<SkillsConfig>> = OnceLock::new();
static LSP_CONFIG_REGISTRY: OnceLock<Mutex<LspConfig>> = OnceLock::new();

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
    #[error("{0}")]
    RetiredConfigKeys(String),
    #[error("missing required config sections: {0}")]
    MissingRequiredSections(String),
    #[error("{0}")]
    UnknownTopLevelKeys(String),
    #[error("environment variable `{0}` referenced in config is not set")]
    MissingEnvironmentVariable(String),
    #[error("{0}")]
    InvalidReference(String),
    #[error("failed to serialize config schema: {0}")]
    SerializeSchema(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HarnessConfig {
    #[serde(rename = "$schema", default)]
    pub schema: Option<String>,
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(rename = "profiles")]
    pub profiles: BTreeMap<String, ProfileConfig>,
    pub permissions: PermissionsConfig,
    pub runtime: RuntimeConfig,
    pub integrations: IntegrationsConfig,
    #[serde(default)]
    pub hooks: HooksConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default)]
    pub lsp: LspConfig,
    #[serde(default)]
    #[serde(skip)]
    #[schemars(skip)]
    pub background_task: BackgroundTaskSettings,
    #[serde(default)]
    #[serde(skip)]
    #[schemars(skip)]
    pub paths: PathsConfig,
    #[serde(default)]
    #[serde(skip)]
    #[schemars(skip)]
    pub deterministic: DeterministicConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiConfig {
    #[serde(default, alias = "defaultProfile")]
    pub default_profile: Option<String>,
    #[serde(default)]
    pub keybindings: BTreeMap<String, String>,
    #[serde(default)]
    pub parity: UiParityConfig,
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
            parity: UiParityConfig::default(),
            max_events_in_memory: default_max_events_in_memory(),
            max_transcript_chars_in_memory: default_max_transcript_chars_in_memory(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiParityConfig {
    #[serde(
        default = "default_ui_variant_cycle_enabled",
        alias = "variantCycleEnabled"
    )]
    pub variant_cycle_enabled: bool,
    #[serde(
        default = "default_ui_child_session_navigation_enabled",
        alias = "childSessionNavigationEnabled"
    )]
    pub child_session_navigation_enabled: bool,
    #[serde(default)]
    pub keybindings: UiParityKeybindingsConfig,
}

impl Default for UiParityConfig {
    fn default() -> Self {
        Self {
            variant_cycle_enabled: default_ui_variant_cycle_enabled(),
            child_session_navigation_enabled: default_ui_child_session_navigation_enabled(),
            keybindings: UiParityKeybindingsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct UiParityKeybindingsConfig {
    #[serde(default, alias = "sessionChildFirst")]
    pub session_child_first: Option<String>,
    #[serde(default, alias = "sessionChildCycle")]
    pub session_child_cycle: Option<String>,
    #[serde(default, alias = "sessionChildCycleReverse")]
    pub session_child_cycle_reverse: Option<String>,
    #[serde(default, alias = "sessionParent")]
    pub session_parent: Option<String>,
    #[serde(default, alias = "variantCycle")]
    pub variant_cycle: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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

fn default_ui_variant_cycle_enabled() -> bool {
    true
}

fn default_ui_child_session_navigation_enabled() -> bool {
    true
}

impl HarnessConfig {
    pub fn apply_session_dir_override(&mut self, session_dir: Option<PathBuf>) {
        if let Some(path) = session_dir {
            self.runtime.session_dir = path.clone();
            self.paths.session_dir = path;
        }
    }

    fn sync_derived_runtime_sections(&mut self) {
        self.background_task = self.runtime.background_tasks.clone();
        self.paths.session_dir = self.runtime.session_dir.clone();
        self.deterministic = self.runtime.deterministic.clone();
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

    fn validate_references(&self) -> Result<(), ConfigError> {
        for (profile_name, profile) in &self.profiles {
            let Some((provider_name, model_name)) = parse_model_ref(&profile.model_ref) else {
                return Err(ConfigError::InvalidReference(format!(
                    "profile `{profile_name}` has invalid `model_ref` `{}`; use `<provider>:<model>`",
                    profile.model_ref
                )));
            };

            let Some(provider) = self.providers.get(provider_name) else {
                return Err(ConfigError::InvalidReference(format!(
                    "profile `{profile_name}` references unknown provider `{provider_name}` in `model_ref` `{}`; available providers: {}",
                    profile.model_ref,
                    format_name_list(self.providers.keys().map(|name| name.as_str()))
                )));
            };

            let models = provider.models();
            let Some(model) = models.get(model_name) else {
                return Err(ConfigError::InvalidReference(format!(
                    "profile `{profile_name}` references unknown model `{model_name}` in `model_ref` `{}`; available models for provider `{provider_name}`: {}",
                    profile.model_ref,
                    format_name_list(models.keys().map(|name| name.as_str()))
                )));
            };

            if let Some(variant_name) = profile.variant.as_deref() {
                let Some(variant) = model.variants.get(variant_name) else {
                    return Err(ConfigError::InvalidReference(format!(
                        "profile `{profile_name}` references unknown variant `{variant_name}` for model `{}`; available variants: {}",
                        profile.model_ref,
                        format_name_list(model.variants.keys().map(|name| name.as_str()))
                    )));
                };

                if variant.disabled {
                    return Err(ConfigError::InvalidReference(format!(
                        "profile `{profile_name}` references disabled variant `{variant_name}` for model `{}`; choose an enabled variant",
                        profile.model_ref
                    )));
                }
            }

            if let Some(target_profile) = profile.exit_target_profile.as_deref() {
                if !self.profiles.contains_key(target_profile) {
                    return Err(ConfigError::InvalidReference(format!(
                        "profile `{profile_name}` references unknown `exit_target_profile` `{target_profile}`; available profiles: {}",
                        format_name_list(self.profiles.keys().map(|name| name.as_str()))
                    )));
                }
            }
        }

        if let Some(default_profile) = self.ui.default_profile.as_deref() {
            if !self.profiles.contains_key(default_profile) {
                return Err(ConfigError::InvalidReference(format!(
                    "ui.default_profile references unknown profile `{default_profile}`; available profiles: {}",
                    format_name_list(self.profiles.keys().map(|name| name.as_str()))
                )));
            }
        }

        self.validate_hook_definitions()?;
        self.validate_skill_roots()?;
        self.validate_lsp_overrides()?;

        Ok(())
    }

    fn validate_hook_definitions(&self) -> Result<(), ConfigError> {
        for (index, hook) in self.hooks.lifecycle.iter().enumerate() {
            if hook.id.as_deref().is_some_and(|id| id.trim().is_empty()) {
                return Err(ConfigError::InvalidReference(format!(
                    "hooks.lifecycle[{index}] for event `{}` must set a non-empty `id` when provided",
                    hook.event.as_str()
                )));
            }

            if hook.command.is_empty() {
                return Err(ConfigError::InvalidReference(format!(
                    "hooks.lifecycle[{index}] for event `{}` must include at least one command token",
                    hook.event.as_str()
                )));
            }

            if hook.command.iter().any(|token| token.trim().is_empty()) {
                return Err(ConfigError::InvalidReference(format!(
                    "hooks.lifecycle[{index}] for event `{}` contains an empty command token; remove blank values",
                    hook.event.as_str()
                )));
            }

            if hook.timeout_ms == 0 {
                return Err(ConfigError::InvalidReference(format!(
                    "hooks.lifecycle[{index}] for event `{}` must set `timeout_ms` to a value greater than 0",
                    hook.event.as_str()
                )));
            }

            if let Some(cwd) = hook.cwd.as_deref() {
                let cwd_path = Path::new(cwd);
                if cwd.trim().is_empty() {
                    return Err(ConfigError::InvalidReference(format!(
                        "hooks.lifecycle[{index}] for event `{}` must set `cwd` to a non-empty relative path when provided",
                        hook.event.as_str()
                    )));
                }

                if cwd_path.is_absolute()
                    || cwd_path
                        .components()
                        .any(|component| matches!(component, Component::ParentDir))
                {
                    return Err(ConfigError::InvalidReference(format!(
                        "hooks.lifecycle[{index}] for event `{}` has invalid `cwd` `{cwd}`; use a workspace-relative path without `..`",
                        hook.event.as_str()
                    )));
                }
            }

            for key in hook.env.keys() {
                if key.trim().is_empty() {
                    return Err(ConfigError::InvalidReference(format!(
                        "hooks.lifecycle[{index}] for event `{}` contains an empty environment variable name",
                        hook.event.as_str()
                    )));
                }
            }
        }

        Ok(())
    }

    fn validate_skill_roots(&self) -> Result<(), ConfigError> {
        for (index, root) in self.skills.project_roots.iter().enumerate() {
            validate_skill_root(root, &format!("skills.project_roots[{index}]"))?;
        }

        for (index, root) in self.skills.global_roots.iter().enumerate() {
            validate_skill_root(root, &format!("skills.global_roots[{index}]"))?;
        }

        for pattern in self.skills.permissions.keys() {
            if pattern.trim().is_empty() {
                return Err(ConfigError::InvalidReference(
                    "skills.permissions contains an empty pattern key; use explicit patterns like `*` or `internal-*`"
                        .to_string(),
                ));
            }
        }

        Ok(())
    }

    fn validate_lsp_overrides(&self) -> Result<(), ConfigError> {
        for (server_name, server) in &self.lsp.servers {
            if server_name.trim().is_empty() {
                return Err(ConfigError::InvalidReference(
                    "lsp.servers contains an empty server key; use a stable id like `rust` or `typescript`"
                        .to_string(),
                ));
            }

            if !is_builtin_lsp_server(server_name) && server.command.is_none() {
                return Err(ConfigError::InvalidReference(format!(
                    "lsp.servers.`{server_name}` must provide `command` for custom local servers"
                )));
            }

            if !is_builtin_lsp_server(server_name) && server.extensions.is_none() {
                return Err(ConfigError::InvalidReference(format!(
                    "lsp.servers.`{server_name}` must provide `extensions` for custom local servers"
                )));
            }

            if let Some(command) = server.command.as_ref() {
                if command.is_empty() {
                    return Err(ConfigError::InvalidReference(format!(
                        "lsp.servers.`{server_name}` must provide at least one command token when `command` is set"
                    )));
                }

                if command.iter().any(|token| token.trim().is_empty()) {
                    return Err(ConfigError::InvalidReference(format!(
                        "lsp.servers.`{server_name}` contains an empty command token; remove blank values"
                    )));
                }
            }

            if let Some(extensions) = server.extensions.as_ref() {
                if extensions.is_empty() {
                    return Err(ConfigError::InvalidReference(format!(
                        "lsp.servers.`{server_name}` must include at least one extension when `extensions` is set"
                    )));
                }

                for extension in extensions {
                    if !(extension.starts_with('.') && extension.len() > 1) {
                        return Err(ConfigError::InvalidReference(format!(
                            "lsp.servers.`{server_name}` has invalid extension `{extension}`; expected values like `.rs` or `.tsx`"
                        )));
                    }
                }
            }

            for key in server.env.keys() {
                if key.trim().is_empty() {
                    return Err(ConfigError::InvalidReference(format!(
                        "lsp.servers.`{server_name}` contains an empty `env` key; use non-empty environment variable names"
                    )));
                }
            }

            if let Some(initialization) = server.initialization.as_ref() {
                if !initialization.is_object() {
                    return Err(ConfigError::InvalidReference(format!(
                        "lsp.servers.`{server_name}` `initialization` must be a JSON object"
                    )));
                }
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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

impl Default for BackgroundTaskSettings {
    fn default() -> Self {
        Self {
            default_concurrency: default_background_task_default_concurrency(),
            provider_concurrency: default_background_task_provider_concurrency(),
            model_concurrency: default_background_task_model_concurrency(),
            stale_timeout_ms: default_background_task_stale_timeout_ms(),
            message_staleness_timeout_ms: default_background_task_message_staleness_timeout_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    #[serde(alias = "backgroundTasks")]
    pub background_tasks: BackgroundTaskSettings,
    #[serde(default = "default_session_dir", alias = "sessionDir")]
    pub session_dir: PathBuf,
    #[serde(default)]
    pub permissions: RuntimePermissionsConfig,
    #[serde(default)]
    pub prompt: PromptRuntimeConfig,
    #[serde(default)]
    pub deterministic: DeterministicConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimePermissionsConfig {
    #[serde(default = "default_runtime_ask_timeout_ms")]
    pub ask_timeout_ms: u64,
}

impl Default for RuntimePermissionsConfig {
    fn default() -> Self {
        Self {
            ask_timeout_ms: default_runtime_ask_timeout_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PromptRuntimeConfig {
    #[serde(default = "default_prompt_wait_timeout_ms")]
    pub wait_timeout_ms: u64,
}

impl Default for PromptRuntimeConfig {
    fn default() -> Self {
        Self {
            wait_timeout_ms: default_prompt_wait_timeout_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct HooksConfig {
    #[serde(default)]
    pub lifecycle: Vec<LifecycleHookConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LifecycleHookConfig {
    #[serde(default, alias = "name")]
    pub id: Option<String>,
    pub event: HookLifecycleEvent,
    pub command: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default = "default_hook_timeout_ms", alias = "timeoutMs")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub critical: bool,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HookLifecycleEvent {
    RunStarted,
    RunFinished,
    RunFailed,
    AgentTurnStarted,
    AgentTurnFinished,
    ToolCallStarted,
    ToolCallFinished,
    ProviderRequestStarted,
    ProviderRequestFinished,
    SubagentSpawned,
    SubagentFinished,
    PermissionRequested,
    PermissionResolved,
}

impl HookLifecycleEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RunStarted => "run_started",
            Self::RunFinished => "run_finished",
            Self::RunFailed => "run_failed",
            Self::AgentTurnStarted => "agent_turn_started",
            Self::AgentTurnFinished => "agent_turn_finished",
            Self::ToolCallStarted => "tool_call_started",
            Self::ToolCallFinished => "tool_call_finished",
            Self::ProviderRequestStarted => "provider_request_started",
            Self::ProviderRequestFinished => "provider_request_finished",
            Self::SubagentSpawned => "subagent_spawned",
            Self::SubagentFinished => "subagent_finished",
            Self::PermissionRequested => "permission_requested",
            Self::PermissionResolved => "permission_resolved",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct HookRuntimeConfig {
    #[serde(default)]
    pub hooks: HooksConfig,
    #[serde(default)]
    pub shell_allowlist: ShellAllowlist,
    #[serde(default, skip)]
    pub suppress_execution: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SkillsConfig {
    #[serde(default, alias = "projectRoots")]
    pub project_roots: Vec<PathBuf>,
    #[serde(default, alias = "globalRoots")]
    pub global_roots: Vec<PathBuf>,
    #[serde(default = "default_skills_walk_to_git_root", alias = "walkToGitRoot")]
    pub walk_to_git_root: bool,
    #[serde(default)]
    pub permissions: BTreeMap<String, PermissionMode>,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            project_roots: default_skills_project_roots(),
            global_roots: default_skills_global_roots(),
            walk_to_git_root: default_skills_walk_to_git_root(),
            permissions: default_skills_permissions(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct LspConfig {
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub servers: BTreeMap<String, LspServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct LspServerConfig {
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub command: Option<Vec<String>>,
    #[serde(default)]
    pub extensions: Option<Vec<String>>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub initialization: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type")]
pub enum ProviderConfig {
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible(OpenAiCompatibleProviderConfig),
}

impl ProviderConfig {
    fn models(&self) -> &BTreeMap<String, ModelConfig> {
        match self {
            Self::OpenAiCompatible(config) => &config.models,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    #[serde(alias = "displayName")]
    pub display_name: String,
    #[serde(default)]
    pub metadata: ModelMetadataConfig,
    #[serde(default, alias = "maxInputTokens")]
    pub max_input_tokens: Option<u32>,
    #[serde(default, alias = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub variants: BTreeMap<String, ModelVariantConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelVariantConfig {
    #[serde(default, alias = "displayName")]
    pub display_name: Option<String>,
    #[serde(default)]
    pub metadata: ModelVariantMetadataConfig,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default, alias = "maxInputTokens")]
    pub max_input_tokens: Option<u32>,
    #[serde(default, alias = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProfileModelMetadata {
    pub profile: String,
    pub provider: String,
    pub model: String,
    pub variant: Option<String>,
    pub display_label: String,
    pub token_window_label: Option<String>,
    pub context_window_tokens: Option<u32>,
    pub max_input_tokens: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub description: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub recommended_for: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ModelMetadataConfig {
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default, alias = "releaseStage")]
    pub release_stage: Option<ModelReleaseStage>,
    #[serde(default, alias = "contextWindowTokens")]
    pub context_window_tokens: Option<u32>,
    #[serde(default, alias = "supportsToolCalls")]
    pub supports_tool_calls: Option<bool>,
    #[serde(default, alias = "supportsReasoningSummaries")]
    pub supports_reasoning_summaries: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelReleaseStage {
    Stable,
    Preview,
    Deprecated,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ModelVariantMetadataConfig {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, alias = "reasoningEffort")]
    pub reasoning_effort: Option<ModelVariantReasoningEffort>,
    #[serde(default, alias = "textVerbosity")]
    pub text_verbosity: Option<ModelVariantTextVerbosity>,
    #[serde(default, alias = "recommendedFor")]
    pub recommended_for: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelVariantReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelVariantTextVerbosity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfileConfig {
    pub description: String,
    #[serde(default, alias = "systemPrompt")]
    pub system_prompt: Option<String>,
    #[serde(rename = "model_ref", alias = "modelRef")]
    pub model_ref: String,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    /// When unset, the runtime omits `temperature` from provider requests so
    /// the provider default applies.
    pub temperature: Option<f32>,
    #[serde(default)]
    pub permissions: Option<ProfilePermissions>,
    #[serde(default, alias = "toolSurface")]
    pub tool_surface: ToolSurface,
    /// Per-profile multi-turn budget enforced directly by the runtime.
    /// There is no separate hardcoded runtime iteration cap beyond this setting.
    #[serde(default = "default_max_iters", alias = "maxIters")]
    pub max_iters: usize,
    #[serde(default, alias = "toolFailureMode")]
    pub tool_failure_mode: ToolFailureMode,
    #[serde(default, alias = "planMode")]
    pub plan_mode: bool,
    #[serde(default, alias = "exitTargetProfile")]
    pub exit_target_profile: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
}

/// Legacy compatibility alias kept for migration shims and older category-named call sites.
pub type CategoryConfig = ProfileConfig;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ProfilePermissions {
    #[serde(default)]
    pub edit: Option<PermissionMode>,
    #[serde(default)]
    pub shell: Option<PermissionMode>,
    #[serde(default)]
    pub network: Option<PermissionMode>,
    #[serde(default)]
    pub question: Option<PermissionMode>,
    #[serde(default)]
    pub task: Option<PermissionMode>,
    #[serde(default, alias = "webFetch")]
    pub webfetch: Option<PermissionMode>,
    #[serde(default, alias = "webSearch")]
    pub websearch: Option<PermissionMode>,
    #[serde(default, alias = "codeSearch")]
    pub codesearch: Option<PermissionMode>,
    #[serde(default, alias = "codeLsp")]
    pub lsp: Option<PermissionMode>,
}

/// Legacy compatibility alias kept for older category-scoped permission call sites.
pub type CategoryPermissions = ProfilePermissions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolFailureMode {
    #[default]
    FailTurn,
    ContinueAsToolMessage,
}

fn default_max_iters() -> usize {
    12
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    Allow,
    Deny,
    Ask,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PermissionsConfig {
    pub defaults: PermissionDefaultsConfig,
    #[serde(rename = "shell_allowlist", alias = "shellAllowlist", default)]
    pub shell_allowlist: ShellAllowlist,
}

impl std::ops::Deref for PermissionsConfig {
    type Target = PermissionDefaultsConfig;

    fn deref(&self) -> &Self::Target {
        &self.defaults
    }
}

impl std::ops::DerefMut for PermissionsConfig {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.defaults
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PermissionDefaultsConfig {
    pub edit: PermissionMode,
    pub shell: PermissionMode,
    pub network: PermissionMode,
    #[serde(default)]
    pub question: Option<PermissionMode>,
    #[serde(default)]
    pub task: Option<PermissionMode>,
    #[serde(default, alias = "webFetch")]
    pub webfetch: Option<PermissionMode>,
    #[serde(default, alias = "webSearch")]
    pub websearch: Option<PermissionMode>,
    #[serde(default, alias = "codeSearch")]
    pub codesearch: Option<PermissionMode>,
    #[serde(default, alias = "codeLsp")]
    pub lsp: Option<PermissionMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ShellAllowlist {
    #[serde(default)]
    pub executables: Vec<String>,
    #[serde(rename = "cwd_roots", alias = "cwdRoots", default)]
    pub cwd_roots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct DeterministicConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub seed: u64,
}

/// Current public integration settings.
///
/// Agent Harness exposes the native remote search transport used by the built-in
/// `web_search` and `code_search` tools, plus configured MCP servers.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct IntegrationsConfig {
    /// Configuration for the currently supported built-in external integrations.
    ///
    /// Agent Harness exposes the native remote search transport used by the
    /// built-in `web_search` and `code_search` tools, plus configured MCP
    /// servers.
    #[serde(default, alias = "remoteSearch")]
    pub remote_search: RemoteSearchConfig,
    #[serde(default)]
    pub mcp: McpConfig,
}

/// Settings for the built-in remote search bridge.
///
/// The current runtime expects an Exa-compatible MCP endpoint for native
/// `web_search` and `code_search` requests.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoteSearchConfig {
    /// Endpoint used by the built-in remote search bridge.
    ///
    /// The current runtime expects an Exa-compatible MCP endpoint for native
    /// `web_search` and `code_search` requests.
    #[serde(default = "default_remote_search_endpoint")]
    pub endpoint: String,
    /// Optional bearer token for the remote search endpoint.
    #[serde(default, alias = "authToken")]
    pub auth_token: Option<String>,
    /// Require an auth token before the native search tools make requests.
    #[serde(default, alias = "requireAuth")]
    pub require_auth: bool,
    /// Request timeout for native remote search calls.
    #[serde(default = "default_remote_search_timeout_secs", alias = "timeoutSecs")]
    pub timeout_secs: u64,
    /// Maximum retry attempts for retryable remote search failures.
    #[serde(default = "default_remote_search_max_retries", alias = "maxRetries")]
    pub max_retries: u32,
    /// Backoff, in milliseconds, between retry attempts.
    #[serde(
        default = "default_remote_search_retry_backoff_ms",
        alias = "retryBackoffMs"
    )]
    pub retry_backoff_ms: u64,
}

impl Default for RemoteSearchConfig {
    fn default() -> Self {
        Self {
            endpoint: default_remote_search_endpoint(),
            auth_token: None,
            require_auth: false,
            timeout_secs: default_remote_search_timeout_secs(),
            max_retries: default_remote_search_max_retries(),
            retry_backoff_ms: default_remote_search_retry_backoff_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: BTreeMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "transport", rename_all = "snake_case", deny_unknown_fields)]
pub enum McpServerConfig {
    Stdio {
        command: Vec<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        #[serde(default)]
        cwd: Option<PathBuf>,
        #[serde(default = "default_mcp_timeout_secs", alias = "timeoutSecs")]
        timeout_secs: u64,
    },
    #[serde(alias = "streamable_http")]
    Http {
        endpoint: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default = "default_mcp_timeout_secs", alias = "timeoutSecs")]
        timeout_secs: u64,
    },
}

impl McpServerConfig {
    pub fn timeout_secs(&self) -> u64 {
        match self {
            Self::Stdio { timeout_secs, .. } | Self::Http { timeout_secs, .. } => *timeout_secs,
        }
    }
}

fn default_session_dir() -> PathBuf {
    PathBuf::from(".agent-harness/sessions")
}

fn default_runtime_ask_timeout_ms() -> u64 {
    30_000
}

fn default_prompt_wait_timeout_ms() -> u64 {
    30_000
}

fn default_hook_timeout_ms() -> u64 {
    5_000
}

fn default_skills_walk_to_git_root() -> bool {
    true
}

fn default_skills_project_roots() -> Vec<PathBuf> {
    vec![
        PathBuf::from(".opencode/skills"),
        PathBuf::from(".claude/skills"),
        PathBuf::from(".agents/skills"),
    ]
}

fn default_skills_global_roots() -> Vec<PathBuf> {
    vec![
        PathBuf::from("~/.config/opencode/skills"),
        PathBuf::from("~/.claude/skills"),
        PathBuf::from("~/.agents/skills"),
    ]
}

fn default_skills_permissions() -> BTreeMap<String, PermissionMode> {
    BTreeMap::from([
        ("*".to_string(), PermissionMode::Allow),
        ("experimental-*".to_string(), PermissionMode::Ask),
        ("internal-*".to_string(), PermissionMode::Deny),
    ])
}

fn default_background_task_default_concurrency() -> usize {
    4
}

fn default_background_task_provider_concurrency() -> usize {
    4
}

fn default_background_task_model_concurrency() -> usize {
    2
}

fn default_background_task_stale_timeout_ms() -> u64 {
    30_000
}

fn default_background_task_message_staleness_timeout_ms() -> u64 {
    10_000
}

fn default_remote_search_endpoint() -> String {
    "https://mcp.exa.ai/mcp".to_string()
}

fn default_remote_search_timeout_secs() -> u64 {
    30
}

fn default_remote_search_max_retries() -> u32 {
    1
}

fn default_remote_search_retry_backoff_ms() -> u64 {
    250
}

fn default_mcp_timeout_secs() -> u64 {
    30
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
        return Ok(env::var(key)
            .ok()
            .filter(|resolved| !resolved.is_empty())
            .unwrap_or_else(|| fallback.to_string()));
    }

    match env::var(reference) {
        Ok(resolved) => Ok(resolved),
        Err(_) => Err(ConfigError::MissingEnvironmentVariable(
            reference.to_string(),
        )),
    }
}

const REQUIRED_CONFIG_SECTIONS: [&str; 5] = [
    "integrations",
    "permissions",
    "profiles",
    "providers",
    "runtime",
];

const ALLOWED_TOP_LEVEL_CONFIG_KEYS: [&str; 11] = [
    "$schema",
    "hooks",
    "integrations",
    "lsp",
    "logging",
    "permissions",
    "profiles",
    "providers",
    "runtime",
    "skills",
    "ui",
];

const RETIRED_TOP_LEVEL_CONFIG_KEYS: [(&str, &str); 4] = [
    (
        "backgroundTask",
        "move its fields under `runtime.background_tasks`",
    ),
    ("categories", "rename it to `profiles`"),
    (
        "deterministic",
        "move its fields under `runtime.deterministic`",
    ),
    ("paths", "move `paths.session_dir` to `runtime.session_dir`"),
];

fn validate_root_config_object(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), ConfigError> {
    let retired = RETIRED_TOP_LEVEL_CONFIG_KEYS
        .iter()
        .filter(|(key, _)| object.contains_key(*key))
        .map(|(key, guidance)| format!("top-level `{key}` was retired; {guidance}"))
        .collect::<Vec<_>>();
    if !retired.is_empty() {
        return Err(ConfigError::RetiredConfigKeys(format!(
            "retired config keys detected: {}",
            retired.join("; ")
        )));
    }

    let mut missing = REQUIRED_CONFIG_SECTIONS
        .iter()
        .copied()
        .filter(|key| !object.contains_key(*key))
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        missing.sort_unstable();
        return Err(ConfigError::MissingRequiredSections(missing.join(", ")));
    }

    let mut unknown = object
        .keys()
        .filter(|key| !ALLOWED_TOP_LEVEL_CONFIG_KEYS.contains(&key.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        unknown.sort_unstable();
        return Err(ConfigError::UnknownTopLevelKeys(format!(
            "unknown top-level config keys: {}; expected only {}",
            format_backticked_list(unknown.iter().copied()),
            format_backticked_list(ALLOWED_TOP_LEVEL_CONFIG_KEYS)
        )));
    }

    Ok(())
}

fn parse_model_ref(model_ref: &str) -> Option<(&str, &str)> {
    let (provider_name, model_name) = model_ref.split_once(':')?;
    if provider_name.is_empty() || model_name.is_empty() {
        return None;
    }
    Some((provider_name, model_name))
}

fn validate_skill_root(root: &Path, location: &str) -> Result<(), ConfigError> {
    if root.as_os_str().is_empty() {
        return Err(ConfigError::InvalidReference(format!(
            "{location} must not be empty; use a normalized relative or absolute skill root"
        )));
    }

    if root
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(ConfigError::InvalidReference(format!(
            "{location} `{}` must not contain `..`; use a normalized skill root path",
            root.display()
        )));
    }

    Ok(())
}

fn format_name_list<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let collected = names.collect::<Vec<_>>();
    if collected.is_empty() {
        "<none>".to_string()
    } else {
        collected.join(", ")
    }
}

fn format_backticked_list<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    items
        .into_iter()
        .map(|item| format!("`{item}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn profile_model_metadata_registry(
) -> &'static Mutex<BTreeMap<String, ResolvedProfileModelMetadata>> {
    PROFILE_MODEL_METADATA_REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn hook_runtime_config_registry() -> &'static Mutex<HookRuntimeConfig> {
    HOOK_RUNTIME_CONFIG_REGISTRY.get_or_init(|| Mutex::new(HookRuntimeConfig::default()))
}

fn skills_config_registry() -> &'static Mutex<SkillsConfig> {
    SKILLS_CONFIG_REGISTRY.get_or_init(|| Mutex::new(SkillsConfig::default()))
}

fn lsp_config_registry() -> &'static Mutex<LspConfig> {
    LSP_CONFIG_REGISTRY.get_or_init(|| Mutex::new(LspConfig::default()))
}

fn with_profile_model_metadata_registry<T>(
    f: impl FnOnce(&mut BTreeMap<String, ResolvedProfileModelMetadata>) -> T,
) -> T {
    match profile_model_metadata_registry().lock() {
        Ok(mut guard) => f(&mut guard),
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            f(&mut guard)
        }
    }
}

fn with_hook_runtime_config_registry<T>(f: impl FnOnce(&mut HookRuntimeConfig) -> T) -> T {
    match hook_runtime_config_registry().lock() {
        Ok(mut guard) => f(&mut guard),
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            f(&mut guard)
        }
    }
}

fn with_skills_config_registry<T>(f: impl FnOnce(&mut SkillsConfig) -> T) -> T {
    match skills_config_registry().lock() {
        Ok(mut guard) => f(&mut guard),
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            f(&mut guard)
        }
    }
}

fn with_lsp_config_registry<T>(f: impl FnOnce(&mut LspConfig) -> T) -> T {
    match lsp_config_registry().lock() {
        Ok(mut guard) => f(&mut guard),
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            f(&mut guard)
        }
    }
}

pub fn refresh_profile_model_metadata_registry(cfg: &HarnessConfig) -> Result<(), ConfigError> {
    let resolved = cfg
        .profiles
        .keys()
        .map(|profile_name| {
            resolve_profile_model_metadata(cfg, profile_name)
                .map(|metadata| (profile_name.clone(), metadata))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    with_profile_model_metadata_registry(|registry| {
        *registry = resolved;
    });

    Ok(())
}

pub fn refresh_hook_runtime_config_registry(cfg: &HarnessConfig) {
    set_registered_hook_runtime_config(HookRuntimeConfig {
        hooks: cfg.hooks.clone(),
        shell_allowlist: cfg.permissions.shell_allowlist.clone(),
        suppress_execution: false,
    });
}

pub fn refresh_skills_config_registry(cfg: &HarnessConfig) {
    with_skills_config_registry(|registered| {
        *registered = cfg.skills.clone();
    });
}

pub fn set_registered_hook_runtime_config(config: HookRuntimeConfig) {
    with_hook_runtime_config_registry(|registered| {
        *registered = config;
    });
}

pub fn registered_hook_runtime_config() -> HookRuntimeConfig {
    with_hook_runtime_config_registry(|registered| registered.clone())
}

pub fn refresh_lsp_config_registry(cfg: &HarnessConfig) {
    set_registered_lsp_config(cfg.lsp.clone());
}

pub fn registered_skills_config() -> SkillsConfig {
    with_skills_config_registry(|registered| registered.clone())
}

pub fn set_registered_lsp_config(config: LspConfig) {
    with_lsp_config_registry(|registered| {
        *registered = config;
    });
}

pub fn registered_lsp_config() -> LspConfig {
    with_lsp_config_registry(|registered| registered.clone())
}

pub fn registered_profile_model_metadata(profile: &str) -> Option<ResolvedProfileModelMetadata> {
    with_profile_model_metadata_registry(|registry| registry.get(profile).cloned())
}

pub fn resolve_profile_model_metadata(
    cfg: &HarnessConfig,
    profile_name: &str,
) -> Result<ResolvedProfileModelMetadata, ConfigError> {
    let profile = cfg.profiles.get(profile_name).ok_or_else(|| {
        ConfigError::InvalidReference(format!(
            "unknown profile `{profile_name}`; available profiles: {}",
            format_name_list(cfg.profiles.keys().map(|name| name.as_str()))
        ))
    })?;

    let Some((provider_name, model_name)) = parse_model_ref(&profile.model_ref) else {
        return Err(ConfigError::InvalidReference(format!(
            "profile `{profile_name}` has invalid `model_ref` `{}`; use `<provider>:<model>`",
            profile.model_ref
        )));
    };

    let provider = cfg.providers.get(provider_name).ok_or_else(|| {
        ConfigError::InvalidReference(format!(
            "profile `{profile_name}` references unknown provider `{provider_name}` in `model_ref` `{}`; available providers: {}",
            profile.model_ref,
            format_name_list(cfg.providers.keys().map(|name| name.as_str()))
        ))
    })?;

    let models = provider.models();
    let model = models.get(model_name).ok_or_else(|| {
        ConfigError::InvalidReference(format!(
            "profile `{profile_name}` references unknown model `{model_name}` in `model_ref` `{}`; available models for provider `{provider_name}`: {}",
            profile.model_ref,
            format_name_list(models.keys().map(|name| name.as_str()))
        ))
    })?;

    let variant = profile.variant.as_deref().map(|variant_name| {
        let variant = model.variants.get(variant_name).ok_or_else(|| {
            ConfigError::InvalidReference(format!(
                "profile `{profile_name}` references unknown variant `{variant_name}` for model `{}`; available variants: {}",
                profile.model_ref,
                format_name_list(model.variants.keys().map(|name| name.as_str()))
            ))
        })?;

        if variant.disabled {
            return Err(ConfigError::InvalidReference(format!(
                "profile `{profile_name}` references disabled variant `{variant_name}` for model `{}`; choose an enabled variant",
                profile.model_ref
            )));
        }

        Ok((variant_name, variant))
    });
    let variant = variant.transpose()?;

    let display_label = build_model_display_label(model, variant);
    let max_input_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.max_input_tokens)
        .or(model.max_input_tokens);
    let max_output_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.max_output_tokens)
        .or(model.max_output_tokens);

    Ok(ResolvedProfileModelMetadata {
        profile: profile_name.to_string(),
        provider: provider_name.to_string(),
        model: model_name.to_string(),
        variant: variant.map(|(variant_name, _)| variant_name.to_string()),
        display_label,
        token_window_label: build_token_window_label(
            model.metadata.context_window_tokens,
            max_input_tokens,
            max_output_tokens,
        ),
        context_window_tokens: model.metadata.context_window_tokens,
        max_input_tokens,
        max_output_tokens,
        description: variant.and_then(|(_, variant_cfg)| variant_cfg.metadata.description.clone()),
        reasoning_effort: variant.and_then(|(_, variant_cfg)| {
            variant_cfg
                .metadata
                .reasoning_effort
                .map(model_variant_reasoning_effort_label)
                .map(str::to_string)
        }),
        text_verbosity: variant.and_then(|(_, variant_cfg)| {
            variant_cfg
                .metadata
                .text_verbosity
                .map(model_variant_text_verbosity_label)
                .map(str::to_string)
        }),
        recommended_for: variant
            .and_then(|(_, variant_cfg)| variant_cfg.metadata.recommended_for.clone()),
    })
}

fn build_model_display_label(
    model: &ModelConfig,
    variant: Option<(&str, &ModelVariantConfig)>,
) -> String {
    let Some((variant_name, variant_cfg)) = variant else {
        return model.display_name.clone();
    };

    let variant_label = variant_cfg.display_name.as_deref().unwrap_or(variant_name);
    format!("{} · {}", model.display_name, variant_label)
}

fn build_token_window_label(
    context_window_tokens: Option<u32>,
    max_input_tokens: Option<u32>,
    max_output_tokens: Option<u32>,
) -> Option<String> {
    let mut segments = Vec::new();

    if let Some(tokens) = context_window_tokens {
        segments.push(format!("{} ctx", compact_token_count(tokens)));
    }
    if let Some(tokens) = max_input_tokens {
        segments.push(format!("{} in", compact_token_count(tokens)));
    }
    if let Some(tokens) = max_output_tokens {
        segments.push(format!("{} out", compact_token_count(tokens)));
    }

    (!segments.is_empty()).then(|| segments.join(" · "))
}

fn compact_token_count(tokens: u32) -> String {
    if tokens >= 1_000 && tokens.is_multiple_of(1_000) {
        format!("{}k", tokens / 1_000)
    } else if tokens >= 1_024 && tokens.is_multiple_of(1_024) {
        format!("{}k", tokens / 1_024)
    } else {
        tokens.to_string()
    }
}

fn model_variant_reasoning_effort_label(effort: ModelVariantReasoningEffort) -> &'static str {
    match effort {
        ModelVariantReasoningEffort::None => "none",
        ModelVariantReasoningEffort::Minimal => "minimal",
        ModelVariantReasoningEffort::Low => "low",
        ModelVariantReasoningEffort::Medium => "medium",
        ModelVariantReasoningEffort::High => "high",
        ModelVariantReasoningEffort::Xhigh => "xhigh",
    }
}

fn model_variant_text_verbosity_label(verbosity: ModelVariantTextVerbosity) -> &'static str {
    match verbosity {
        ModelVariantTextVerbosity::Low => "low",
        ModelVariantTextVerbosity::Medium => "medium",
        ModelVariantTextVerbosity::High => "high",
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
    validate_root_config_object(object)?;

    let mut parsed: HarnessConfig =
        json5::from_str(raw).map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
    parsed.sync_derived_runtime_sections();
    parsed.apply_env_substitutions()?;
    parsed.validate_references()?;
    refresh_hook_runtime_config_registry(&parsed);
    refresh_skills_config_registry(&parsed);
    refresh_lsp_config_registry(&parsed);
    refresh_profile_model_metadata_registry(&parsed)?;
    Ok(parsed)
}

fn is_builtin_lsp_server(name: &str) -> bool {
    matches!(
        name,
        "go" | "json" | "python" | "rust" | "typescript" | "yaml"
    )
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
    use std::{ffi::OsString, sync::Mutex};

    static CONFIG_DISCOVERY_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct DiscoveryTestContext {
        previous_cwd: PathBuf,
        previous_xdg_config_home: Option<OsString>,
    }

    impl DiscoveryTestContext {
        fn new(cwd: &Path, xdg_config_home: Option<&Path>) -> Self {
            let previous_cwd = env::current_dir().expect("capture current dir");
            let previous_xdg_config_home = env::var_os("XDG_CONFIG_HOME");

            env::set_current_dir(cwd).expect("set test current dir");
            match xdg_config_home {
                Some(path) => env::set_var("XDG_CONFIG_HOME", path),
                None => env::remove_var("XDG_CONFIG_HOME"),
            }

            Self {
                previous_cwd,
                previous_xdg_config_home,
            }
        }
    }

    impl Drop for DiscoveryTestContext {
        fn drop(&mut self) {
            env::set_current_dir(&self.previous_cwd).expect("restore current dir");
            match &self.previous_xdg_config_home {
                Some(value) => env::set_var("XDG_CONFIG_HOME", value),
                None => env::remove_var("XDG_CONFIG_HOME"),
            }
        }
    }

    fn config_fixture(
        profiles: &str,
        api_key: &str,
        ui_section: Option<&str>,
        schema: Option<&str>,
    ) -> String {
        let ui_section = ui_section.unwrap_or("");
        let schema_section = schema
            .map(|value| format!(r#""$schema": "{value}","#))
            .unwrap_or_default();

        format!(
            r#"
        {{
          {schema_section}
          providers: {{
            default: {{
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "{api_key}",
              api_mode: "responses",
              timeout_ms: 60000,
              models: {{
                "gpt-4o-mini": {{
                  display_name: "GPT-4o mini",
                }},
              }},
            }},
          }},
          profiles: {{
            {profiles}
          }},
          permissions: {{
            defaults: {{
              edit: "ask",
              shell: "ask",
              network: "deny",
              question: "ask",
              task: "ask",
              webfetch: "deny",
              websearch: "deny",
              codesearch: "deny",
              lsp: "allow",
            }},
            shell_allowlist: {{
              executables: ["git"],
              cwd_roots: ["."],
            }},
          }},
          runtime: {{
            background_tasks: {{
              default_concurrency: 2,
              provider_concurrency: 2,
              model_concurrency: 2,
              stale_timeout_ms: 15000,
              message_staleness_timeout_ms: 5000,
            }},
            session_dir: ".agent-harness/sessions",
            permissions: {{
              ask_timeout_ms: 45000,
            }},
            prompt: {{
              wait_timeout_ms: 15000,
            }},
            deterministic: {{
              enabled: false,
              seed: 42,
            }},
          }},
          integrations: {{
            remote_search: {{
              endpoint: "https://mcp.exa.ai/mcp",
            }},
          }},
          {ui_section}
        }}
        "#,
            schema_section = schema_section,
            api_key = api_key,
            profiles = profiles,
            ui_section = ui_section,
        )
    }

    fn deep_profile(extra_fields: &str) -> String {
        format!(
            r#"
            deep: {{
              description: "Deep work",
              model_ref: "default:gpt-4o-mini",
              {extra_fields}
            }},
            "#,
            extra_fields = extra_fields,
        )
    }

    #[test]
    fn example_config_parses() {
        let profiles = r#"
            deep: {
              description: "Default deep execution profile",
              model_ref: "default:gpt-4o-mini",
              tool_surface: "native",
              tools: ["fs.read"],
            },
            tool_audit: {
              description: "Audit profile",
              model_ref: "default:gpt-4o-mini",
              max_iters: 20,
              tool_failure_mode: "continue_as_tool_message",
              tools: ["fs.read", "tool.invalid"],
            },
            deep_compat: {
              description: "Compat profile",
              model_ref: "default:gpt-4o-mini",
              tool_surface: "compat",
              tools: ["read"],
            },
        "#;

        let text = config_fixture(
            profiles,
            "${OPENAI_API_KEY:-sk-zerolimit}",
            Some(
                r#"
                ui: {
                  default_profile: "deep",
                },
                "#,
            ),
            Some("./harness.schema.json"),
        );
        let parsed = load_config_from_str(&text).expect("fixture config must parse");

        assert_eq!(parsed.schema.as_deref(), Some("./harness.schema.json"));
        assert!(parsed.providers.contains_key("default"));
        assert!(parsed.profiles.contains_key("deep"));
        assert_eq!(parsed.profiles["deep"].tool_surface, ToolSurface::Native);
        assert_eq!(
            parsed.profiles["tool_audit"].tool_failure_mode,
            ToolFailureMode::ContinueAsToolMessage
        );
        assert_eq!(parsed.profiles["tool_audit"].max_iters, 20);
        assert_eq!(
            parsed.profiles["deep_compat"].tool_surface,
            ToolSurface::Compat
        );
        assert_eq!(
            parsed.permissions.defaults.question,
            Some(PermissionMode::Ask)
        );
        assert_eq!(parsed.permissions.defaults.task, Some(PermissionMode::Ask));
        assert_eq!(
            parsed.permissions.defaults.webfetch,
            Some(PermissionMode::Deny)
        );
        assert_eq!(
            parsed.permissions.defaults.websearch,
            Some(PermissionMode::Deny)
        );
        assert_eq!(
            parsed.permissions.defaults.codesearch,
            Some(PermissionMode::Deny)
        );
        assert_eq!(parsed.permissions.defaults.lsp, Some(PermissionMode::Allow));
        assert_eq!(
            parsed.runtime.session_dir,
            PathBuf::from(".agent-harness/sessions")
        );
        assert_eq!(parsed.runtime.permissions.ask_timeout_ms, 45_000);
        assert_eq!(parsed.runtime.prompt.wait_timeout_ms, 15_000);
        assert_eq!(parsed.background_task.default_concurrency, 2);
        assert_eq!(
            parsed.paths.session_dir,
            PathBuf::from(".agent-harness/sessions")
        );
    }

    #[test]
    fn built_in_lsp_presets_accept_override_only_entries() {
        let cfg = r#"
            {
              providers: {
                default: {
                  type: "openai_compatible",
                  base_url: "http://127.0.0.1:8317/v1",
                  api_key: "test-key",
                  models: {
                    "gpt-4o-mini": {
                      display_name: "GPT-4o mini"
                    }
                  }
                }
              },
              profiles: {
                deep: {
                  description: "Deep work",
                  model_ref: "default:gpt-4o-mini",
                  tools: ["fs.read"]
                }
              },
              permissions: {
                defaults: {
                  edit: "ask",
                  shell: "ask",
                  network: "deny"
                }
              },
              runtime: {
                background_tasks: {
                  default_concurrency: 2,
                  provider_concurrency: 2,
                  model_concurrency: 2,
                  stale_timeout_ms: 15000,
                  message_staleness_timeout_ms: 5000
                },
                session_dir: ".agent-harness/sessions",
                deterministic: {
                  enabled: false,
                  seed: 42
                }
              },
              integrations: {
                remote_search: {
                  endpoint: "https://mcp.exa.ai/mcp"
                }
              },
              lsp: {
                servers: {
                  python: {
                    command: ["pyright-langserver", "--stdio"],
                    env: {
                      PYRIGHT_PYTHON_FORCE_VERSION: "latest"
                    }
                  },
                  go: {
                    command: ["gopls"]
                  },
                  yaml: {
                    initialization: {
                      yaml: {
                        keyOrdering: false
                      }
                    }
                  },
                  json: {
                    disabled: true
                  }
                }
              }
            }
        "#;

        let parsed = load_config_from_str(cfg).expect("built-in lsp presets should parse");
        assert!(parsed.lsp.servers.contains_key("python"));
        assert!(parsed.lsp.servers.contains_key("go"));
        assert!(parsed.lsp.servers.contains_key("yaml"));
        assert!(parsed.lsp.servers.contains_key("json"));
    }

    #[test]
    fn missing_required_sections_are_deterministic() {
        let err = load_config_from_str(r#"{"version":1}"#).expect_err("must fail");
        assert_eq!(
            err.to_string(),
            "missing required config sections: integrations, permissions, profiles, providers, runtime"
        );
    }

    #[test]
    fn retired_top_level_keys_fail_with_migration_guidance() {
        let err = load_config_from_str(
            r#"
            {
              categories: {},
              backgroundTask: {},
              paths: {},
              deterministic: {},
              providers: {},
              permissions: {},
              runtime: {},
              integrations: {},
              profiles: {}
            }
            "#,
        )
        .expect_err("retired keys must fail");

        assert_eq!(
            err.to_string(),
            "retired config keys detected: top-level `backgroundTask` was retired; move its fields under `runtime.background_tasks`; top-level `categories` was retired; rename it to `profiles`; top-level `deterministic` was retired; move its fields under `runtime.deterministic`; top-level `paths` was retired; move `paths.session_dir` to `runtime.session_dir`"
        );
    }

    #[test]
    fn unknown_top_level_key_is_rejected_strictly() {
        let cfg = r#"
            {
              extraTopLevel: true,
              providers: {
                default: {
                  type: "openai_compatible",
                  base_url: "http://127.0.0.1:8317/v1",
                  api_key: "test-key",
                  models: {
                    "gpt-4o-mini": {
                      display_name: "GPT-4o mini"
                    }
                  }
                }
              },
              profiles: {
                deep: {
                  description: "Deep work",
                  model_ref: "default:gpt-4o-mini",
                  tools: ["fs.read"]
                }
              },
              permissions: {
                defaults: {
                  edit: "ask",
                  shell: "ask",
                  network: "deny"
                }
              },
              runtime: {
                background_tasks: {
                  default_concurrency: 2,
                  provider_concurrency: 2,
                  model_concurrency: 2,
                  stale_timeout_ms: 15000,
                  message_staleness_timeout_ms: 5000
                },
                session_dir: ".agent-harness/sessions",
                deterministic: {
                  enabled: false,
                  seed: 42
                }
              },
              integrations: {
                remote_search: {
                  endpoint: "https://mcp.exa.ai/mcp"
                }
              }
            }
            "#;

        let err = load_config_from_str(cfg).expect_err("unknown top-level key must fail");
        assert_eq!(
            err.to_string(),
            "unknown top-level config keys: `extraTopLevel`; expected only `$schema`, `hooks`, `integrations`, `lsp`, `logging`, `permissions`, `profiles`, `providers`, `runtime`, `skills`, `ui`"
        );
    }

    #[test]
    fn unknown_nested_key_is_rejected_strictly() {
        let cfg = config_fixture(
            &deep_profile(
                r#"
                tools: ["fs.read"],
                unexpected_profile_field: true,
                "#,
            ),
            "test-key",
            None,
            None,
        );

        let err = load_config_from_str(&cfg).expect_err("unknown nested key must fail");
        assert!(err
            .to_string()
            .contains("unknown field `unexpected_profile_field`"));
    }

    #[test]
    fn profile_model_ref_provider_must_exist() {
        let cfg = config_fixture(
            r#"
            deep: {
              description: "Deep work",
              model_ref: "missing:gpt-4o-mini",
              tools: ["fs.read"],
            },
            "#,
            "test-key",
            None,
            None,
        );

        let err = load_config_from_str(&cfg).expect_err("unknown provider must fail");
        assert_eq!(
            err.to_string(),
            "profile `deep` references unknown provider `missing` in `model_ref` `missing:gpt-4o-mini`; available providers: default"
        );
    }

    #[test]
    fn profile_exit_target_profile_must_exist() {
        let cfg = config_fixture(
            &deep_profile(
                r#"
                exit_target_profile: "ops",
                tools: ["fs.read"],
                "#,
            ),
            "test-key",
            None,
            None,
        );

        let err = load_config_from_str(&cfg).expect_err("unknown exit target profile must fail");
        assert_eq!(
            err.to_string(),
            "profile `deep` references unknown `exit_target_profile` `ops`; available profiles: deep"
        );
    }

    #[test]
    fn ui_default_profile_must_exist() {
        let cfg = config_fixture(
            &deep_profile(r#"tools: ["fs.read"],"#),
            "test-key",
            Some(
                r#"
                ui: {
                  default_profile: "ops",
                },
                "#,
            ),
            None,
        );

        let err = load_config_from_str(&cfg).expect_err("unknown ui default profile must fail");
        assert_eq!(
            err.to_string(),
            "ui.default_profile references unknown profile `ops`; available profiles: deep"
        );
    }

    #[test]
    fn relative_paths_remain_cwd_relative_when_loading_from_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("nested/config.jsonc");
        fs::create_dir_all(config_path.parent().expect("config parent"))
            .expect("create config parent");
        let cfg = r#"
            {
              providers: {
                default: {
                  type: "openai_compatible",
                  base_url: "http://127.0.0.1:8317/v1",
                  api_key: "test-key",
                  models: {
                    "gpt-4o-mini": {
                      display_name: "GPT-4o mini"
                    }
                  }
                }
              },
              profiles: {
                deep: {
                  description: "Deep work",
                  model_ref: "default:gpt-4o-mini",
                  tools: ["fs.read"]
                }
              },
              permissions: {
                defaults: {
                  edit: "ask",
                  shell: "ask",
                  network: "deny"
                }
              },
              runtime: {
                background_tasks: {
                  default_concurrency: 2,
                  provider_concurrency: 2,
                  model_concurrency: 2,
                  stale_timeout_ms: 15000,
                  message_staleness_timeout_ms: 5000
                },
                session_dir: "relative-sessions",
                permissions: {
                  ask_timeout_ms: 45000
                },
                prompt: {
                  wait_timeout_ms: 15000
                },
                deterministic: {
                  enabled: false,
                  seed: 42
                }
              },
              integrations: {
                remote_search: {
                  endpoint: "https://mcp.exa.ai/mcp"
                }
              },
              logging: {
                file: "logs/harness.log"
              }
            }
            "#;
        fs::write(&config_path, cfg).expect("write config");

        let parsed = load_config_from_file(&config_path).expect("config should parse");
        assert_eq!(
            parsed.runtime.session_dir,
            PathBuf::from("relative-sessions")
        );
        assert_eq!(parsed.paths.session_dir, PathBuf::from("relative-sessions"));
        assert_eq!(parsed.logging.file, Some(PathBuf::from("logs/harness.log")));
    }

    #[test]
    fn schema_uses_profiles_runtime_and_integrations_without_categories() {
        let schema = harness_schema_pretty_json().expect("schema generation should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&schema).expect("schema output should be valid json");
        let properties = parsed
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .expect("schema should contain properties");

        assert!(properties.contains_key("$schema"));
        assert!(properties.contains_key("providers"));
        assert!(properties.contains_key("profiles"));
        assert!(properties.contains_key("permissions"));
        assert!(properties.contains_key("runtime"));
        assert!(properties.contains_key("hooks"));
        assert!(properties.contains_key("skills"));
        assert!(properties.contains_key("lsp"));
        assert!(properties.contains_key("integrations"));
        assert!(!properties.contains_key("categories"));
        assert!(schema.contains("\"ask_timeout_ms\""));
        assert!(schema.contains("\"wait_timeout_ms\""));
        assert!(!schema.contains("HARNESS_PROMPT_WAIT_TIMEOUT_MS"));
    }

    #[test]
    fn json5_comments_trailing_commas_and_schema_field_parse() {
        let cfg = r#"
        {
          // optional editor schema hint
          "$schema": "./harness.schema.json",
          providers: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              models: {
                "gpt-4o-mini": {
                  display_name: "GPT-4o mini",
                },
              },
            },
          },
          profiles: {
            deep: {
              description: "Deep work",
              model_ref: "default:gpt-4o-mini",
              tools: ["fs.read",],
            },
          },
          permissions: {
            defaults: {
              edit: "ask",
              shell: "ask",
              network: "deny",
            },
            shell_allowlist: {
              executables: ["git",],
              cwd_roots: [".",],
            },
          },
            runtime: {
              background_tasks: {
                default_concurrency: 2,
                provider_concurrency: 2,
                model_concurrency: 2,
                stale_timeout_ms: 15000,
                message_staleness_timeout_ms: 5000,
              },
              session_dir: ".agent-harness/sessions",
              permissions: {
                ask_timeout_ms: 45000,
              },
              prompt: {
                wait_timeout_ms: 15000,
              },
              deterministic: {
                enabled: false,
                seed: 42,
              },
            },
          integrations: {
            remote_search: {
              endpoint: "https://mcp.exa.ai/mcp",
            },
          },
        }
        "#;

        let parsed = load_config_from_str(cfg).expect("json5 flavored config should parse");
        assert_eq!(parsed.schema.as_deref(), Some("./harness.schema.json"));
        assert_eq!(parsed.profiles["deep"].model_ref, "default:gpt-4o-mini");
    }

    #[test]
    fn resolve_config_path_prefers_explicit_path_over_discovery() {
        let _lock = CONFIG_DISCOVERY_TEST_LOCK
            .lock()
            .expect("lock discovery tests");
        let temp = tempfile::tempdir().expect("tempdir");
        let xdg_root = temp.path().join("xdg");
        let xdg_config = xdg_root.join("harness/config.jsonc");
        let cwd_config = temp.path().join("harness.jsonc");
        let explicit_config = temp.path().join("explicit.jsonc");

        fs::create_dir_all(xdg_config.parent().expect("xdg parent"))
            .expect("create xdg config dir");
        fs::write(&xdg_config, "xdg").expect("write xdg config");
        fs::write(&cwd_config, "cwd").expect("write cwd config");
        fs::write(&explicit_config, "explicit").expect("write explicit config");

        let _context = DiscoveryTestContext::new(temp.path(), Some(&xdg_root));

        assert_eq!(
            resolve_config_path(Some(&explicit_config)),
            Some(explicit_config)
        );
    }

    #[test]
    fn resolve_config_path_prefers_cwd_harness_jsonc_over_xdg_config() {
        let _lock = CONFIG_DISCOVERY_TEST_LOCK
            .lock()
            .expect("lock discovery tests");
        let temp = tempfile::tempdir().expect("tempdir");
        let xdg_root = temp.path().join("xdg");
        let xdg_config = xdg_root.join("harness/config.jsonc");
        let cwd_config = temp.path().join("harness.jsonc");

        fs::create_dir_all(xdg_config.parent().expect("xdg parent"))
            .expect("create xdg config dir");
        fs::write(&xdg_config, "xdg").expect("write xdg config");
        fs::write(&cwd_config, "cwd").expect("write cwd config");

        let _context = DiscoveryTestContext::new(temp.path(), Some(&xdg_root));

        assert_eq!(
            resolve_config_path(None),
            Some(PathBuf::from("harness.jsonc"))
        );
    }

    #[test]
    fn env_var_substitution_works() {
        let expected = env::var("PATH").expect("PATH must exist in test environment");
        let cfg = config_fixture(
            &deep_profile(
                r#"
                system_prompt: "Be precise.",
                tool_failure_mode: "continue_as_tool_message",
                tools: ["fs.read"],
                "#,
            ),
            "${PATH}",
            None,
            None,
        );

        let parsed = load_config_from_str(&cfg).expect("config with env reference must parse");
        let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
        assert_eq!(provider.api_key, expected);
    }

    #[test]
    fn ui_default_profile_parses() {
        let cfg = config_fixture(
            &deep_profile(r#"tools: ["fs.read"],"#),
            "test-key",
            Some(
                r#"
                ui: {
                  defaultProfile: "deep",
                },
                "#,
            ),
            None,
        );

        let parsed = load_config_from_str(&cfg).expect("config with ui.defaultProfile must parse");
        assert_eq!(parsed.ui.default_profile, Some("deep".to_string()));
    }

    #[test]
    fn ui_default_profile_defaults_to_none() {
        let cfg = config_fixture(
            &deep_profile(r#"tools: ["fs.read"],"#),
            "test-key",
            None,
            None,
        );

        let parsed = load_config_from_str(&cfg).expect("config without ui section must parse");
        assert_eq!(parsed.ui.default_profile, None);
    }

    #[test]
    fn profile_tool_surface_defaults_to_native_when_omitted() {
        let cfg = config_fixture(
            &deep_profile(r#"tools: ["fs.read"],"#),
            "test-key",
            None,
            None,
        );

        let parsed =
            load_config_from_str(&cfg).expect("config with default tool surface must parse");
        assert_eq!(parsed.profiles["deep"].tool_surface, ToolSurface::Native);
    }

    #[test]
    fn profile_tool_surface_parses_compat_explicitly() {
        let cfg = config_fixture(
            &deep_profile(
                r#"
                tool_surface: "compat",
                tools: ["read"],
                "#,
            ),
            "test-key",
            None,
            None,
        );

        let parsed =
            load_config_from_str(&cfg).expect("config with compat tool surface must parse");
        assert_eq!(parsed.profiles["deep"].tool_surface, ToolSurface::Compat);
    }

    #[test]
    fn profile_tool_failure_mode_defaults_to_fail_turn() {
        let cfg = config_fixture(
            &deep_profile(r#"tools: ["fs.read"],"#),
            "test-key",
            None,
            None,
        );

        let parsed =
            load_config_from_str(&cfg).expect("config with default tool failure mode must parse");
        assert_eq!(parsed.profiles["deep"].max_iters, default_max_iters());
        assert_eq!(
            parsed.profiles["deep"].tool_failure_mode,
            ToolFailureMode::FailTurn
        );
    }

    #[test]
    fn profile_tool_failure_mode_and_system_prompt_parse_explicitly() {
        let cfg = config_fixture(
            &deep_profile(
                r#"
                system_prompt: "Be precise.",
                max_iters: 24,
                tool_failure_mode: "continue_as_tool_message",
                tools: ["fs.read"],
                "#,
            ),
            "test-key",
            None,
            None,
        );

        let parsed = load_config_from_str(&cfg)
            .expect("config with explicit tool failure mode and prompt must parse");
        assert_eq!(
            parsed.profiles["deep"].tool_failure_mode,
            ToolFailureMode::ContinueAsToolMessage
        );
        assert_eq!(parsed.profiles["deep"].max_iters, 24);
        assert_eq!(
            parsed.profiles["deep"].system_prompt.as_deref(),
            Some("Be precise.")
        );
    }

    #[test]
    fn env_var_default_fallback_works() {
        let cfg = config_fixture(
            &deep_profile(r#"tools: ["fs.read"],"#),
            "${HARNESS_CONFIG_TEST_API_KEY_FALLBACK:-fallback-key}",
            None,
            None,
        );

        let parsed =
            load_config_from_str(&cfg).expect("config with fallback env reference must parse");
        let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
        assert_eq!(provider.api_key, "fallback-key");
    }

    #[test]
    fn empty_env_var_uses_default_fallback() {
        let cfg = config_fixture(
            &deep_profile(r#"tools: ["fs.read"],"#),
            "${HARNESS_CONFIG_TEST_API_KEY_EMPTY:-fallback-key}",
            None,
            None,
        );

        env::set_var("HARNESS_CONFIG_TEST_API_KEY_EMPTY", "");

        let parsed = load_config_from_str(&cfg)
            .expect("config with empty env reference should use fallback value");
        let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
        assert_eq!(provider.api_key, "fallback-key");

        env::remove_var("HARNESS_CONFIG_TEST_API_KEY_EMPTY");
    }

    #[test]
    fn missing_required_env_var_is_an_error() {
        let cfg = config_fixture(
            &deep_profile(r#"tools: ["fs.read"],"#),
            "${HARNESS_CONFIG_TEST_API_KEY_REQUIRED}",
            None,
            None,
        );

        let err =
            load_config_from_str(&cfg).expect_err("missing required env variable should fail");
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
