use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

use crate::text::non_empty_trimmed;

pub const DEFAULT_REMOTE_SEARCH_ENDPOINT: &str = "https://mcp.exa.ai/mcp";
pub const DEFAULT_REMOTE_SEARCH_TIMEOUT_SECS: u64 = 30;
pub const DEFAULT_REMOTE_SEARCH_MAX_RETRIES: u32 = 1;
pub const DEFAULT_REMOTE_SEARCH_RETRY_BACKOFF_MS: u64 = 250;

mod discovery;
mod loader;
mod public;

pub use self::discovery::{
    resolve_config_layer_paths, resolve_config_path, resolve_config_path_with_context,
    ConfigDiscoveryContext,
};
pub use self::loader::{
    load_config_from_file, load_config_from_file_with_context, load_config_from_str,
    load_resolved_config, load_resolved_config_with_context, ConfigLoadContext, LoadedConfig,
};
pub use self::public::{
    harness_schema_pretty_json, harness_tui_schema_pretty_json, public_config_contract,
    InstructionList, PublicAgentConfig, PublicConfigAlias, PublicConfigAliasScope,
    PublicConfigCompactionKnob, PublicConfigContract, PublicConfigKeyStatus,
    PublicConfigPermissionName, PublicConfigSurface, PublicConfigTopLevelKey,
    PublicPermissionConfig, PublicPermissionValue, PublicProfilePermissions,
    PublicRulePermissionValue, PublicRuntimeConfig, PublicTuiConfig,
    PublicUnsupportedInactiveValue,
};

static PROFILE_MODEL_METADATA_REGISTRY: OnceLock<
    Mutex<BTreeMap<String, ResolvedProfileModelMetadata>>,
> = OnceLock::new();
static HOOK_RUNTIME_CONFIG_REGISTRY: OnceLock<Mutex<HookRuntimeConfig>> = OnceLock::new();
static SKILLS_CONFIG_REGISTRY: OnceLock<Mutex<SkillsConfig>> = OnceLock::new();
static LSP_CONFIG_REGISTRY: OnceLock<Mutex<LspConfig>> = OnceLock::new();
static INTEGRATIONS_CONFIG_REGISTRY: OnceLock<Mutex<Option<IntegrationsConfig>>> = OnceLock::new();
static MCP_SERVER_CONNECTION_REGISTRY: OnceLock<Mutex<BTreeMap<String, McpServerConnectionState>>> =
    OnceLock::new();
static MCP_SERVER_FIRST_CLASS_TOOL_ID_REGISTRY: OnceLock<
    Mutex<BTreeMap<String, BTreeMap<String, String>>>,
> = OnceLock::new();

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
    #[serde(default, alias = "defaultAgent")]
    pub default_agent: Option<String>,
    #[serde(default)]
    #[serde(skip)]
    #[schemars(skip)]
    pub instruction_files: Vec<InstructionFile>,
}

fn default_hashline_edit() -> bool {
    true
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

fn is_blank_config_value(value: &str) -> bool {
    non_empty_trimmed(value).is_none()
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

        if let Some(default_agent) = self.default_agent.clone() {
            match self.ui.default_profile.as_deref() {
                None => self.ui.default_profile = Some(default_agent),
                Some(profile) if profile == default_agent => {}
                Some(profile) => {
                    return Err(ConfigError::InvalidReference(format!(
                        "top-level `default_agent` `{default_agent}` conflicts with `ui.default_profile` `{profile}`; use one value"
                    )));
                }
            }
        } else if let Some(default_profile) = self.ui.default_profile.clone() {
            self.default_agent = Some(default_profile);
        }

        Ok(())
    }

    fn validate_references(&mut self) -> Result<(), ConfigError> {
        if self.agents.is_empty() {
            return Err(ConfigError::MissingRequiredSections("agents".to_string()));
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

        if let Some(mut default_profile) = self.ui.default_profile.clone() {
            if !self.agents.contains_key(default_profile.as_str()) {
                if self.agents.contains_key("build") {
                    self.ui.default_profile = Some("build".to_string());
                    self.default_agent = Some("build".to_string());
                    default_profile = "build".to_string();
                } else {
                    return Err(ConfigError::InvalidReference(format!(
                        "ui.default_profile references unknown agent `{default_profile}`; available agents: {}",
                        format_name_list(self.agents.keys().map(|name| name.as_str()))
                    )));
                }
            }
            if let Some(profile) = self.agents.get(default_profile.as_str()) {
                if profile.mode.is_subagent_only() {
                    return Err(ConfigError::InvalidReference(format!(
                        "default_agent `{default_profile}` must not reference a subagent-only profile"
                    )));
                }
                if profile.hidden {
                    return Err(ConfigError::InvalidReference(format!(
                        "default_agent `{default_profile}` must not reference a hidden profile"
                    )));
                }
            }
        }

        self.validate_hook_definitions()?;
        self.validate_skill_roots()?;
        self.validate_lsp_overrides()?;
        self.validate_mcp_servers()?;

        Ok(())
    }

    fn validate_mcp_servers(&self) -> Result<(), ConfigError> {
        for (server_name, server) in &self.integrations.mcp.servers {
            if is_blank_config_value(server_name) {
                return Err(ConfigError::InvalidReference(
                    "integrations.mcp.servers contains an empty server name; use explicit ids like `docs-rs` or `gh_grep`"
                        .to_string(),
                ));
            }
            if !server_name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
            {
                return Err(ConfigError::InvalidReference(format!(
                    "integrations.mcp.servers.{server_name} has an invalid server id; use only ASCII letters, digits, `_`, or `-`"
                )));
            }

            match server {
                McpServerConfig::Stdio {
                    command,
                    env,
                    cwd,
                    timeout_secs,
                    ..
                } => {
                    if command.is_empty() {
                        return Err(ConfigError::InvalidReference(format!(
                            "integrations.mcp.servers.{server_name} must include at least one stdio command token"
                        )));
                    }
                    if command.iter().any(|token| is_blank_config_value(token)) {
                        return Err(ConfigError::InvalidReference(format!(
                            "integrations.mcp.servers.{server_name} contains an empty stdio command token"
                        )));
                    }
                    if *timeout_secs == 0 {
                        return Err(ConfigError::InvalidReference(format!(
                            "integrations.mcp.servers.{server_name} must set `timeout_secs` to a value greater than 0"
                        )));
                    }
                    if let Some(cwd) = cwd {
                        if cwd.as_os_str().is_empty() {
                            return Err(ConfigError::InvalidReference(format!(
                                "integrations.mcp.servers.{server_name} must set `cwd` to a non-empty path when provided"
                            )));
                        }
                    }
                    for key in env.keys() {
                        if is_blank_config_value(key) {
                            return Err(ConfigError::InvalidReference(format!(
                                "integrations.mcp.servers.{server_name} contains an empty environment variable name"
                            )));
                        }
                    }
                }
                McpServerConfig::Http {
                    endpoint,
                    headers,
                    timeout_secs,
                    ..
                } => {
                    if is_blank_config_value(endpoint) {
                        return Err(ConfigError::InvalidReference(format!(
                            "integrations.mcp.servers.{server_name} must set a non-empty HTTP endpoint"
                        )));
                    }
                    if *timeout_secs == 0 {
                        return Err(ConfigError::InvalidReference(format!(
                            "integrations.mcp.servers.{server_name} must set `timeout_secs` to a value greater than 0"
                        )));
                    }
                    for key in headers.keys() {
                        if is_blank_config_value(key) {
                            return Err(ConfigError::InvalidReference(format!(
                                "integrations.mcp.servers.{server_name} contains an empty HTTP header name"
                            )));
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn validate_hook_definitions(&self) -> Result<(), ConfigError> {
        for (index, hook) in self.hooks.lifecycle.iter().enumerate() {
            if hook.id.as_deref().is_some_and(is_blank_config_value) {
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

            if hook
                .command
                .iter()
                .any(|token| is_blank_config_value(token))
            {
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
                if is_blank_config_value(cwd) {
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
                if is_blank_config_value(key) {
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
            if is_blank_config_value(pattern) {
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
            if is_blank_config_value(server_name) {
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

                if command.iter().any(|token| is_blank_config_value(token)) {
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
                if is_blank_config_value(key) {
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
    pub compaction: CompactionRuntimeConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompactionRuntimeConfig {
    #[serde(default, alias = "modelBacked")]
    pub model_backed: bool,
    #[serde(default, alias = "modelRef", alias = "model")]
    pub model_ref: Option<String>,
    #[serde(default, alias = "splitOversizedTurns")]
    pub split_oversized_turns: bool,
    #[serde(
        default = "default_compaction_auto_retry_overflow",
        alias = "autoRetryOverflow"
    )]
    pub auto_retry_overflow: bool,
    #[serde(
        default = "default_compaction_structured_summary_contract",
        alias = "structuredSummaryContract"
    )]
    pub structured_summary_contract: bool,
    #[serde(
        default = "default_compaction_estimated_token_triggers",
        alias = "estimatedTokenTriggers"
    )]
    pub estimated_token_triggers: bool,
    #[serde(
        default = "default_compaction_fallback_input_tokens",
        alias = "fallbackInputTokens"
    )]
    pub fallback_input_tokens: u32,
}

impl Default for CompactionRuntimeConfig {
    fn default() -> Self {
        Self {
            model_backed: false,
            model_ref: None,
            split_oversized_turns: false,
            auto_retry_overflow: default_compaction_auto_retry_overflow(),
            structured_summary_contract: default_compaction_structured_summary_contract(),
            estimated_token_triggers: default_compaction_estimated_token_triggers(),
            fallback_input_tokens: default_compaction_fallback_input_tokens(),
        }
    }
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

    fn display_label(&self, provider_name: &str) -> String {
        match self {
            Self::OpenAiCompatible(config) => config
                .name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or(provider_name)
                .to_string(),
        }
    }

    fn normalize_public_config_aliases(&mut self) -> Result<(), ConfigError> {
        match self {
            Self::OpenAiCompatible(config) => config.normalize_public_config_aliases(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenAiCompatibleProviderConfig {
    #[serde(default)]
    pub name: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    #[serde(rename = "name", alias = "display_name", alias = "displayName")]
    pub display_name: String,
    #[serde(default)]
    pub metadata: ModelMetadataConfig,
    #[serde(default)]
    pub limit: ModelLimitConfig,
    #[serde(default)]
    pub modalities: ModelModalitiesConfig,
    #[serde(default)]
    pub options: BTreeMap<String, serde_json::Value>,
    #[serde(default, alias = "maxInputTokens")]
    pub max_input_tokens: Option<u32>,
    #[serde(default, alias = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub variants: BTreeMap<String, ModelVariantConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileConfig {
    pub model: String,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub fallback: Vec<ModelProfileTargetConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelProfileTargetConfig {
    pub model: String,
    #[serde(default)]
    pub variant: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelVariantConfig {
    #[serde(
        rename = "name",
        default,
        alias = "display_name",
        alias = "displayName"
    )]
    pub display_name: Option<String>,
    #[serde(default)]
    pub metadata: ModelVariantMetadataConfig,
    #[serde(default)]
    pub limit: ModelLimitConfig,
    #[serde(default)]
    pub modalities: ModelModalitiesConfig,
    #[serde(default)]
    pub options: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default, alias = "contextWindowTokens")]
    pub context_window_tokens: Option<u32>,
    #[serde(default, alias = "maxInputTokens")]
    pub max_input_tokens: Option<u32>,
    #[serde(default, alias = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ModelLimitConfig {
    #[serde(default)]
    pub context: Option<u32>,
    #[serde(default)]
    pub input: Option<u32>,
    #[serde(default)]
    pub output: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ModelModalitiesConfig {
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub output: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProfileModelMetadata {
    pub profile: String,
    pub profile_description: Option<String>,
    pub provider: String,
    pub provider_display_label: String,
    pub provider_backend_label: Option<String>,
    pub model: String,
    pub model_display_label: String,
    pub variant: Option<String>,
    pub variant_display_label: Option<String>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModelCatalogEntry {
    pub provider: String,
    pub provider_display_label: String,
    pub provider_backend_label: Option<String>,
    pub model: String,
    pub model_display_label: String,
    pub variant: Option<String>,
    pub variant_display_label: Option<String>,
    pub display_label: String,
    pub token_window_label: Option<String>,
    pub context_window_tokens: Option<u32>,
    pub max_input_tokens: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub description: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub recommended_for: Option<String>,
    pub supports_reasoning_summaries: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModelTarget {
    pub model_ref: String,
    pub provider: String,
    pub model: String,
    pub variant: Option<String>,
    pub reasoning_effort: Option<String>,
    pub text_verbosity: Option<String>,
    pub reasoning_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModelSelection {
    pub selector: String,
    pub profile: Option<String>,
    pub primary: ResolvedModelTarget,
    pub fallback: Vec<ResolvedModelTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModelProfileCatalogEntry {
    pub name: String,
    pub primary: ResolvedModelTarget,
    pub fallback: Vec<ResolvedModelTarget>,
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

/// Legacy compatibility alias kept for migration shims and older category-named call sites.
pub type CategoryConfig = ProfileConfig;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ProfilePermissions {
    #[serde(default, rename = "*")]
    pub fallback: Option<PermissionMode>,
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
    #[serde(default)]
    pub rules: PermissionRuleSet,
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

fn default_runtime_tool_failure_mode() -> ToolFailureMode {
    ToolFailureMode::ContinueAsToolMessage
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpServerConnectionState {
    Connected,
    Failed(String),
}

/// Settings for the built-in remote search bridge.
///
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoteSearchConfig {
    /// Endpoint used by the built-in remote search bridge.
    /// Exa-compatible MCP endpoint for native `web_search` and `code_search`.
    ///
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

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(tag = "transport", rename_all = "snake_case", deny_unknown_fields)]
pub enum McpServerConfig {
    Stdio {
        command: Vec<String>,
        #[serde(default, alias = "environment")]
        env: BTreeMap<String, String>,
        #[serde(default)]
        cwd: Option<PathBuf>,
        #[serde(
            default = "default_mcp_timeout_secs",
            alias = "timeoutSecs",
            alias = "timeout"
        )]
        timeout_secs: u64,
        #[serde(default = "default_mcp_enabled")]
        enabled: bool,
    },
    #[serde(alias = "streamable_http")]
    Http {
        #[serde(alias = "url")]
        endpoint: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(
            default = "default_mcp_timeout_secs",
            alias = "timeoutSecs",
            alias = "timeout"
        )]
        timeout_secs: u64,
        #[serde(default = "default_mcp_enabled")]
        enabled: bool,
    },
}

impl McpServerConfig {
    pub fn timeout_secs(&self) -> u64 {
        match self {
            Self::Stdio { timeout_secs, .. } | Self::Http { timeout_secs, .. } => *timeout_secs,
        }
    }

    pub fn enabled(&self) -> bool {
        match self {
            Self::Stdio { enabled, .. } | Self::Http { enabled, .. } => *enabled,
        }
    }
}

impl<'de> Deserialize<'de> for McpServerConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = HarnessTaggedMcpServerConfig::deserialize(deserializer)?;
        Ok(raw.into_runtime())
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case", deny_unknown_fields)]
enum HarnessTaggedMcpServerConfig {
    Stdio {
        command: Vec<String>,
        #[serde(default, alias = "environment")]
        env: BTreeMap<String, String>,
        #[serde(default)]
        cwd: Option<PathBuf>,
        #[serde(
            default = "default_mcp_timeout_secs",
            alias = "timeoutSecs",
            alias = "timeout"
        )]
        timeout_secs: u64,
        #[serde(default = "default_mcp_enabled")]
        enabled: bool,
    },
    #[serde(alias = "streamable_http")]
    Http {
        #[serde(alias = "url")]
        endpoint: String,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(
            default = "default_mcp_timeout_secs",
            alias = "timeoutSecs",
            alias = "timeout"
        )]
        timeout_secs: u64,
        #[serde(default = "default_mcp_enabled")]
        enabled: bool,
    },
}

impl HarnessTaggedMcpServerConfig {
    fn into_runtime(self) -> McpServerConfig {
        match self {
            Self::Stdio {
                command,
                env,
                cwd,
                timeout_secs,
                enabled,
            } => McpServerConfig::Stdio {
                command,
                env,
                cwd,
                timeout_secs,
                enabled,
            },
            Self::Http {
                endpoint,
                headers,
                timeout_secs,
                enabled,
            } => McpServerConfig::Http {
                endpoint,
                headers,
                timeout_secs,
                enabled,
            },
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

fn default_compaction_auto_retry_overflow() -> bool {
    true
}

fn default_compaction_structured_summary_contract() -> bool {
    true
}

fn default_compaction_estimated_token_triggers() -> bool {
    true
}

fn default_compaction_fallback_input_tokens() -> u32 {
    32_768
}

fn default_hook_timeout_ms() -> u64 {
    5_000
}

fn default_skills_walk_to_git_root() -> bool {
    true
}

fn default_skills_project_roots() -> Vec<PathBuf> {
    vec![
        PathBuf::from(".agent-harness/skills"),
        PathBuf::from(".harness/skills"),
    ]
}

fn default_skills_global_roots() -> Vec<PathBuf> {
    vec![PathBuf::from("~/.config/agent-harness/skills")]
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
    DEFAULT_REMOTE_SEARCH_ENDPOINT.to_string()
}

fn default_remote_search_timeout_secs() -> u64 {
    DEFAULT_REMOTE_SEARCH_TIMEOUT_SECS
}

fn default_remote_search_max_retries() -> u32 {
    DEFAULT_REMOTE_SEARCH_MAX_RETRIES
}

fn default_remote_search_retry_backoff_ms() -> u64 {
    DEFAULT_REMOTE_SEARCH_RETRY_BACKOFF_MS
}

fn default_mcp_timeout_secs() -> u64 {
    30
}

fn default_mcp_enabled() -> bool {
    true
}

fn default_provider_timeout_ms() -> u64 {
    60_000
}

fn parse_model_ref(model_ref: &str) -> Option<(&str, &str)> {
    let (provider_name, model_name) = model_ref
        .split_once(':')
        .or_else(|| model_ref.split_once('/'))?;
    if provider_name.is_empty() || model_name.is_empty() {
        return None;
    }
    Some((provider_name, model_name))
}

fn is_direct_model_ref(model_ref: &str) -> bool {
    model_ref.contains(':') || model_ref.contains('/')
}

fn normalize_model_ref(provider: &str, model: &str) -> String {
    format!("{provider}:{model}")
}

pub fn resolve_model_selection(
    cfg: &HarnessConfig,
    selector: &str,
    variant_override: Option<&str>,
) -> Result<ResolvedModelSelection, ConfigError> {
    let selector = selector.trim();
    if selector.is_empty() {
        return Err(ConfigError::InvalidReference(
            "model selector must not be empty; use `<provider>:<model>` or a configured `model_profile` name"
                .to_string(),
        ));
    }

    if is_direct_model_ref(selector) {
        return resolve_direct_model_target(cfg, selector, variant_override, "model selector").map(
            |primary| ResolvedModelSelection {
                selector: selector.to_string(),
                profile: None,
                primary,
                fallback: Vec::new(),
            },
        );
    }

    if cfg.model_profiles.contains_key(selector) {
        return resolve_named_model_profile(cfg, selector, variant_override);
    }

    Err(ConfigError::InvalidReference(format!(
        "unknown model profile `{selector}`; unqualified model selectors must match `model_profile` names; available profiles: {}",
        format_name_list(cfg.model_profiles.keys().map(|name| name.as_str()))
    )))
}

fn resolve_agent_model_selection(
    cfg: &HarnessConfig,
    agent_name: &str,
    agent: &ProfileConfig,
) -> Result<ResolvedModelSelection, ConfigError> {
    resolve_model_selection(cfg, &agent.model_ref, agent.variant.as_deref()).map_err(|err| {
        ConfigError::InvalidReference(format!(
            "agent `{agent_name}` has invalid model selection `{}`: {err}",
            agent.model_ref
        ))
    })
}

fn resolve_named_model_profile(
    cfg: &HarnessConfig,
    profile_name: &str,
    variant_override: Option<&str>,
) -> Result<ResolvedModelSelection, ConfigError> {
    let profile = cfg.model_profiles.get(profile_name).ok_or_else(|| {
        ConfigError::InvalidReference(format!(
            "unknown model profile `{profile_name}`; available profiles: {}",
            format_name_list(cfg.model_profiles.keys().map(|name| name.as_str()))
        ))
    })?;

    let primary = resolve_model_profile_target(
        cfg,
        &ModelProfileTargetConfig {
            model: profile.model.clone(),
            variant: profile.variant.clone(),
        },
        variant_override,
        &format!("model_profile `{profile_name}`"),
    )?;
    let fallback = profile
        .fallback
        .iter()
        .enumerate()
        .map(|(index, target)| {
            resolve_model_profile_target(
                cfg,
                target,
                None,
                &format!("model_profile `{profile_name}` fallback[{index}]"),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(ResolvedModelSelection {
        selector: profile_name.to_string(),
        profile: Some(profile_name.to_string()),
        primary,
        fallback,
    })
}

fn resolve_model_profile_target(
    cfg: &HarnessConfig,
    target: &ModelProfileTargetConfig,
    variant_override: Option<&str>,
    context: &str,
) -> Result<ResolvedModelTarget, ConfigError> {
    if !is_direct_model_ref(&target.model) {
        return Err(ConfigError::InvalidReference(format!(
            "{context} references `{}`; model profile targets must use direct refs like `<provider>:<model>` or `<provider>/<model>`",
            target.model
        )));
    }

    resolve_direct_model_target(
        cfg,
        &target.model,
        variant_override.or(target.variant.as_deref()),
        context,
    )
}

fn resolve_direct_model_target(
    cfg: &HarnessConfig,
    model_ref: &str,
    variant_name: Option<&str>,
    context: &str,
) -> Result<ResolvedModelTarget, ConfigError> {
    let Some((provider_name, model_name)) = parse_model_ref(model_ref) else {
        return Err(ConfigError::InvalidReference(format!(
            "{context} has invalid model ref `{model_ref}`; use `<provider>:<model>` or `<provider>/<model>`"
        )));
    };

    let resolved = resolve_configured_model_metadata(cfg, provider_name, model_name, variant_name)
        .map_err(|err| ConfigError::InvalidReference(format!("{context}: {err}")))?;
    Ok(ResolvedModelTarget {
        model_ref: normalize_model_ref(&resolved.provider, &resolved.model),
        provider: resolved.provider,
        model: resolved.model,
        variant: resolved.variant,
        reasoning_effort: resolved.reasoning_effort.clone(),
        text_verbosity: resolved.text_verbosity,
        reasoning_summary: if resolved.supports_reasoning_summaries
            && resolved.reasoning_effort.is_some()
        {
            Some("auto".to_string())
        } else {
            None
        },
    })
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

fn integrations_config_registry() -> &'static Mutex<Option<IntegrationsConfig>> {
    INTEGRATIONS_CONFIG_REGISTRY.get_or_init(|| Mutex::new(None))
}

fn mcp_server_connection_registry() -> &'static Mutex<BTreeMap<String, McpServerConnectionState>> {
    MCP_SERVER_CONNECTION_REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn mcp_server_first_class_tool_id_registry(
) -> &'static Mutex<BTreeMap<String, BTreeMap<String, String>>> {
    MCP_SERVER_FIRST_CLASS_TOOL_ID_REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn with_registry_lock<T, U>(registry: &'static Mutex<T>, f: impl FnOnce(&mut T) -> U) -> U {
    match registry.lock() {
        Ok(mut guard) => f(&mut guard),
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            f(&mut guard)
        }
    }
}

fn with_profile_model_metadata_registry<T>(
    f: impl FnOnce(&mut BTreeMap<String, ResolvedProfileModelMetadata>) -> T,
) -> T {
    with_registry_lock(profile_model_metadata_registry(), f)
}

fn with_hook_runtime_config_registry<T>(f: impl FnOnce(&mut HookRuntimeConfig) -> T) -> T {
    with_registry_lock(hook_runtime_config_registry(), f)
}

fn with_skills_config_registry<T>(f: impl FnOnce(&mut SkillsConfig) -> T) -> T {
    with_registry_lock(skills_config_registry(), f)
}

fn with_lsp_config_registry<T>(f: impl FnOnce(&mut LspConfig) -> T) -> T {
    with_registry_lock(lsp_config_registry(), f)
}

fn with_integrations_config_registry<T>(f: impl FnOnce(&mut Option<IntegrationsConfig>) -> T) -> T {
    with_registry_lock(integrations_config_registry(), f)
}

fn with_mcp_server_connection_registry<T>(
    f: impl FnOnce(&mut BTreeMap<String, McpServerConnectionState>) -> T,
) -> T {
    with_registry_lock(mcp_server_connection_registry(), f)
}

fn with_mcp_server_first_class_tool_id_registry<T>(
    f: impl FnOnce(&mut BTreeMap<String, BTreeMap<String, String>>) -> T,
) -> T {
    with_registry_lock(mcp_server_first_class_tool_id_registry(), f)
}

pub fn refresh_profile_model_metadata_registry(cfg: &HarnessConfig) -> Result<(), ConfigError> {
    let resolved = cfg
        .agents
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

pub fn refresh_integrations_config_registry(cfg: &HarnessConfig) {
    set_registered_integrations_config(cfg.integrations.clone());
    clear_registered_mcp_server_connection_states();
    clear_registered_mcp_server_first_class_tool_ids();
}

pub fn registered_skills_config() -> SkillsConfig {
    with_skills_config_registry(|registered| registered.clone())
}

pub fn set_registered_integrations_config(config: IntegrationsConfig) {
    with_integrations_config_registry(|registered| {
        *registered = Some(config);
    });
}

pub fn clear_registered_integrations_config() {
    with_integrations_config_registry(|registered| {
        *registered = None;
    });
    clear_registered_mcp_server_connection_states();
    clear_registered_mcp_server_first_class_tool_ids();
}

pub fn registered_integrations_config() -> Option<IntegrationsConfig> {
    with_integrations_config_registry(|registered| registered.clone())
}

pub fn set_registered_mcp_server_connection_states(
    states: BTreeMap<String, McpServerConnectionState>,
) {
    with_mcp_server_connection_registry(|registered| {
        *registered = states;
    });
}

pub fn clear_registered_mcp_server_connection_states() {
    with_mcp_server_connection_registry(|registered| {
        registered.clear();
    });
}

pub fn set_registered_mcp_server_first_class_tool_ids(
    tool_ids: BTreeMap<String, BTreeMap<String, String>>,
) {
    with_mcp_server_first_class_tool_id_registry(|registered| {
        *registered = tool_ids;
    });
}

pub fn clear_registered_mcp_server_first_class_tool_ids() {
    with_mcp_server_first_class_tool_id_registry(|registered| {
        registered.clear();
    });
}

pub fn registered_mcp_server_first_class_tool_id(
    server_name: &str,
    remote_tool_name: &str,
) -> Option<String> {
    with_mcp_server_first_class_tool_id_registry(|registered| {
        registered
            .get(server_name)
            .and_then(|tool_ids| tool_ids.get(remote_tool_name))
            .cloned()
    })
}

pub fn registered_mcp_server_connection_state(
    server_name: &str,
) -> Option<McpServerConnectionState> {
    with_mcp_server_connection_registry(|registered| registered.get(server_name).cloned())
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
    let profile = cfg.agents.get(profile_name).ok_or_else(|| {
        ConfigError::InvalidReference(format!(
            "unknown agent `{profile_name}`; available agents: {}",
            format_name_list(cfg.agents.keys().map(|name| name.as_str()))
        ))
    })?;

    let selection = resolve_agent_model_selection(cfg, profile_name, profile)?;
    let provider_name = selection.primary.provider.as_str();
    let model_name = selection.primary.model.as_str();

    let provider = cfg.providers.get(provider_name).ok_or_else(|| {
        ConfigError::InvalidReference(format!(
            "agent `{profile_name}` references unknown provider `{provider_name}` in model selection `{}`; available providers: {}",
            profile.model_ref,
            format_name_list(cfg.providers.keys().map(|name| name.as_str()))
        ))
    })?;

    let models = provider.models();
    let model = models.get(model_name).ok_or_else(|| {
            ConfigError::InvalidReference(format!(
                "agent `{profile_name}` references unknown model `{model_name}` in model selection `{}`; available models for provider `{provider_name}`: {}",
                profile.model_ref,
                format_name_list(models.keys().map(|name| name.as_str()))
            ))
        })?;

    let variant = selection.primary.variant.as_deref().map(|variant_name| {
        let variant = model.variants.get(variant_name).ok_or_else(|| {
            ConfigError::InvalidReference(format!(
                "agent `{profile_name}` references unknown variant `{variant_name}` for model `{}`; available variants: {}",
                selection.primary.model_ref,
                format_name_list(model.variants.keys().map(|name| name.as_str()))
            ))
        })?;

        if variant.disabled {
            return Err(ConfigError::InvalidReference(format!(
                "agent `{profile_name}` references disabled variant `{variant_name}` for model `{}`; choose an enabled variant",
                selection.primary.model_ref
            )));
        }

        Ok((variant_name, variant))
    });
    let variant = variant.transpose()?;

    let display_label = build_model_display_label(model, variant);
    let variant_display_label = variant.map(|(variant_name, variant_cfg)| {
        variant_cfg
            .display_name
            .clone()
            .unwrap_or_else(|| variant_name.to_string())
    });
    let context_window_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.context_window_tokens)
        .or_else(|| variant.and_then(|(_, variant_cfg)| variant_cfg.limit.context))
        .or(model.metadata.context_window_tokens)
        .or(model.limit.context);
    let max_input_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.max_input_tokens)
        .or_else(|| variant.and_then(|(_, variant_cfg)| variant_cfg.limit.input))
        .or(model.max_input_tokens)
        .or(model.limit.input);
    let max_output_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.max_output_tokens)
        .or_else(|| variant.and_then(|(_, variant_cfg)| variant_cfg.limit.output))
        .or(model.max_output_tokens)
        .or(model.limit.output);

    Ok(ResolvedProfileModelMetadata {
        profile: profile_name.to_string(),
        profile_description: Some(profile.description.clone()),
        provider: provider_name.to_string(),
        provider_display_label: provider.display_label(provider_name),
        provider_backend_label: provider_backend_label(provider).map(str::to_string),
        model: model_name.to_string(),
        model_display_label: model.display_name.clone(),
        variant: variant.map(|(variant_name, _)| variant_name.to_string()),
        variant_display_label,
        display_label,
        token_window_label: build_token_window_label(
            context_window_tokens,
            max_input_tokens,
            max_output_tokens,
        ),
        context_window_tokens,
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

pub fn resolve_configured_model_metadata(
    cfg: &HarnessConfig,
    provider_name: &str,
    model_name: &str,
    variant_name: Option<&str>,
) -> Result<ResolvedModelCatalogEntry, ConfigError> {
    let provider = cfg.providers.get(provider_name).ok_or_else(|| {
        ConfigError::InvalidReference(format!(
            "unknown provider `{provider_name}`; available providers: {}",
            format_name_list(cfg.providers.keys().map(|name| name.as_str()))
        ))
    })?;

    let model = provider.models().get(model_name).ok_or_else(|| {
        ConfigError::InvalidReference(format!(
            "unknown model `{model_name}` for provider `{provider_name}`; available models: {}",
            format_name_list(provider.models().keys().map(|name| name.as_str()))
        ))
    })?;

    let variant = variant_name.map(|variant_name| {
        let variant = model.variants.get(variant_name).ok_or_else(|| {
            ConfigError::InvalidReference(format!(
                "unknown variant `{variant_name}` for model `{provider_name}:{model_name}`; available variants: {}",
                format_name_list(model.variants.keys().map(|name| name.as_str()))
            ))
        })?;

        if variant.disabled {
            return Err(ConfigError::InvalidReference(format!(
                "variant `{variant_name}` for model `{provider_name}:{model_name}` is disabled"
            )));
        }

        Ok((variant_name, variant))
    });
    let variant = variant.transpose()?;

    Ok(build_resolved_model_catalog_entry(
        provider_name,
        model_name,
        model,
        provider,
        variant,
    ))
}

pub fn configured_model_catalog(cfg: &HarnessConfig) -> Vec<ResolvedModelCatalogEntry> {
    let mut entries = Vec::new();

    for (provider_name, provider) in &cfg.providers {
        for (model_name, model) in provider.models() {
            entries.push(build_resolved_model_catalog_entry(
                provider_name,
                model_name,
                model,
                provider,
                None,
            ));

            for (variant_name, variant_cfg) in &model.variants {
                if variant_cfg.disabled {
                    continue;
                }

                entries.push(build_resolved_model_catalog_entry(
                    provider_name,
                    model_name,
                    model,
                    provider,
                    Some((variant_name.as_str(), variant_cfg)),
                ));
            }
        }
    }

    entries
}

pub fn configured_model_profile_catalog(
    cfg: &HarnessConfig,
) -> Result<Vec<ResolvedModelProfileCatalogEntry>, ConfigError> {
    cfg.model_profiles
        .keys()
        .map(|name| {
            resolve_named_model_profile(cfg, name, None).map(|selection| {
                ResolvedModelProfileCatalogEntry {
                    name: name.clone(),
                    primary: selection.primary,
                    fallback: selection.fallback,
                }
            })
        })
        .collect()
}

fn build_resolved_model_catalog_entry(
    provider_name: &str,
    model_name: &str,
    model: &ModelConfig,
    provider: &ProviderConfig,
    variant: Option<(&str, &ModelVariantConfig)>,
) -> ResolvedModelCatalogEntry {
    let context_window_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.context_window_tokens)
        .or_else(|| variant.and_then(|(_, variant_cfg)| variant_cfg.limit.context))
        .or(model.metadata.context_window_tokens)
        .or(model.limit.context);
    let max_input_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.max_input_tokens)
        .or_else(|| variant.and_then(|(_, variant_cfg)| variant_cfg.limit.input))
        .or(model.max_input_tokens)
        .or(model.limit.input);
    let max_output_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.max_output_tokens)
        .or_else(|| variant.and_then(|(_, variant_cfg)| variant_cfg.limit.output))
        .or(model.max_output_tokens)
        .or(model.limit.output);

    ResolvedModelCatalogEntry {
        provider: provider_name.to_string(),
        provider_display_label: provider.display_label(provider_name),
        provider_backend_label: provider_backend_label(provider).map(str::to_string),
        model: model_name.to_string(),
        model_display_label: model.display_name.clone(),
        variant: variant.map(|(variant_name, _)| variant_name.to_string()),
        variant_display_label: variant.map(|(variant_name, variant_cfg)| {
            variant_cfg
                .display_name
                .clone()
                .unwrap_or_else(|| variant_name.to_string())
        }),
        display_label: build_model_display_label(model, variant),
        token_window_label: build_token_window_label(
            context_window_tokens,
            max_input_tokens,
            max_output_tokens,
        ),
        context_window_tokens,
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
        supports_reasoning_summaries: model.metadata.supports_reasoning_summaries.unwrap_or(false),
    }
}

fn provider_backend_label(provider: &ProviderConfig) -> Option<&'static str> {
    match provider {
        ProviderConfig::OpenAiCompatible(_) => Some("OpenAI"),
    }
}

fn merge_string_alias(
    target: &mut impl StringAliasTarget,
    alias: Option<String>,
    target_path: &str,
    alias_path: &str,
) -> Result<(), ConfigError> {
    let Some(alias) = alias.map(|value| value.trim().to_string()) else {
        return Ok(());
    };
    if alias.is_empty() {
        return Ok(());
    }

    match target.current_value() {
        Some(current) if current == alias => Ok(()),
        Some(_) => Err(ConfigError::InvalidReference(format!(
            "{target_path} conflicts with {alias_path}; use one value"
        ))),
        None => {
            target.set_value(alias);
            Ok(())
        }
    }
}

fn merge_map_alias(
    target: &mut BTreeMap<String, String>,
    alias: BTreeMap<String, String>,
    target_path: &str,
    alias_path: &str,
) -> Result<(), ConfigError> {
    merge_alias_value(target, alias, BTreeMap::is_empty, target_path, alias_path)
}

fn merge_vec_alias(
    target: &mut Vec<String>,
    alias: Vec<String>,
    target_path: &str,
    alias_path: &str,
) -> Result<(), ConfigError> {
    merge_alias_value(target, alias, Vec::is_empty, target_path, alias_path)
}

fn merge_alias_value<T>(
    target: &mut T,
    alias: T,
    is_empty: impl Fn(&T) -> bool,
    target_path: &str,
    alias_path: &str,
) -> Result<(), ConfigError>
where
    T: PartialEq,
{
    if is_empty(&alias) {
        return Ok(());
    }
    if is_empty(target) {
        *target = alias;
        return Ok(());
    }
    if *target == alias {
        return Ok(());
    }

    Err(ConfigError::InvalidReference(format!(
        "{target_path} conflicts with {alias_path}; use one value"
    )))
}

trait StringAliasTarget {
    fn current_value(&self) -> Option<&str>;
    fn set_value(&mut self, value: String);
}

impl StringAliasTarget for String {
    fn current_value(&self) -> Option<&str> {
        non_empty_trimmed(self)
    }

    fn set_value(&mut self, value: String) {
        *self = value;
    }
}

impl StringAliasTarget for Option<String> {
    fn current_value(&self) -> Option<&str> {
        self.as_deref().and_then(non_empty_trimmed)
    }

    fn set_value(&mut self, value: String) {
        *self = Some(value);
    }
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

fn is_builtin_lsp_server(name: &str) -> bool {
    matches!(
        name,
        "go" | "json" | "python" | "rust" | "typescript" | "yaml"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static CONFIG_DISCOVERY_TEST_LOCK: Mutex<()> = Mutex::new(());
    fn discovery_context(cwd: &Path, xdg_config_home: Option<&Path>) -> ConfigLoadContext {
        ConfigLoadContext {
            discovery: ConfigDiscoveryContext {
                current_dir: cwd.to_path_buf(),
                xdg_config_home: xdg_config_home.map(Path::to_path_buf),
                home: Some(cwd.to_path_buf()),
                runtime_config_path: None,
                tui_config_path: None,
            },
            runtime_content: None,
        }
    }

    fn config_fixture(
        agents: &str,
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
          agents: {{
            {agents}
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
            agents = agents,
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

    fn write_agent_markdown_in(repo_root: &Path, prompt_root: &str, name: &str, content: &str) {
        let path = repo_root
            .join(prompt_root)
            .join("agents")
            .join(format!("{name}.md"));
        fs::create_dir_all(path.parent().expect("agent markdown parent"))
            .expect("create agent markdown parent");
        fs::write(path, content).expect("write agent markdown");
    }

    fn write_agent_markdown(repo_root: &Path, name: &str, content: &str) {
        write_agent_markdown_in(repo_root, ".agent-harness", name, content);
    }

    fn write_legacy_agent_markdown(repo_root: &Path, name: &str, content: &str) {
        write_agent_markdown_in(repo_root, ".agent-harness", name, content);
    }

    #[test]
    fn structured_summary_contract_defaults_on_and_serializes_alias() {
        let default_compaction: CompactionRuntimeConfig =
            serde_json::from_value(serde_json::json!({})).expect("empty compaction config parses");
        assert!(default_compaction.structured_summary_contract);
        assert!(default_compaction.estimated_token_triggers);
        assert_eq!(default_compaction.fallback_input_tokens, 32_768);

        let disabled_via_alias: CompactionRuntimeConfig =
            serde_json::from_value(serde_json::json!({
                "structuredSummaryContract": false,
                "estimatedTokenTriggers": false,
                "fallbackInputTokens": 65_536,
            }))
            .expect("camelCase compaction aliases parse");
        assert!(!disabled_via_alias.structured_summary_contract);
        assert!(!disabled_via_alias.estimated_token_triggers);
        assert_eq!(disabled_via_alias.fallback_input_tokens, 65_536);

        let serialized =
            serde_json::to_value(&disabled_via_alias).expect("compaction config serializes");
        assert_eq!(
            serialized.get("structured_summary_contract"),
            Some(&serde_json::Value::Bool(false))
        );
        assert_eq!(
            serialized.get("estimated_token_triggers"),
            Some(&serde_json::Value::Bool(false))
        );
        assert_eq!(
            serialized.get("fallback_input_tokens"),
            Some(&serde_json::Value::from(65_536))
        );
    }

    #[test]
    fn example_config_parses_public_agents_and_compaction() {
        let agents = r#"
            deep: {
              description: "Default deep execution profile",
              model_ref: "default:gpt-4o-mini",
              tools: ["read"],
            },
            review: {
              description: "Review profile",
              model_ref: "default:gpt-4o-mini",
              max_iters: 20,
              tool_failure_mode: "continue_as_tool_message",
              tools: ["read", "invalid"],
            },
        "#;

        let text = config_fixture(
            agents,
            "test-openai-api-key",
            Some(
                r#"
                ui: {
                  default_profile: "deep",
                },
                "#,
            ),
            Some("./config.json"),
        );
        let parsed = load_config_from_str(&text).expect("fixture config must parse");

        assert_eq!(parsed.schema.as_deref(), Some("./config.json"));
        assert!(parsed.providers.contains_key("default"));
        assert!(parsed.agents.contains_key("deep"));
        assert_eq!(
            parsed.agents["review"].tool_failure_mode,
            ToolFailureMode::ContinueAsToolMessage
        );
        assert_eq!(parsed.agents["review"].max_iters, Some(20));
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
              agents: {
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
        let err = load_config_from_str("{}").expect_err("config without agents must fail");
        assert_eq!(err.to_string(), "missing required config sections: agents");
    }

    #[test]
    fn retired_top_level_keys_fail_with_migration_guidance() {
        let parsed = load_config_from_str(
            r#"
            {
              provider: {
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
              model: "default/gpt-4o-mini",
              categories: {
                deep: {
                  description: "Deep work",
                  model_ref: "default:gpt-4o-mini",
                  tools: ["read"]
                }
              },
              backgroundTask: {
                defaultConcurrency: 2,
                providerConcurrency: 2,
                modelConcurrency: 2,
                staleTimeoutMs: 30000,
                messageStalenessTimeoutMs: 10000
              },
              paths: {
                session_dir: ".agent-harness/sessions"
              },
              deterministic: {
                enabled: false,
                seed: 42
              }
            }
            "#,
        )
        .expect("legacy compatibility keys should translate");

        assert!(parsed.agents.contains_key("deep"));
        assert_eq!(parsed.runtime.background_tasks.default_concurrency, 2);
        assert_eq!(
            parsed.paths.session_dir,
            PathBuf::from(".agent-harness/sessions")
        );
    }

    #[test]
    fn runtime_background_tasks_camel_case_aliases_parse_without_duplicate_fields() {
        let parsed = load_config_from_str(
            r#"
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
              agents: {
                deep: {
                  description: "Deep work",
                  model_ref: "default:gpt-4o-mini",
                  tools: ["read"]
                }
              },
              runtime: {
                background_tasks: {
                  defaultConcurrency: 2,
                  providerConcurrency: 3,
                  modelConcurrency: 4,
                  staleTimeoutMs: 30000,
                  messageStalenessTimeoutMs: 10000
                },
                permissions: {
                  askTimeoutMs: 777
                },
                prompt: {
                  waitTimeoutMs: 999
                },
                sessionDir: ".agent-harness/custom-sessions"
              }
            }
            "#,
        )
        .expect("runtime camelCase aliases should parse without duplicate logical fields");

        assert_eq!(parsed.runtime.background_tasks.default_concurrency, 2);
        assert_eq!(parsed.runtime.background_tasks.provider_concurrency, 3);
        assert_eq!(parsed.runtime.background_tasks.model_concurrency, 4);
        assert_eq!(parsed.runtime.background_tasks.stale_timeout_ms, 30000);
        assert_eq!(
            parsed.runtime.background_tasks.message_staleness_timeout_ms,
            10000
        );
        assert_eq!(parsed.runtime.permissions.ask_timeout_ms, 777);
        assert_eq!(parsed.runtime.prompt.wait_timeout_ms, 999);
        assert_eq!(
            parsed.runtime.session_dir,
            PathBuf::from(".agent-harness/custom-sessions")
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
              agents: {
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
        assert!(err
            .to_string()
            .contains("unknown top-level config keys: `extraTopLevel`"));
        assert!(err.to_string().contains("`provider`"));
    }

    #[test]
    fn top_level_hashline_edit_alias_and_default_are_accepted() {
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
          agents: {
            build: {
              description: "Build work",
              model_ref: "default:gpt-4o-mini",
              tools: ["read"]
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
          hashlineEdit: false
        }
        "#;

        let parsed = load_config_from_str(cfg).expect("hashline-edit alias should parse");
        assert!(!parsed.hashline_edit);

        let defaults_cfg = r#"
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
          agents: {
            build: {
              description: "Build work",
              model_ref: "default:gpt-4o-mini",
              tools: ["read"]
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

        let parsed_defaults =
            load_config_from_str(defaults_cfg).expect("hashline-edit defaults should parse");
        assert!(parsed_defaults.hashline_edit);
    }

    #[test]
    fn top_level_default_agent_camel_case_alias_is_accepted() {
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
          agents: {
            build: {
              description: "Build work",
              model_ref: "default:gpt-4o-mini",
              tools: ["fs.read"]
            }
          },
          defaultAgent: "build",
          permissions: {
            defaults: {
              edit: "allow",
              shell: "allow",
              network: "allow"
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
            session_dir: ".agent-harness/sessions"
          },
          integrations: {
            remote_search: {
              endpoint: "https://mcp.exa.ai/mcp"
            }
          }
        }
        "#;

        let parsed = load_config_from_str(cfg).expect("defaultAgent alias should parse");
        assert_eq!(parsed.default_agent.as_deref(), Some("build"));
        assert_eq!(parsed.ui.default_profile.as_deref(), Some("build"));
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
            "agent `deep` has invalid model selection `missing:gpt-4o-mini`: model selector: unknown provider `missing`; available providers: default"
        );
    }

    #[test]
    fn profile_rejects_legacy_plan_mode_field() {
        let cfg = config_fixture(
            &deep_profile(
                r#"
                plan_mode: true,
                tools: ["fs.read"],
                "#,
            ),
            "test-key",
            None,
            None,
        );

        let err = load_config_from_str(&cfg).expect_err("legacy plan_mode must fail");
        assert!(err.to_string().contains("unknown field `plan_mode`"));
    }

    #[test]
    fn profile_rejects_legacy_exit_target_profile_field() {
        let cfg = config_fixture(
            &deep_profile(
                r#"
                exit_target_profile: "build",
                tools: ["fs.read"],
                "#,
            ),
            "test-key",
            None,
            None,
        );

        let err = load_config_from_str(&cfg).expect_err("legacy exit_target_profile must fail");
        assert!(err
            .to_string()
            .contains("unknown field `exit_target_profile`"));
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
            "ui.default_profile references unknown agent `ops`; available agents: deep"
        );
    }

    #[test]
    fn default_agent_normalizes_to_runtime_shape() {
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
          agents: {
            build: {
              description: "Build work",
              model_ref: "default:gpt-4o-mini",
              tools: ["fs.read"]
            },
            plan: {
              description: "Planning work",
              model_ref: "default:gpt-4o-mini",
              permissions: {
                edit: "deny",
                shell: "deny"
              },
              tools: ["fs.read"]
            }
          },
          default_agent: "build",
          permissions: {
            defaults: {
              edit: "allow",
              shell: "allow",
              network: "allow"
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
            session_dir: ".agent-harness/sessions"
          },
          integrations: {
            remote_search: {
              endpoint: "https://mcp.exa.ai/mcp"
            }
          }
        }
        "#;

        let parsed = load_config_from_str(cfg).expect("default_agent config should parse");
        assert!(parsed.agents.contains_key("build"));
        assert!(parsed.agents.contains_key("plan"));
        assert_eq!(parsed.ui.default_profile.as_deref(), Some("build"));
        assert_eq!(parsed.default_agent.as_deref(), Some("build"));
        assert_eq!(
            parsed.agents["plan"].permissions.as_ref().unwrap().edit,
            Some(PermissionMode::Deny)
        );
        assert_eq!(
            parsed.agents["plan"].permissions.as_ref().unwrap().shell,
            Some(PermissionMode::Deny)
        );
    }

    #[test]
    fn public_default_agents_continue_after_tool_failures() {
        let cfg = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              },
              models: {
                "gpt-4o-mini": {
                  name: "GPT-4o mini"
                }
              }
            }
          },
          model: "default/gpt-4o-mini",
          small_model: "default/gpt-4o-mini",
          agent: {
            build: {
              system_prompt: "Build work"
            }
          },
          default_agent: "build",
          permission: {
            edit: "allow",
            bash: "allow",
            question: "allow",
            task: "allow",
            webfetch: "allow",
            websearch: "allow",
            codesearch: "allow",
            lsp: "allow"
          }
        }
        "#;

        let parsed = load_config_from_str(cfg).expect("public config should parse");
        assert_eq!(
            parsed.agents["build"].tool_failure_mode,
            ToolFailureMode::ContinueAsToolMessage
        );
        assert!(parsed.agents.contains_key("plan"));
        assert_eq!(parsed.agents["plan"].mode, AgentMode::Primary);
        let plan = parsed.agents["plan"].permissions.as_ref().unwrap();
        assert_eq!(plan.shell, Some(PermissionMode::Ask));
        assert_eq!(plan.task, Some(PermissionMode::Allow));
        assert!(plan.rules.edit.iter().any(|rule| matches!(
            (&rule.selector, &rule.mode),
            (PermissionSelector::CatchAll, PermissionMode::Deny)
        )));
        assert!(plan.rules.edit.iter().any(|rule| matches!(
            (&rule.selector, &rule.mode),
            (PermissionSelector::Prefix(prefix), PermissionMode::Allow)
                if prefix == ".agent-harness/plans/"
        )));

        let tools = &parsed.agents["plan"].tools;
        for required_tool in [
            "read",
            "glob",
            "grep",
            "list",
            "lsp",
            "question",
            "task",
            "background_output",
            "edit",
            "bash",
            "plan_exit",
        ] {
            assert!(
                tools.contains(&required_tool.to_string()),
                "plan profile should expose required tool {required_tool}"
            );
        }
        assert!(tools.contains(&"bash".to_string()));
        assert!(parsed.agents["build"]
            .tools
            .contains(&"plan_enter".to_string()));
        assert!(!tools.contains(&"plan_enter".to_string()));
    }

    #[test]
    fn public_agent_schema_accepts_prompt_model_tool_map_and_metadata() {
        let cfg = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              },
              models: {
                "gpt-4o-mini": { name: "GPT-4o mini" }
              }
            }
          },
          model: "default/gpt-4o-mini",
          agent: {
            build: {
              name: "Builder",
              prompt: "Build work",
              model: "default/gpt-4o-mini",
              topP: 0.8,
              mode: "primary",
              hidden: false,
              color: "blue",
              options: { reasoningEffort: "low" },
              steps: 12,
              tools: {
                read: true,
                bash: false,
                task: true
              }
            },
            explore: {
              disable: true
            },
            reviewer: {
              name: "Reviewer",
              description: "Review work",
              model: "default/gpt-4o-mini",
              mode: "subagent",
              hidden: true,
              maxSteps: 4,
              tools: ["read"]
            }
          },
          default_agent: "build",
          permission: "allow"
        }
        "#;

        let parsed = load_config_from_str(cfg).expect("public agent schema should parse");
        let build = &parsed.agents["build"];
        assert_eq!(build.name.as_deref(), Some("Builder"));
        assert_eq!(build.system_prompt.as_deref(), Some("Build work"));
        assert_eq!(build.model_ref, "default/gpt-4o-mini");
        assert_eq!(build.top_p, Some(0.8));
        assert_eq!(build.mode, AgentMode::Primary);
        assert!(!build.hidden);
        assert_eq!(build.color.as_deref(), Some("blue"));
        assert!(build.options.contains_key("reasoningEffort"));
        assert_eq!(build.max_iters, Some(12));
        assert_eq!(build.tools, vec!["read".to_string(), "task".to_string()]);
        assert!(!parsed.agents.contains_key("explore"));
        let reviewer = &parsed.agents["reviewer"];
        assert_eq!(reviewer.name.as_deref(), Some("Reviewer"));
        assert_eq!(reviewer.mode, AgentMode::Subagent);
        assert!(reviewer.hidden);
        assert_eq!(reviewer.max_iters, Some(4));
    }

    #[test]
    fn default_agent_rejects_subagent_only_and_hidden_profiles() {
        for (field, value, expected) in [
            (
                "mode",
                "\"subagent\"",
                "must not reference a subagent-only profile",
            ),
            ("hidden", "true", "must not reference a hidden profile"),
        ] {
            let cfg = format!(
                r#"
        {{
          provider: {{
            default: {{
              type: "openai_compatible",
              options: {{
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              }},
              models: {{
                "gpt-4o-mini": {{ name: "GPT-4o mini" }}
              }}
            }}
          }},
          model: "default/gpt-4o-mini",
          agent: {{
            build: {{
              prompt: "Build work",
              {field}: {value}
            }}
          }},
          default_agent: "build",
          permission: "allow"
        }}
        "#
            );
            let err = load_config_from_str(&cfg).expect_err("invalid default agent must fail");
            assert!(err.to_string().contains(expected));
        }
    }

    #[test]
    fn default_agent_rejects_disabled_profile() {
        let cfg = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              },
              models: {
                "gpt-4o-mini": { name: "GPT-4o mini" }
              }
            }
          },
          model: "default/gpt-4o-mini",
          agent: {
            explore: { disable: true }
          },
          default_agent: "explore",
          permission: "allow"
        }
        "#;

        let err = load_config_from_str(cfg).expect_err("disabled default agent must fail");
        assert!(err
            .to_string()
            .contains("default_agent `explore` references a disabled agent"));
    }

    fn public_minimal_config_with_permission(permission: &str) -> String {
        format!(
            r#"
        {{
          provider: {{
            default: {{
              type: "openai_compatible",
              options: {{
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              }},
              models: {{
                "gpt-4o-mini": {{
                  name: "GPT-4o mini"
                }}
              }}
            }}
          }},
          model: "default/gpt-4o-mini",
          agent: {{
            build: {{
              system_prompt: "Build work"
            }}
          }},
          default_agent: "build",
          permission: {permission}
        }}
        "#
        )
    }

    #[test]
    fn permission_scalar_expands_to_public_kinds_and_network() {
        for (raw, mode) in [
            ("\"ask\"", PermissionMode::Ask),
            ("\"allow\"", PermissionMode::Allow),
            ("\"deny\"", PermissionMode::Deny),
        ] {
            let cfg = public_minimal_config_with_permission(raw);
            let parsed = load_config_from_str(&cfg).expect("permission scalar should parse");
            assert_eq!(parsed.permissions.defaults.edit, mode);
            assert_eq!(parsed.permissions.defaults.shell, mode);
            assert_eq!(parsed.permissions.defaults.network, mode);
            assert_eq!(parsed.permissions.defaults.question, Some(mode.clone()));
            assert_eq!(parsed.permissions.defaults.task, Some(mode.clone()));
            assert_eq!(parsed.permissions.defaults.webfetch, Some(mode.clone()));
            assert_eq!(parsed.permissions.defaults.websearch, Some(mode.clone()));
            assert_eq!(parsed.permissions.defaults.codesearch, Some(mode.clone()));
            assert_eq!(parsed.permissions.defaults.lsp, Some(mode));
        }
    }

    #[test]
    fn permission_scalar_rejects_invalid_mode() {
        let cfg = public_minimal_config_with_permission("\"maybe\"");
        load_config_from_str(&cfg).expect_err("invalid permission scalar must fail");
    }

    #[test]
    fn permission_object_accepts_per_tool_scalar_modes() {
        let cfg = public_minimal_config_with_permission(
            r#"{
                bash: "ask",
                edit: "deny",
                question: "allow",
                task: "ask",
                webfetch: "deny",
                websearch: "allow",
                codesearch: "deny",
                lsp: "allow"
            }"#,
        );
        let parsed = load_config_from_str(&cfg).expect("per-tool scalar permissions should parse");

        assert_eq!(parsed.permissions.defaults.shell, PermissionMode::Ask);
        assert_eq!(parsed.permissions.defaults.edit, PermissionMode::Deny);
        assert_eq!(
            parsed.permissions.defaults.question,
            Some(PermissionMode::Allow)
        );
        assert_eq!(parsed.permissions.defaults.task, Some(PermissionMode::Ask));
        assert_eq!(
            parsed.permissions.defaults.webfetch,
            Some(PermissionMode::Deny)
        );
        assert_eq!(
            parsed.permissions.defaults.websearch,
            Some(PermissionMode::Allow)
        );
        assert_eq!(
            parsed.permissions.defaults.codesearch,
            Some(PermissionMode::Deny)
        );
        assert_eq!(parsed.permissions.defaults.lsp, Some(PermissionMode::Allow));
        assert!(parsed.permissions.rules.shell.is_empty());
        assert!(parsed.permissions.rules.edit.is_empty());
        assert!(parsed.permissions.rules.task.is_empty());
    }

    #[test]
    fn permission_rule_object_preserves_shell_allowlist_and_rules() {
        let cfg = public_minimal_config_with_permission(
            r#"{
                "*": "deny",
                bash: {
                  "git status": "allow",
                  "cargo test*": "ask",
                  "*": "deny"
                },
                edit: {
                  "docs/**": "allow",
                  "crates/harness-core/src/config.rs": "ask",
                  "*": "deny"
                },
                task: {
                  "explore": "allow",
                  "review-*": "ask",
                  "*": "deny"
                },
                shell_allowlist: {
                  executables: ["git"],
                  cwd_roots: ["."]
                }
            }"#,
        );
        let parsed = load_config_from_str(&cfg).expect("permission rule object should parse");

        assert_eq!(
            parsed.permissions.defaults.question,
            Some(PermissionMode::Deny)
        );
        assert_eq!(parsed.permissions.shell_allowlist.executables, vec!["git"]);
        assert_eq!(parsed.permissions.shell_allowlist.cwd_roots, vec!["."]);
        assert_eq!(parsed.permissions.rules.shell.len(), 3);
        assert_eq!(parsed.permissions.rules.edit.len(), 3);
        assert_eq!(parsed.permissions.rules.task.len(), 3);
    }

    #[test]
    fn permission_rule_rejects_invalid_selector_forms() {
        for permission in [
            r#"{ bash: { "/^git/": "allow" } }"#,
            r#"{ bash: { "cargo * test": "allow" } }"#,
            r#"{ edit: { "../secrets/**": "allow" } }"#,
            r#"{ edit: { "/tmp/file": "allow" } }"#,
            r#"{ edit: { "docs/*": "allow" } }"#,
            r#"{ bash: { "git status": "sometimes" } }"#,
            r#"{ bash: { "git status": { mode: "allow" } } }"#,
            r#"{ edit: { "docs/**": 1 } }"#,
            r#"{ task: { "/explore/": "allow" } }"#,
            r#"{ question: { "*": "allow" } }"#,
        ] {
            let cfg = public_minimal_config_with_permission(permission);
            load_config_from_str(&cfg).expect_err("invalid permission selector form must fail");
        }
    }

    #[test]
    fn model_limit_modalities_and_options_normalize_to_catalog_metadata() {
        let cfg = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
                timeoutMs: 30000
              },
              models: {
                "gpt-4o-mini": {
                  name: "GPT-4o mini",
                  limit: { context: 272000, input: 200000, output: 128000 },
                  modalities: { input: ["text", "image"], output: ["text"] },
                  options: { reasoning: { effort: "high" } },
                  variants: {
                    fast: {
                      name: "Fast",
                      limit: { context: 128000, input: 64000, output: 32000 },
                      modalities: { input: ["text"], output: ["text"] },
                      options: { temperature: 0.2 }
                    }
                  }
                }
              }
            }
          },
          model: "default/gpt-4o-mini",
          agent: {
            build: {
              system_prompt: "Build work",
              variant: "fast"
            }
          },
          default_agent: "build",
          permission: "allow"
        }
        "#;

        let parsed = load_config_from_str(cfg).expect("model limit config should parse");
        let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
        assert_eq!(provider.timeout_ms, 30_000);
        let model = &provider.models["gpt-4o-mini"];
        assert_eq!(model.limit.context, Some(272_000));
        assert_eq!(model.modalities.input, vec!["text", "image"]);
        assert!(model.options.contains_key("reasoning"));
        assert_eq!(model.variants["fast"].limit.output, Some(32_000));

        let metadata = resolve_profile_model_metadata(&parsed, "build")
            .expect("profile metadata should resolve");
        assert_eq!(metadata.context_window_tokens, Some(128_000));
        assert_eq!(metadata.max_input_tokens, Some(64_000));
        assert_eq!(metadata.max_output_tokens, Some(32_000));
    }

    #[test]
    fn model_limit_rejects_unknown_metadata_fields() {
        let cfg = r#"
        {
          provider: {
            default: {
              type: "openai_compatible",
              base_url: "http://127.0.0.1:8317/v1",
              api_key: "test-key",
              models: {
                "gpt-4o-mini": {
                  name: "GPT-4o mini",
                  limit: { context: 272000, training: 1 }
                }
              }
            }
          },
          model: "default/gpt-4o-mini",
          permission: "allow"
        }
        "#;

        let err = load_config_from_str(cfg).expect_err("unknown limit field must fail");
        assert!(
            err.to_string().contains("unknown field `training`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn legacy_provider_name_and_options_normalize_to_runtime_shape() {
        let cfg = r#"
        {
          providers: {
            default: {
              type: "openai_compatible",
              name: "CLIProxyAPI",
              options: {
                baseURL: "http://127.0.0.1:8317/v1",
                apiKey: "test-key",
              },
              models: {
                "gpt-4o-mini": {
                  name: "GPT-4o mini"
                }
              }
            }
          },
          agents: {
            build: {
              description: "Build work",
              model_ref: "default:gpt-4o-mini",
              tools: ["fs.read"]
            }
          },
          permissions: {
            defaults: {
              edit: "allow",
              shell: "allow",
              network: "allow"
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
            session_dir: ".agent-harness/sessions"
          },
          integrations: {
            remote_search: {
              endpoint: "https://mcp.exa.ai/mcp"
            }
          }
        }
        "#;

        let parsed = load_config_from_str(cfg).expect("legacy provider config should parse");
        let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
        assert_eq!(provider.name.as_deref(), Some("CLIProxyAPI"));
        assert_eq!(provider.base_url, "http://127.0.0.1:8317/v1");
        assert_eq!(provider.api_key, "test-key");
        assert_eq!(provider.models["gpt-4o-mini"].display_name, "GPT-4o mini");

        let metadata = resolve_profile_model_metadata(&parsed, "build")
            .expect("profile metadata should resolve");
        assert_eq!(metadata.provider_display_label, "CLIProxyAPI");
    }

    #[test]
    fn top_level_legacy_agent_key_is_translated() {
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
          agent: {
            plan: {
              description: "Planning work",
              model_ref: "default:gpt-4o-mini",
              tools: ["fs.read"]
            }
          },
          permissions: {
            defaults: {
              edit: "allow",
              shell: "allow",
              network: "allow"
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
            session_dir: ".agent-harness/sessions"
          },
          integrations: {
            remote_search: {
              endpoint: "https://mcp.exa.ai/mcp"
            }
          }
        }
        "#;

        let parsed = load_config_from_str(cfg).expect("canonical `agent` key should parse");
        assert!(parsed.agents.contains_key("plan"));
    }

    #[test]
    fn invalid_explicit_default_profile_falls_back_to_build_when_available() {
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
          agents: {
            build: {
              description: "Build work",
              model_ref: "default:gpt-4o-mini",
              tools: ["fs.read"]
            },
            plan: {
              description: "Planning work",
              model_ref: "default:gpt-4o-mini",
              tools: ["fs.read"]
            }
          },
          permissions: {
            defaults: {
              edit: "allow",
              shell: "allow",
              network: "allow"
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
            session_dir: ".agent-harness/sessions"
          },
          integrations: {
            remote_search: {
              endpoint: "https://mcp.exa.ai/mcp"
            }
          },
          ui: {
            default_profile: "ops"
          }
        }
        "#;

        let parsed = load_config_from_str(cfg).expect("invalid default should fall back to build");
        assert_eq!(parsed.ui.default_profile.as_deref(), Some("build"));
        assert_eq!(parsed.default_agent.as_deref(), Some("build"));
    }

    #[test]
    fn relative_paths_remain_cwd_relative_when_loading_from_file() {
        let _lock = CONFIG_DISCOVERY_TEST_LOCK
            .lock()
            .expect("lock discovery tests");
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
              agents: {
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
    fn schema_uses_runtime_first_public_contract() {
        let schema = harness_schema_pretty_json().expect("schema generation should succeed");
        let parsed: serde_json::Value =
            serde_json::from_str(&schema).expect("schema output should be valid json");
        let properties = parsed
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .expect("schema should contain properties");

        assert!(properties.contains_key("$schema"));
        assert!(properties.contains_key("provider"));
        assert!(properties.contains_key("agent"));
        assert!(properties.contains_key("default_agent"));
        assert!(properties.contains_key("permission"));
        assert!(properties.contains_key("model"));
        assert!(properties.contains_key("small_model"));
        assert!(properties.contains_key("mcp"));
        assert!(properties.contains_key("skills"));
        assert!(properties.contains_key("instructions"));
        assert!(!properties.contains_key("categories"));
        assert!(!properties.contains_key("profiles"));
        let runtime = properties
            .get("runtime")
            .and_then(|value| value.get("allOf"))
            .and_then(serde_json::Value::as_array)
            .expect("public runtime schema should expose the narrow runtime settings surface");
        let runtime_ref = runtime
            .first()
            .and_then(|value| value.get("$ref"))
            .and_then(serde_json::Value::as_str);
        assert_eq!(
            runtime_ref,
            Some("#/definitions/PublicRuntimeSettingsConfig")
        );
        assert!(!properties.contains_key("integrations"));
    }

    #[test]
    fn public_top_level_skills_translate_into_runtime_config() {
        let parsed = load_config_from_str(
            r#"
            {
              model: "default:gpt-4o-mini",
              provider: {
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
              skills: {
                disabled: ["skill:project:disabled-doctor"],
                walkToGitRoot: false,
                permissions: {
                  "internal-*": "deny"
                }
              }
            }
            "#,
        )
        .expect("runtime config with top-level skills should parse");

        assert!(!parsed.skills.walk_to_git_root);
        assert_eq!(
            parsed.skills.project_roots,
            vec![
                PathBuf::from(".agent-harness/skills"),
                PathBuf::from(".harness/skills")
            ]
        );
        assert_eq!(
            parsed.skills.global_roots,
            vec![PathBuf::from("~/.config/agent-harness/skills")]
        );
        assert_eq!(
            parsed.skills.disabled,
            vec!["skill:project:disabled-doctor".to_string()]
        );
        assert_eq!(
            parsed.skills.permissions.get("internal-*"),
            Some(&PermissionMode::Deny)
        );
    }

    #[test]
    fn json5_comments_trailing_commas_and_schema_field_parse() {
        let cfg = r#"
        {
          // optional editor schema hint
            "$schema": "./config.json",
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
          agents: {
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
        assert_eq!(parsed.schema.as_deref(), Some("./config.json"));
        assert_eq!(parsed.agents["deep"].model_ref, "default:gpt-4o-mini");
    }

    #[test]
    fn resolve_config_path_prefers_explicit_path_over_discovery() {
        let _lock = CONFIG_DISCOVERY_TEST_LOCK
            .lock()
            .expect("lock discovery tests");
        let temp = tempfile::tempdir().expect("tempdir");
        let xdg_root = temp.path().join("xdg");
        let xdg_config = xdg_root.join("harness/harness.jsonc");
        let cwd_config = temp.path().join("harness.jsonc");
        let explicit_config = temp.path().join("explicit.jsonc");

        fs::create_dir_all(xdg_config.parent().expect("xdg parent"))
            .expect("create xdg config dir");
        fs::write(&xdg_config, "xdg").expect("write xdg config");
        fs::write(&cwd_config, "cwd").expect("write cwd config");
        fs::write(&explicit_config, "explicit").expect("write explicit config");

        let context = discovery_context(temp.path(), Some(&xdg_root));

        assert_eq!(
            resolve_config_path_with_context(Some(&explicit_config), &context.discovery),
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
        let xdg_config = xdg_root.join("harness/harness.jsonc");
        let cwd_config = temp.path().join("harness.jsonc");

        fs::create_dir_all(xdg_config.parent().expect("xdg parent"))
            .expect("create xdg config dir");
        fs::write(&xdg_config, "xdg").expect("write xdg config");
        fs::write(&cwd_config, "cwd").expect("write cwd config");

        let context = discovery_context(temp.path(), Some(&xdg_root));

        assert_eq!(
            resolve_config_path_with_context(None, &context.discovery),
            Some(cwd_config)
        );
    }

    #[test]
    fn resolve_config_layer_paths_orders_global_then_local() {
        let _lock = CONFIG_DISCOVERY_TEST_LOCK
            .lock()
            .expect("lock discovery tests");
        let temp = tempfile::tempdir().expect("tempdir");
        let xdg_root = temp.path().join("xdg");
        let xdg_config = xdg_root.join("harness/harness.jsonc");
        let cwd_config = temp.path().join("harness.jsonc");

        fs::create_dir_all(xdg_config.parent().expect("xdg parent"))
            .expect("create xdg config dir");
        fs::write(
            &xdg_config,
            "{ providers: {}, permissions: {}, runtime: {}, integrations: {}, agents: {} }",
        )
        .expect("write xdg config");
        fs::write(&cwd_config, "{ agents: {} }").expect("write cwd config");

        let context = discovery_context(temp.path(), Some(&xdg_root));

        assert_eq!(
            discovery::resolve_config_layer_paths_with_context(None, &context.discovery),
            vec![xdg_config, cwd_config]
        );
    }

    #[test]
    fn resolve_config_layer_paths_include_env_and_project_ancestor_layers() {
        let _lock = CONFIG_DISCOVERY_TEST_LOCK
            .lock()
            .expect("lock discovery tests");
        let temp = tempfile::tempdir().expect("tempdir");
        let xdg_root = temp.path().join("xdg");
        let repo = temp.path().join("repo");
        let nested = repo.join("workspace/app");
        let env_config = temp.path().join("env/harness.env.jsonc");
        let xdg_config = xdg_root.join("harness/harness.jsonc");
        let repo_config = repo.join("harness.jsonc");
        let repo_dot_config = repo.join(".agent-harness/harness.jsonc");
        let nested_config = nested.join("harness.json");
        let nested_dot_config = nested.join(".agent-harness/harness.json");

        fs::create_dir_all(xdg_config.parent().expect("xdg parent")).expect("create xdg dir");
        fs::create_dir_all(
            repo_dot_config
                .parent()
                .expect("repo .agent-harness parent"),
        )
        .expect("create repo .agent-harness dir");
        fs::create_dir_all(
            nested_dot_config
                .parent()
                .expect("nested .agent-harness parent"),
        )
        .expect("create nested .agent-harness dir");
        fs::create_dir_all(env_config.parent().expect("env parent")).expect("create env dir");
        fs::create_dir_all(repo.join(".git")).expect("create repo git dir");

        for path in [
            &xdg_config,
            &env_config,
            &repo_config,
            &repo_dot_config,
            &nested_config,
            &nested_dot_config,
        ] {
            fs::write(path, "{}").expect("write placeholder config");
        }

        let mut context = discovery_context(&nested, Some(&xdg_root));
        context.discovery.runtime_config_path = Some(env_config.clone());
        assert_eq!(
            discovery::resolve_config_layer_paths_with_context(None, &context.discovery),
            vec![
                xdg_config,
                env_config,
                repo_config,
                repo_dot_config,
                nested_config,
                nested_dot_config,
            ]
        );
    }

    #[test]
    fn load_resolved_config_merges_global_then_local_and_prefers_local_values() {
        let _lock = CONFIG_DISCOVERY_TEST_LOCK
            .lock()
            .expect("lock discovery tests");
        let temp = tempfile::tempdir().expect("tempdir");
        let xdg_root = temp.path().join("xdg");
        let xdg_config = xdg_root.join("harness/harness.jsonc");
        let cwd_config = temp.path().join("harness.jsonc");

        fs::create_dir_all(xdg_config.parent().expect("xdg parent"))
            .expect("create xdg config dir");
        fs::write(
            &xdg_config,
            r#"
            {
              providers: {
                default: {
                  type: "openai_compatible",
                  base_url: "http://127.0.0.1:8317/v1",
                  api_key: "test-key",
                  api_mode: "responses",
                  timeout_ms: 60000,
                  models: {
                    "gpt-4o-mini": {
                      display_name: "GPT-4o mini",
                    },
                  },
                },
              },
              permissions: {
                defaults: {
                  edit: "ask",
                  shell: "deny",
                  network: "allow",
                },
                shell_allowlist: {
                  executables: ["git"],
                  cwd_roots: ["."],
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
            "#,
        )
        .expect("write xdg config");
        fs::write(
            &cwd_config,
            r#"
            {
              agents: {
                build: {
                  description: "Build profile",
                  model_ref: "default:gpt-4o-mini",
                  tools: ["read"],
                },
              },
              permissions: {
                defaults: {
                  shell: "allow",
                },
              },
              ui: {
                default_profile: "build",
              },
            }
            "#,
        )
        .expect("write cwd config");

        let context = discovery_context(temp.path(), Some(&xdg_root));

        let loaded = load_resolved_config_with_context(None, &context)
            .expect("load resolved config")
            .expect("merged config should resolve");

        assert_eq!(loaded.paths, vec![xdg_config.clone(), cwd_config.clone()]);
        assert_eq!(loaded.config.ui.default_profile.as_deref(), Some("build"));
        assert!(matches!(
            loaded.config.permissions.defaults.shell,
            PermissionMode::Allow
        ));
        assert!(loaded.config.agents.contains_key("build"));
        assert_eq!(loaded.primary_path(), Some(cwd_config.as_path()));
    }

    #[test]
    fn load_resolved_config_applies_harness_config_content_last() {
        let _lock = CONFIG_DISCOVERY_TEST_LOCK
            .lock()
            .expect("lock discovery tests");
        let temp = tempfile::tempdir().expect("tempdir");
        let config_path = temp.path().join("harness.jsonc");
        fs::write(
            &config_path,
            serde_json::json!({
                "provider": {
                    "default": {
                        "type": "openai_compatible",
                        "base_url": "http://127.0.0.1:1/v1",
                        "api_key": "DUMMY",
                        "models": {
                            "gpt-4o": { "name": "GPT-4o" },
                            "gpt-4o-mini": { "name": "GPT-4o mini" }
                        }
                    }
                },
                "model": "default/gpt-4o",
                "small_model": "default/gpt-4o-mini",
                "default_agent": "build",
                "permission": {
                    "bash": "deny",
                    "edit": "allow"
                }
            })
            .to_string(),
        )
        .expect("write config");

        let mut context = discovery_context(temp.path(), None);
        context.runtime_content =
            Some("{ permission: { bash: \"allow\" }, default_agent: \"plan\" }".to_string());
        let loaded = load_resolved_config_with_context(None, &context)
            .expect("load config")
            .expect("config should resolve");
        assert!(matches!(
            loaded.config.permissions.defaults.shell,
            PermissionMode::Allow
        ));
        assert_eq!(loaded.config.default_agent.as_deref(), Some("plan"));
    }

    #[test]
    fn load_resolved_config_explicit_path_bypasses_discovery_layers() {
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
        fs::write(
            &xdg_config,
            "{ providers: {}, permissions: {}, runtime: {}, integrations: {}, agents: {} }",
        )
        .expect("write xdg config");
        fs::write(&cwd_config, "{ agents: {} }").expect("write cwd config");
        fs::write(
            &explicit_config,
            config_fixture(&deep_profile(r#"tools: ["read"],"#), "test-key", None, None),
        )
        .expect("write explicit config");

        let context = discovery_context(temp.path(), Some(&xdg_root));

        let loaded = load_resolved_config_with_context(Some(&explicit_config), &context)
            .expect("load explicit config")
            .expect("explicit config should resolve");

        assert_eq!(loaded.paths, vec![explicit_config]);
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
    fn runtime_profile_max_iters_defaults_to_unbounded() {
        let cfg = config_fixture(&deep_profile(r#"tools: ["read"],"#), "test-key", None, None);

        let parsed =
            load_config_from_str(&cfg).expect("config with default tool failure mode must parse");
        assert_eq!(parsed.agents["deep"].max_iters, None);
        assert_eq!(
            parsed.agents["deep"].tool_failure_mode,
            ToolFailureMode::ContinueAsToolMessage
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
            parsed.agents["deep"].tool_failure_mode,
            ToolFailureMode::ContinueAsToolMessage
        );
        assert_eq!(parsed.agents["deep"].max_iters, Some(24));
        assert_eq!(
            parsed.agents["deep"].system_prompt.as_deref(),
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

        let parsed = loader::load_config_from_str_with_lookup(&cfg, &|_| None)
            .expect("config with fallback env reference must parse");
        let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
        assert_eq!(provider.api_key, "fallback-key");
    }

    #[test]
    fn env_var_default_fallback_uses_fallback_for_empty_var() {
        let cfg = config_fixture(
            &deep_profile(r#"tools: ["fs.read"],"#),
            "${HARNESS_CONFIG_TEST_API_KEY_FALLBACK:-fallback-key}",
            None,
            None,
        );

        let parsed = loader::load_config_from_str_with_lookup(&cfg, &|_| Some(String::new()))
            .expect("config with empty fallback env reference must parse");
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

        let parsed = loader::load_config_from_str_with_lookup(&cfg, &|_| Some(String::new()))
            .expect("config with empty env reference should use fallback value");
        let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
        assert_eq!(provider.api_key, "fallback-key");
    }

    #[test]
    fn missing_required_env_var_is_an_error() {
        let cfg = config_fixture(
            &deep_profile(r#"tools: ["fs.read"],"#),
            "${HARNESS_CONFIG_TEST_API_KEY_REQUIRED}",
            None,
            None,
        );

        let err = loader::load_config_from_str_with_lookup(&cfg, &|_| None)
            .expect_err("missing required env variable should fail");
        assert_eq!(
            err.to_string(),
            "environment variable `HARNESS_CONFIG_TEST_API_KEY_REQUIRED` referenced in config is not set"
        );
    }

    #[test]
    fn missing_openai_api_key_errors_even_for_cliproxy_loopback_base_url() {
        let err =
            loader::resolve_string_reference_with_lookup("${OPENAI_API_KEY}", None, &|_| None)
                .expect_err("loopback providers should still require OPENAI_API_KEY");

        assert_eq!(
            err.to_string(),
            "environment variable `OPENAI_API_KEY` referenced in config is not set"
        );
    }

    #[test]
    fn configured_openai_api_key_env_reference_resolves_without_fallback() {
        let resolved =
            loader::resolve_string_reference_with_lookup("${OPENAI_API_KEY}", None, &|_| {
                Some("test-openai-api-key".to_string())
            })
            .expect("OPENAI_API_KEY should resolve when it is set");

        assert_eq!(resolved, "test-openai-api-key");
    }

    #[test]
    fn upstream_env_reference_uses_empty_string_when_missing() {
        let cfg = config_fixture(
            &deep_profile(r#"tools: ["fs.read"],"#),
            "{env:HARNESS_CONFIG_TEST_OPTIONAL_EMPTY}",
            None,
            None,
        );

        let parsed = loader::load_config_from_str_with_lookup(&cfg, &|_| None)
            .expect("upstream env reference should parse");
        let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
        assert_eq!(provider.api_key, "");
    }

    #[test]
    fn upstream_file_reference_resolves_relative_to_config_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let config_dir = temp.path().join("nested");
        let secret_path = config_dir.join("secrets/api-key.txt");
        let config_path = config_dir.join("harness.jsonc");
        fs::create_dir_all(secret_path.parent().expect("secret parent"))
            .expect("create secret parent");
        fs::write(&secret_path, "file-key").expect("write secret file");
        fs::write(
            &config_path,
            config_fixture(
                &deep_profile(r#"tools: ["fs.read"],"#),
                "{file:secrets/api-key.txt}",
                None,
                None,
            ),
        )
        .expect("write config");

        let parsed =
            load_config_from_file(&config_path).expect("file reference config should parse");
        let ProviderConfig::OpenAiCompatible(provider) = parsed.providers.get("default").unwrap();
        assert_eq!(provider.api_key, "file-key");
    }

    #[test]
    fn load_config_from_file_can_define_agent_from_markdown_frontmatter() {
        let _lock = CONFIG_DISCOVERY_TEST_LOCK
            .lock()
            .expect("lock discovery tests");
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("create repo dir");
        fs::create_dir_all(repo.join(".git")).expect("create git dir");

        let config_path = repo.join("harness.jsonc");
        fs::write(&config_path, config_fixture("", "test-key", None, None)).expect("write config");
        write_agent_markdown(
            &repo,
            "build",
            r#"---
{
  description: "Build from markdown",
  model_ref: "default:gpt-4o-mini",
  tools: ["read", "grep"],
  max_iters: 18
}
---

Execute from markdown only."#,
        );

        let parsed = load_config_from_file(&config_path).expect("markdown-only agent config");
        let build = parsed.agents.get("build").expect("build agent");
        assert_eq!(build.description, "Build from markdown");
        assert_eq!(build.model_ref, "default:gpt-4o-mini");
        assert_eq!(build.tools, vec!["read", "grep"]);
        assert_eq!(build.max_iters, Some(18));
        assert_eq!(
            build.system_prompt.as_deref(),
            Some("Execute from markdown only.")
        );
    }

    #[test]
    fn load_config_from_file_still_accepts_legacy_agent_harness_prompt_dir() {
        let _lock = CONFIG_DISCOVERY_TEST_LOCK
            .lock()
            .expect("lock discovery tests");
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("create repo dir");
        fs::create_dir_all(repo.join(".git")).expect("create git dir");

        let config_path = repo.join("harness.jsonc");
        fs::write(&config_path, config_fixture("", "test-key", None, None)).expect("write config");
        write_legacy_agent_markdown(
            &repo,
            "build",
            r#"---
{
  description: "Legacy build prompt",
  model_ref: "default:gpt-4o-mini"
}
---

Legacy prompt body."#,
        );

        let parsed = load_config_from_file(&config_path)
            .expect("legacy prompt dir should remain compatible");
        assert_eq!(
            parsed.agents["build"].system_prompt.as_deref(),
            Some("Legacy prompt body.")
        );
    }

    #[test]
    fn load_config_from_file_keeps_inline_system_prompt_over_markdown_prompt() {
        let _lock = CONFIG_DISCOVERY_TEST_LOCK
            .lock()
            .expect("lock discovery tests");
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("create repo dir");
        fs::create_dir_all(repo.join(".git")).expect("create git dir");

        let config_path = repo.join("harness.jsonc");
        fs::write(
            &config_path,
            config_fixture(
                &deep_profile(
                    r#"
                    system_prompt: "Inline prompt",
                    tools: ["read"],
                    "#,
                ),
                "test-key",
                None,
                None,
            ),
        )
        .expect("write config");
        write_agent_markdown(&repo, "deep", "Markdown prompt body.");

        let parsed = load_config_from_file(&config_path).expect("config with markdown prompt");
        assert_eq!(
            parsed.agents["deep"].system_prompt.as_deref(),
            Some("Inline prompt")
        );
    }

    #[test]
    fn load_config_from_file_discovers_project_agents_md_separately() {
        let _lock = CONFIG_DISCOVERY_TEST_LOCK
            .lock()
            .expect("lock discovery tests");
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("create repo dir");
        fs::create_dir_all(repo.join(".git")).expect("create git dir");

        let config_path = repo.join("harness.jsonc");
        fs::write(
            &config_path,
            config_fixture(&deep_profile(r#"tools: ["read"],"#), "test-key", None, None),
        )
        .expect("write config");
        fs::write(repo.join("AGENTS.md"), "Project instructions live here.")
            .expect("write AGENTS.md");

        let parsed = load_config_from_file(&config_path).expect("config with project instructions");
        assert_eq!(parsed.instruction_files.len(), 1);
        assert_eq!(
            parsed.instruction_files[0].content,
            "Project instructions live here."
        );
        assert!(parsed.instruction_files[0].path.ends_with("AGENTS.md"));
    }

    #[test]
    fn load_config_from_file_discovers_repo_assets_when_cwd_differs() {
        let _lock = CONFIG_DISCOVERY_TEST_LOCK
            .lock()
            .expect("lock discovery tests");
        let temp = tempfile::tempdir().expect("tempdir");
        let outside = temp.path().join("outside");
        let repo = temp.path().join("repo");
        let config_dir = repo.join("configs").join("nested");
        fs::create_dir_all(&outside).expect("create outside dir");
        fs::create_dir_all(&config_dir).expect("create config dir");
        fs::create_dir_all(repo.join(".git")).expect("create git dir");

        let config_path = config_dir.join("harness.jsonc");
        fs::write(&config_path, config_fixture("", "test-key", None, None)).expect("write config");
        write_agent_markdown(
            &repo,
            "build",
            r#"---
{
  description: "Build from repo root markdown",
  model_ref: "default:gpt-4o-mini"
}
---

Prompt discovered from the config repo root."#,
        );
        fs::write(repo.join("AGENTS.md"), "Repo-root instructions.").expect("write repo AGENTS.md");

        let parsed = load_config_from_file(&config_path).expect("discover repo-root assets");
        let build = parsed.agents.get("build").expect("build agent");
        assert_eq!(build.description, "Build from repo root markdown");
        assert_eq!(
            build.system_prompt.as_deref(),
            Some("Prompt discovered from the config repo root.")
        );
        assert_eq!(parsed.instruction_files.len(), 1);
        assert_eq!(
            parsed.instruction_files[0].content,
            "Repo-root instructions."
        );
        assert_eq!(parsed.instruction_files[0].path, repo.join("AGENTS.md"));
    }

    #[test]
    fn load_config_from_file_ignores_unmatched_prompt_only_markdown_assets() {
        let _lock = CONFIG_DISCOVERY_TEST_LOCK
            .lock()
            .expect("lock discovery tests");
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("create repo dir");
        fs::create_dir_all(repo.join(".git")).expect("create git dir");

        let config_path = repo.join("harness.jsonc");
        fs::write(
            &config_path,
            config_fixture(&deep_profile(r#"tools: ["read"],"#), "test-key", None, None),
        )
        .expect("write config");
        write_agent_markdown(&repo, "stray", "Prompt body without frontmatter metadata.");

        let parsed = load_config_from_file(&config_path).expect("prompt-only stray asset ignored");
        assert!(!parsed.agents.contains_key("stray"));
        assert!(parsed.agents.contains_key("deep"));
    }

    #[test]
    fn load_config_from_file_rejects_invalid_markdown_frontmatter() {
        let _lock = CONFIG_DISCOVERY_TEST_LOCK
            .lock()
            .expect("lock discovery tests");
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("create repo dir");
        fs::create_dir_all(repo.join(".git")).expect("create git dir");

        let config_path = repo.join("harness.jsonc");
        fs::write(
            &config_path,
            config_fixture(&deep_profile(r#"tools: ["read"],"#), "test-key", None, None),
        )
        .expect("write config");
        write_agent_markdown(
            &repo,
            "deep",
            r#"---
{ description: }
---

Broken prompt."#,
        );

        let err = load_config_from_file(&config_path).expect_err("invalid markdown should fail");
        assert!(err.to_string().contains("invalid markdown frontmatter"));
        assert!(err.to_string().contains("deep.md"));
    }

    #[test]
    fn load_config_from_file_rejects_legacy_plan_markdown_frontmatter() {
        let _lock = CONFIG_DISCOVERY_TEST_LOCK
            .lock()
            .expect("lock discovery tests");
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("create repo dir");
        fs::create_dir_all(repo.join(".git")).expect("create git dir");

        let config_path = repo.join("harness.jsonc");
        fs::write(
            &config_path,
            config_fixture(&deep_profile(r#"tools: ["read"],"#), "test-key", None, None),
        )
        .expect("write config");
        write_agent_markdown(
            &repo,
            "deep",
            r#"---
{
  description: "Legacy plan prompt",
  model_ref: "default:gpt-4o-mini",
  planMode: true
}
---

Legacy prompt."#,
        );

        let err =
            load_config_from_file(&config_path).expect_err("legacy plan frontmatter should fail");
        assert!(err.to_string().contains("invalid markdown frontmatter"));
        assert!(err.to_string().contains("unknown field `planMode`"));
    }
}
