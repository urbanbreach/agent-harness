// allow: SIZE_OK — config discovery and loading (path resolution + merge)
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
};

use crate::auth::ProviderId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

mod aliases;
mod defaults;
mod discovery;
mod integrations;
mod loader;
mod model_alias;
mod model_catalog;
mod model_selection;
mod model_types;
mod provider;
mod public;
mod registries;
mod settings_registry;
mod settings_write;
mod validation;

use self::defaults::{
    default_background_task_default_concurrency,
    default_background_task_message_staleness_timeout_ms,
    default_background_task_model_concurrency, default_background_task_provider_concurrency,
    default_background_task_stale_timeout_ms, default_compaction_auto_retry_overflow,
    default_compaction_enabled, default_compaction_estimated_token_triggers,
    default_compaction_fallback_input_tokens, default_compaction_keep_recent_tokens,
    default_compaction_reserve_tokens, default_compaction_structured_summary_contract,
    default_hashline_edit, default_hook_timeout_ms, default_logging_level,
    default_max_events_in_memory, default_max_transcript_chars_in_memory,
    default_prompt_wait_timeout_ms, default_provider_retry_base_delay_ms,
    default_provider_retry_max_delay_ms, default_provider_retry_max_retries,
    default_runtime_ask_timeout_ms, default_runtime_tool_failure_mode, default_session_dir,
    default_skills_global_roots, default_skills_permissions, default_skills_project_roots,
    default_skills_walk_to_git_root,
};
pub use self::defaults::{
    DEFAULT_REMOTE_SEARCH_ENDPOINT, DEFAULT_REMOTE_SEARCH_MAX_RETRIES,
    DEFAULT_REMOTE_SEARCH_RETRY_BACKOFF_MS, DEFAULT_REMOTE_SEARCH_TIMEOUT_SECS,
};
pub use self::discovery::{
    resolve_config_layer_paths, resolve_config_path, resolve_config_path_with_context,
    ConfigDiscoveryContext,
};
pub use self::integrations::{
    IntegrationsConfig, McpConfig, McpServerConfig, McpServerConnectionState, RemoteSearchConfig,
};
pub use self::loader::{
    load_config_from_file, load_config_from_file_with_context, load_config_from_str,
    load_resolved_config, load_resolved_config_with_context, ConfigLoadContext, LoadedConfig,
};
pub use self::model_alias::{is_model_alias, known_model_aliases, resolve_model_alias, ModelAlias};
pub use self::model_catalog::{
    configured_model_catalog, configured_model_profile_catalog, resolve_configured_model_metadata,
    resolve_profile_model_metadata,
};
pub use self::model_selection::resolve_model_selection;
use self::model_selection::{resolve_agent_model_selection, resolve_named_model_profile};
pub use self::model_types::{
    ModelConfig, ModelLimitConfig, ModelMetadataConfig, ModelModalitiesConfig, ModelProfileConfig,
    ModelProfileTargetConfig, ModelReleaseStage, ModelVariantConfig, ModelVariantMetadataConfig,
    ModelVariantReasoningEffort, ModelVariantTextVerbosity, ResolvedModelCatalogEntry,
    ResolvedModelProfileCatalogEntry, ResolvedModelSelection, ResolvedModelTarget,
    ResolvedProfileModelMetadata,
};
pub use self::provider::{
    AnthropicProviderConfig, OpenAiApiMode, OpenAiCompatibleProviderConfig,
    OpenAiCompatibleProviderOptions, ProviderConfig,
};
pub use self::public::{
    harness_schema_pretty_json, harness_tui_schema_pretty_json, public_config_contract,
    InstructionList, PublicAgentConfig, PublicAgentTools, PublicConfigAlias,
    PublicConfigAliasScope, PublicConfigCompactionKnob, PublicConfigContract,
    PublicConfigKeyStatus, PublicConfigPermissionName, PublicConfigSurface,
    PublicConfigTopLevelKey, PublicPermissionConfig, PublicPermissionValue,
    PublicProfilePermissions, PublicRulePermissionValue, PublicRuntimeConfig, PublicTuiConfig,
    PublicUnsupportedInactiveValue,
};
pub use self::registries::{
    clear_registered_integrations_config, clear_registered_mcp_server_connection_states,
    clear_registered_mcp_server_first_class_tool_ids, refresh_hook_runtime_config_registry,
    refresh_integrations_config_registry, refresh_lsp_config_registry,
    refresh_profile_model_metadata_registry, refresh_skills_config_registry,
    registered_formatter_config, registered_hook_runtime_config, registered_integrations_config,
    registered_lsp_config, registered_mcp_server_connection_state,
    registered_mcp_server_first_class_tool_id, registered_profile_model_metadata,
    registered_skills_config, set_registered_formatter_config, set_registered_hook_runtime_config,
    set_registered_integrations_config, set_registered_lsp_config,
    set_registered_mcp_server_connection_states, set_registered_mcp_server_first_class_tool_ids,
};
pub use self::settings_registry::{
    explain_setting, is_metadata_only_setting, resolve_setting_id, setting_definition,
    settings_compat_migrations, settings_registry, settings_registry_json,
    summarize_settings_registry, SchemaId, SettingCompatMigration, SettingDefinition, SettingId,
    SettingMergeStrategy, SettingMutability, SettingScope, SettingSensitivity,
    SettingSourceExplanation, SettingSurface, SettingsRegistrySummary,
};
pub use self::settings_write::{
    read_effective_compaction_auto_retry_overflow, read_effective_compaction_enabled,
    read_effective_compaction_estimated_token_triggers,
    read_effective_compaction_structured_summary_contract, read_effective_deterministic_enabled,
    read_effective_hashline_edit, reset_project_compaction_auto_retry_overflow,
    reset_project_compaction_enabled, reset_project_compaction_estimated_token_triggers,
    reset_project_compaction_structured_summary_contract, reset_project_deterministic_enabled,
    reset_project_hashline_edit, reset_project_setting_to_default,
    write_project_compaction_auto_retry_overflow, write_project_compaction_enabled,
    write_project_compaction_estimated_token_triggers,
    write_project_compaction_structured_summary_contract, write_project_deterministic_enabled,
    write_project_hashline_edit, write_project_setting_bool, SettingWriteError,
};
use self::validation::{
    is_blank_config_value, validate_hook_definitions, validate_lsp_overrides, validate_mcp_servers,
    validate_skill_roots,
};

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
    #[error("failed to read markdown asset {path}: {source}")]
    ReadMarkdownAsset {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid markdown frontmatter in {path}: {reason}")]
    InvalidMarkdownFrontmatter { path: String, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InstructionFile {
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HarnessConfig {
    #[serde(rename = "$schema", default)]
    pub schema: Option<String>,
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub disabled_providers: Vec<String>,
    #[serde(default)]
    pub enabled_providers: Vec<String>,
    #[serde(
        rename = "model_profile",
        default,
        alias = "modelProfile",
        alias = "model_profiles"
    )]
    pub model_profiles: BTreeMap<String, ModelProfileConfig>,
    #[serde(default)]
    pub agents: BTreeMap<String, ProfileConfig>,
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
    #[serde(default = "default_hashline_edit", alias = "hashlineEdit")]
    pub hashline_edit: bool,
    #[serde(default)]
    pub formatter: FormatterConfig,
    #[serde(default)]
    #[serde(skip)]
    #[schemars(skip)]
    pub instruction_files: Vec<InstructionFile>,
    #[serde(default)]
    #[serde(skip)]
    #[schemars(skip)]
    pub small_model: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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

/// Runtime formatter configuration.
///
/// `deny_unknown_fields` is intentionally omitted because `#[serde(flatten)]`
/// on the per-formatter overrides map allows arbitrary formatter-name keys in
/// the public object form; serde would otherwise reject those keys as unknown.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FormatterConfig {
    #[serde(default = "default_formatter_enabled")]
    pub enabled: bool,
    #[serde(default, alias = "experimentalOxfmt")]
    pub experimental_oxfmt: bool,
    #[serde(flatten, default)]
    pub overrides: BTreeMap<String, FormatterOverride>,
}

impl Default for FormatterConfig {
    fn default() -> Self {
        Self {
            enabled: default_formatter_enabled(),
            experimental_oxfmt: false,
            overrides: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FormatterOverride {
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub command: Option<Vec<String>>,
    #[serde(default)]
    pub environment: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub extensions: Option<Vec<String>>,
}

fn default_formatter_enabled() -> bool {
    true
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

    fn normalize_public_config_aliases(&mut self) -> Result<(), ConfigError> {
        for provider in self.providers.values_mut() {
            provider.normalize_public_config_aliases()?;
        }

        Ok(())
    }

    fn validate_references(&mut self) -> Result<(), ConfigError> {
        let supported_agents = ["default", "explore", "general", "librarian"];
        if self.agents.len() != supported_agents.len()
            || supported_agents
                .iter()
                .any(|name| !self.agents.contains_key(*name))
        {
            return Err(ConfigError::InvalidReference(
                "agent registry must contain exactly `default`, `explore`, `general`, and `librarian`"
                    .to_string(),
            ));
        }

        for provider_name in self.providers.keys() {
            if ProviderId::parse(provider_name).is_none() {
                return Err(ConfigError::InvalidReference(
                    "provider contains an invalid provider id; provider IDs must be non-empty and must not contain path traversal characters, slashes, null bytes, newlines, or terminal control characters"
                        .to_string(),
                ));
            }
        }

        for profile_name in self.model_profiles.keys() {
            if is_blank_config_value(profile_name) {
                return Err(ConfigError::InvalidReference(
                    "model_profile contains an empty profile name; use explicit names like `fast` or `reasoning`"
                        .to_string(),
                ));
            }
            resolve_named_model_profile(self, profile_name, None)?;
        }

        for (agent_name, agent) in &self.agents {
            resolve_agent_model_selection(self, agent_name, agent)?;
        }

        if let Some(default_profile) = self.ui.default_profile.clone() {
            if !self.agents.contains_key(default_profile.as_str()) {
                return Err(ConfigError::InvalidReference(format!(
                    "ui.default_profile references unknown agent `{default_profile}`; available agents: {}",
                    format_name_list(self.agents.keys().map(|name| name.as_str()))
                )));
            }
        }

        validate_hook_definitions(self)?;
        validate_skill_roots(self)?;
        validate_lsp_overrides(self)?;
        validate_mcp_servers(self)?;

        Ok(())
    }

    pub fn instruction_prompt_prefix(&self) -> Option<String> {
        if self.instruction_files.is_empty() {
            return None;
        }

        Some(
            self.instruction_files
                .iter()
                .map(|instruction| {
                    format!(
                        "Instructions from: {}\n{}",
                        instruction.path.display(),
                        instruction.content
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n"),
        )
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
    #[serde(default)]
    pub compaction: CompactionSettings,
    #[serde(default, alias = "providerRetry")]
    pub provider_retry: ProviderRetryRuntimeConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderRetryRuntimeConfig {
    #[serde(default = "default_provider_retry_max_retries", alias = "maxRetries")]
    pub max_retries: u32,
    #[serde(
        default = "default_provider_retry_base_delay_ms",
        alias = "baseDelayMs"
    )]
    pub base_delay_ms: u64,
    #[serde(default = "default_provider_retry_max_delay_ms", alias = "maxDelayMs")]
    pub max_delay_ms: u64,
}

impl Default for ProviderRetryRuntimeConfig {
    fn default() -> Self {
        Self {
            max_retries: default_provider_retry_max_retries(),
            base_delay_ms: default_provider_retry_base_delay_ms(),
            max_delay_ms: default_provider_retry_max_delay_ms(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct CompactionSettings {
    #[serde(default = "default_compaction_enabled")]
    pub enabled: bool,
    #[serde(default = "default_compaction_reserve_tokens", alias = "reserveTokens")]
    pub reserve_tokens: u32,
    #[serde(
        default = "default_compaction_keep_recent_tokens",
        alias = "keepRecentTokens"
    )]
    pub keep_recent_tokens: u32,
    #[serde(default = "default_compaction_auto_retry_overflow")]
    pub auto_retry_overflow: bool,
    #[serde(
        default = "default_compaction_structured_summary_contract",
        alias = "structuredSummaryContract"
    )]
    pub structured_summary_contract: bool,
    #[serde(default = "default_compaction_estimated_token_triggers")]
    pub estimated_token_triggers: bool,
    #[serde(
        default = "default_compaction_fallback_input_tokens",
        alias = "fallbackInputTokens"
    )]
    pub fallback_input_tokens: u32,
    #[serde(default)]
    pub split_oversized_turns: bool,
    #[serde(default)]
    pub suppress_auto_compaction: bool,
}

impl Default for CompactionSettings {
    fn default() -> Self {
        Self {
            enabled: default_compaction_enabled(),
            reserve_tokens: default_compaction_reserve_tokens(),
            keep_recent_tokens: default_compaction_keep_recent_tokens(),
            auto_retry_overflow: default_compaction_auto_retry_overflow(),
            structured_summary_contract: default_compaction_structured_summary_contract(),
            estimated_token_triggers: default_compaction_estimated_token_triggers(),
            fallback_input_tokens: default_compaction_fallback_input_tokens(),
            split_oversized_turns: false,
            suppress_auto_compaction: false,
        }
    }
}

pub type CompactionRuntimeConfig = CompactionSettings;

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
    CompactionRequested,
    CompactionWritten,
    CompactionApplied,
    CompactionFailed,
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
            Self::CompactionRequested => "compaction_requested",
            Self::CompactionWritten => "compaction_written",
            Self::CompactionApplied => "compaction_applied",
            Self::CompactionFailed => "compaction_failed",
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
    #[serde(default, alias = "projectRoots", alias = "paths")]
    pub project_roots: Vec<PathBuf>,
    #[serde(default, alias = "globalRoots")]
    pub global_roots: Vec<PathBuf>,
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default, alias = "disabledIds")]
    pub disabled: Vec<String>,
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
            urls: Vec::new(),
            disabled: Vec::new(),
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfileConfig {
    #[serde(default)]
    pub name: Option<String>,
    pub description: String,
    #[serde(default, alias = "systemPrompt", alias = "prompt")]
    pub system_prompt: Option<String>,
    #[serde(rename = "model_ref", alias = "modelRef", alias = "model")]
    pub model_ref: String,
    #[serde(default, skip_serializing_if = "is_false")]
    #[schemars(skip)]
    pub model_ref_explicit: bool,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    /// When unset, the runtime omits `temperature` from provider requests so
    /// the provider default applies.
    pub temperature: Option<f32>,
    #[serde(default, alias = "topP")]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub mode: AgentMode,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub options: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub permissions: Option<ProfilePermissions>,
    /// Optional per-profile multi-turn budget. When unset, the runtime does not
    /// impose a profile-specific iteration cap.
    #[serde(default, alias = "maxIters", alias = "steps", alias = "maxSteps")]
    pub max_iters: Option<usize>,
    #[serde(
        default = "default_runtime_tool_failure_mode",
        alias = "toolFailureMode"
    )]
    pub tool_failure_mode: ToolFailureMode,
    #[serde(default)]
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    Primary,
    Subagent,
    #[default]
    All,
}

impl AgentMode {
    pub fn is_subagent_only(self) -> bool {
        matches!(self, Self::Subagent)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ProfilePermissions {
    #[serde(default, rename = "*")]
    pub fallback: Option<PermissionMode>,
    #[serde(default)]
    pub edit: Option<PermissionMode>,
    #[serde(default, alias = "bash")]
    pub shell: Option<PermissionMode>,
    #[serde(default)]
    pub network: Option<PermissionMode>,
    #[serde(default)]
    pub question: Option<PermissionMode>,
    #[serde(default)]
    pub task: Option<PermissionMode>,
    /// Independent of `task` (reference policy allows task but denies todowrite).
    #[serde(default)]
    pub todowrite: Option<PermissionMode>,
    #[serde(default, alias = "webFetch")]
    pub webfetch: Option<PermissionMode>,
    #[serde(default, alias = "webSearch")]
    pub websearch: Option<PermissionMode>,
    #[serde(default, alias = "codeSearch")]
    pub codesearch: Option<PermissionMode>,
    #[serde(default, alias = "codeLsp")]
    pub lsp: Option<PermissionMode>,
    #[serde(default)]
    pub read: Option<PermissionMode>,
    #[serde(default)]
    pub external_directory: Option<PermissionMode>,
    #[serde(default)]
    pub doom_loop: Option<PermissionMode>,
    #[serde(default)]
    pub rules: PermissionRuleSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolFailureMode {
    #[default]
    FailTurn,
    ContinueAsToolMessage,
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
    #[serde(default, rename = "*")]
    pub fallback: Option<PermissionMode>,
    #[serde(default)]
    pub rules: PermissionRuleSet,
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
    #[serde(default)]
    pub read: Option<PermissionMode>,
    #[serde(default)]
    pub external_directory: Option<PermissionMode>,
    #[serde(default)]
    pub doom_loop: Option<PermissionMode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct PermissionRuleSet {
    #[serde(default)]
    pub shell: Vec<PermissionSelectorRule>,
    #[serde(default)]
    pub edit: Vec<PermissionSelectorRule>,
    #[serde(default)]
    pub task: Vec<PermissionSelectorRule>,
    #[serde(default)]
    pub read: Vec<PermissionSelectorRule>,
    #[serde(default)]
    pub external_directory: Vec<PermissionSelectorRule>,
}

// Last-match-wins OC defaults: * allow, *.env ask, *.env.* ask, *.env.example allow.
#[must_use]
pub fn default_read_env_permission_rules() -> Vec<PermissionSelectorRule> {
    vec![
        PermissionSelectorRule {
            selector: PermissionSelector::CatchAll,
            mode: PermissionMode::Allow,
        },
        PermissionSelectorRule {
            selector: PermissionSelector::Glob("*.env".to_string()),
            mode: PermissionMode::Ask,
        },
        PermissionSelectorRule {
            selector: PermissionSelector::Glob("*.env.*".to_string()),
            mode: PermissionMode::Ask,
        },
        PermissionSelectorRule {
            selector: PermissionSelector::Glob("*.env.example".to_string()),
            mode: PermissionMode::Allow,
        },
    ]
}

#[must_use]
pub fn default_permission_rule_set_with_read_env() -> PermissionRuleSet {
    PermissionRuleSet {
        read: default_read_env_permission_rules(),
        ..PermissionRuleSet::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PermissionSelectorRule {
    pub selector: PermissionSelector,
    pub mode: PermissionMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum PermissionSelector {
    Exact(String),
    Prefix(String),
    Glob(String),
    CatchAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum ShellAllowlistMode {
    #[default]
    PermissionPatterns,
    LegacyExecutables,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ShellAllowlist {
    #[serde(default, alias = "policy_mode", alias = "policyMode")]
    pub mode: ShellAllowlistMode,
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

fn merge_config_value(base: &mut serde_json::Value, overlay: serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(overlay_map)) => {
            for (key, value) in overlay_map {
                match base_map.get_mut(&key) {
                    Some(existing) => merge_config_value(existing, value),
                    None => {
                        base_map.insert(key, value);
                    }
                }
            }
        }
        (base_slot, overlay_value) => *base_slot = overlay_value,
    }
}

#[cfg(test)]
mod tests;
