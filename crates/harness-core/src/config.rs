use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use schemars::{schema_for, JsonSchema};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use thiserror::Error;

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

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicRuntimeConfig {
    #[serde(rename = "$schema", default)]
    pub schema: Option<String>,
    #[serde(default)]
    pub provider: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default, alias = "smallModel")]
    pub small_model: Option<String>,
    #[serde(default)]
    pub agent: BTreeMap<String, PublicAgentConfig>,
    #[serde(default, alias = "defaultAgent")]
    pub default_agent: Option<String>,
    #[serde(default)]
    pub permission: PublicPermissionConfig,
    #[serde(default)]
    pub mcp: BTreeMap<String, McpServerConfig>,
    #[serde(default)]
    pub instructions: Option<InstructionList>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicAgentConfig {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, alias = "systemPrompt")]
    pub system_prompt: Option<String>,
    #[serde(default, alias = "model_ref", alias = "modelRef")]
    pub model: Option<String>,
    #[serde(default, alias = "smallModel")]
    pub use_small_model: bool,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default, alias = "permissions")]
    pub permission: Option<PublicProfilePermissions>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicPermissionConfig {
    #[serde(default)]
    pub edit: Option<PermissionMode>,
    #[serde(default, alias = "shell")]
    pub bash: Option<PermissionMode>,
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
    #[serde(default, skip_serializing)]
    #[schemars(skip)]
    pub network: Option<PermissionMode>,
    #[serde(rename = "shell_allowlist", alias = "shellAllowlist", default)]
    pub shell_allowlist: Option<ShellAllowlist>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicProfilePermissions {
    #[serde(default)]
    pub edit: Option<PermissionMode>,
    #[serde(default, alias = "shell")]
    pub bash: Option<PermissionMode>,
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
    #[serde(default, skip_serializing)]
    #[schemars(skip)]
    pub network: Option<PermissionMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum InstructionList {
    Single(String),
    Many(Vec<String>),
}

impl Default for InstructionList {
    fn default() -> Self {
        Self::Many(Vec::new())
    }
}

impl InstructionList {
    fn entries(&self) -> Vec<String> {
        match self {
            Self::Single(value) => vec![value.clone()],
            Self::Many(values) => values.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicTuiConfig {
    #[serde(rename = "$schema", default)]
    pub schema: Option<String>,
    #[serde(rename = "keybinds", alias = "keybindings", default)]
    pub keybindings: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct MarkdownAgentFrontmatter {
    pub description: Option<String>,
    #[serde(alias = "systemPrompt")]
    pub system_prompt: Option<String>,
    #[serde(rename = "model_ref", alias = "modelRef")]
    pub model_ref: Option<String>,
    pub variant: Option<String>,
    pub temperature: Option<f32>,
    pub permissions: Option<ProfilePermissions>,
    #[serde(alias = "maxIters")]
    pub max_iters: Option<usize>,
    #[serde(alias = "toolFailureMode")]
    pub tool_failure_mode: Option<ToolFailureMode>,
    #[serde(alias = "planMode")]
    pub plan_mode: Option<bool>,
    #[serde(alias = "exitTargetProfile")]
    pub exit_target_profile: Option<String>,
    pub tools: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct MarkdownAgentFile {
    frontmatter: MarkdownAgentFrontmatter,
    prompt_body: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HarnessConfig {
    #[serde(rename = "$schema", default)]
    pub schema: Option<String>,
    pub providers: BTreeMap<String, ProviderConfig>,
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

#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub config: HarnessConfig,
    pub paths: Vec<PathBuf>,
}

impl LoadedConfig {
    pub fn primary_path(&self) -> Option<&Path> {
        self.paths.last().map(PathBuf::as_path)
    }

    pub fn path_display(&self) -> String {
        match self.paths.as_slice() {
            [] => "<none>".to_string(),
            [path] => path.display().to_string(),
            paths => paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(" + "),
        }
    }
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

        for (agent_name, agent) in &self.agents {
            let Some((provider_name, model_name)) = parse_model_ref(&agent.model_ref) else {
                return Err(ConfigError::InvalidReference(format!(
                    "agent `{agent_name}` has invalid `model_ref` `{}`; use `<provider>:<model>`",
                    agent.model_ref
                )));
            };

            let Some(provider) = self.providers.get(provider_name) else {
                return Err(ConfigError::InvalidReference(format!(
                    "agent `{agent_name}` references unknown provider `{provider_name}` in `model_ref` `{}`; available providers: {}",
                    agent.model_ref,
                    format_name_list(self.providers.keys().map(|name| name.as_str()))
                )));
            };

            let models = provider.models();
            let Some(model) = models.get(model_name) else {
                return Err(ConfigError::InvalidReference(format!(
                    "agent `{agent_name}` references unknown model `{model_name}` in `model_ref` `{}`; available models for provider `{provider_name}`: {}",
                    agent.model_ref,
                    format_name_list(models.keys().map(|name| name.as_str()))
                )));
            };

            if let Some(variant_name) = agent.variant.as_deref() {
                let Some(variant) = model.variants.get(variant_name) else {
                    return Err(ConfigError::InvalidReference(format!(
                        "agent `{agent_name}` references unknown variant `{variant_name}` for model `{}`; available variants: {}",
                        agent.model_ref,
                        format_name_list(model.variants.keys().map(|name| name.as_str()))
                    )));
                };

                if variant.disabled {
                    return Err(ConfigError::InvalidReference(format!(
                        "agent `{agent_name}` references disabled variant `{variant_name}` for model `{}`; choose an enabled variant",
                        agent.model_ref
                    )));
                }
            }

            if let Some(target_profile) = agent.exit_target_profile.as_deref() {
                if !self.agents.contains_key(target_profile) {
                    return Err(ConfigError::InvalidReference(format!(
                        "agent `{agent_name}` references unknown `exit_target_profile` `{target_profile}`; available agents: {}",
                        format_name_list(self.agents.keys().map(|name| name.as_str()))
                    )));
                }
            }
        }

        if let Some(default_profile) = self.ui.default_profile.as_deref() {
            if !self.agents.contains_key(default_profile) {
                if self.agents.contains_key("build") {
                    self.ui.default_profile = Some("build".to_string());
                    self.default_agent = Some("build".to_string());
                } else {
                    return Err(ConfigError::InvalidReference(format!(
                        "ui.default_profile references unknown agent `{default_profile}`; available agents: {}",
                        format_name_list(self.agents.keys().map(|name| name.as_str()))
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
            if server_name.trim().is_empty() {
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
                    if command.iter().any(|token| token.trim().is_empty()) {
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
                        if key.trim().is_empty() {
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
                    if endpoint.trim().is_empty() {
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
                        if key.trim().is_empty() {
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

fn resolve_discovered_prompt_assets(
    parsed: &mut HarnessConfig,
    config_path: &Path,
) -> Result<(), ConfigError> {
    parsed.agents = merge_configured_and_markdown_agents(&parsed.agents, config_path)?;
    parsed.instruction_files = discover_instruction_files(config_path)?;
    Ok(())
}

fn merge_configured_and_markdown_agents(
    configured: &BTreeMap<String, ProfileConfig>,
    config_path: &Path,
) -> Result<BTreeMap<String, ProfileConfig>, ConfigError> {
    let discovered = discover_markdown_agents(config_path)?;
    let agent_names = discovered
        .keys()
        .chain(configured.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut merged = BTreeMap::new();

    for name in agent_names {
        let profile = match (discovered.get(&name), configured.get(&name)) {
            (Some(markdown), Some(config)) => {
                Some(merge_markdown_agent_with_config(config, markdown))
            }
            (None, Some(config)) => Some(config.clone()),
            (Some(markdown), None) => profile_from_markdown_agent(markdown)?,
            (None, None) => None,
        };

        if let Some(profile) = profile {
            merged.insert(name, profile);
        }
    }

    Ok(merged)
}

fn merge_markdown_agent_with_config(
    config: &ProfileConfig,
    markdown: &MarkdownAgentFile,
) -> ProfileConfig {
    let prompt = config
        .system_prompt
        .clone()
        .or_else(|| markdown.prompt_body.clone())
        .or_else(|| markdown.frontmatter.system_prompt.clone());

    ProfileConfig {
        description: config.description.clone(),
        system_prompt: prompt,
        model_ref: config.model_ref.clone(),
        variant: config.variant.clone(),
        temperature: config.temperature,
        permissions: config.permissions.clone(),
        max_iters: config.max_iters,
        tool_failure_mode: config.tool_failure_mode,
        plan_mode: config.plan_mode,
        exit_target_profile: config.exit_target_profile.clone(),
        tools: config.tools.clone(),
    }
}

fn profile_from_markdown_agent(
    markdown: &MarkdownAgentFile,
) -> Result<Option<ProfileConfig>, ConfigError> {
    let Some(description) = markdown.frontmatter.description.clone() else {
        return Ok(None);
    };
    let Some(model_ref) = markdown.frontmatter.model_ref.clone() else {
        return Ok(None);
    };

    Ok(Some(ProfileConfig {
        description,
        system_prompt: markdown
            .prompt_body
            .clone()
            .or_else(|| markdown.frontmatter.system_prompt.clone()),
        model_ref,
        variant: markdown.frontmatter.variant.clone(),
        temperature: markdown.frontmatter.temperature,
        permissions: markdown.frontmatter.permissions.clone(),
        max_iters: markdown
            .frontmatter
            .max_iters
            .unwrap_or_else(default_max_iters),
        tool_failure_mode: markdown.frontmatter.tool_failure_mode.unwrap_or_default(),
        plan_mode: markdown.frontmatter.plan_mode.unwrap_or(false),
        exit_target_profile: markdown.frontmatter.exit_target_profile.clone(),
        tools: markdown.frontmatter.tools.clone().unwrap_or_default(),
    }))
}

fn discover_markdown_agents(
    config_path: &Path,
) -> Result<BTreeMap<String, MarkdownAgentFile>, ConfigError> {
    let mut agents = BTreeMap::new();

    for dir in agent_prompt_search_dirs(config_path) {
        if !dir.exists() {
            continue;
        }

        for file in markdown_files_in_dir(&dir)? {
            let name = file
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::trim)
                .filter(|stem| !stem.is_empty())
                .ok_or_else(|| {
                    ConfigError::InvalidReference(format!(
                        "agent markdown `{}` must have a valid UTF-8 file stem",
                        file.display()
                    ))
                })?
                .to_string();
            if agents.contains_key(&name) {
                continue;
            }

            let content =
                fs::read_to_string(&file).map_err(|source| ConfigError::ReadMarkdownAsset {
                    path: file.display().to_string(),
                    source,
                })?;
            let (frontmatter, prompt_body) =
                parse_markdown_frontmatter::<MarkdownAgentFrontmatter>(&file, &content)?;
            agents.insert(
                name,
                MarkdownAgentFile {
                    frontmatter,
                    prompt_body: (!prompt_body.is_empty()).then_some(prompt_body),
                },
            );
        }
    }

    Ok(agents)
}

fn discover_instruction_files(config_path: &Path) -> Result<Vec<InstructionFile>, ConfigError> {
    let mut instructions = Vec::new();
    let mut seen = BTreeSet::new();

    for path in instruction_search_paths(config_path) {
        if !path.exists() || !seen.insert(path.clone()) {
            continue;
        }

        let content =
            fs::read_to_string(&path).map_err(|source| ConfigError::ReadMarkdownAsset {
                path: path.display().to_string(),
                source,
            })?;
        let content = content.trim().to_string();
        if content.is_empty() {
            continue;
        }

        instructions.push(InstructionFile { path, content });
    }

    Ok(instructions)
}

fn parse_markdown_frontmatter<T>(path: &Path, content: &str) -> Result<(T, String), ConfigError>
where
    T: DeserializeOwned + Default,
{
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return Ok((T::default(), content.trim().to_string()));
    }

    let mut frontmatter_lines = Vec::new();
    let mut found_closing = false;
    for line in &mut lines {
        if line == "---" {
            found_closing = true;
            break;
        }
        frontmatter_lines.push(line);
    }

    if !found_closing {
        return Err(ConfigError::InvalidMarkdownFrontmatter {
            path: path.display().to_string(),
            reason: "frontmatter must end with `---`".to_string(),
        });
    }

    let frontmatter_text = frontmatter_lines.join("\n");
    let frontmatter = if frontmatter_text.trim().is_empty() {
        T::default()
    } else {
        json5::from_str(&frontmatter_text).map_err(|err| {
            ConfigError::InvalidMarkdownFrontmatter {
                path: path.display().to_string(),
                reason: err.to_string(),
            }
        })?
    };

    Ok((
        frontmatter,
        lines.collect::<Vec<_>>().join("\n").trim().to_string(),
    ))
}

fn markdown_files_in_dir(dir: &Path) -> Result<Vec<PathBuf>, ConfigError> {
    let mut files = Vec::new();
    collect_markdown_files(dir, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_markdown_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), ConfigError> {
    let mut entries = fs::read_dir(dir)
        .map_err(|source| ConfigError::ReadMarkdownAsset {
            path: dir.display().to_string(),
            source,
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|source| ConfigError::ReadMarkdownAsset {
            path: dir.display().to_string(),
            source,
        })?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, files)?;
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            files.push(path);
        }
    }

    Ok(())
}

fn agent_prompt_search_dirs(config_path: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    for base in discovery_search_bases(config_path) {
        push_unique_path(&mut dirs, base.join(".agent-harness").join("agents"));
    }

    if let Some(config_dir) = config_path.parent() {
        push_unique_path(&mut dirs, config_dir.join(".agent-harness").join("agents"));
    }

    dirs
}

fn instruction_search_paths(config_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    for base in discovery_search_bases(config_path) {
        push_unique_path(&mut paths, base.join("AGENTS.md"));
    }

    if let Some(config_dir) = config_path.parent() {
        push_unique_path(&mut paths, config_dir.join("AGENTS.md"));
    }

    paths
}

fn discovery_search_bases(config_path: &Path) -> Vec<PathBuf> {
    let mut bases = Vec::new();

    if let Ok(cwd) = env::current_dir() {
        for base in project_search_bases(&cwd) {
            push_unique_path(&mut bases, base);
        }
    }

    if let Some(config_dir) = config_path.parent() {
        for base in project_search_bases(config_dir) {
            push_unique_path(&mut bases, base);
        }
    }

    if bases.is_empty() {
        bases.push(PathBuf::from("."));
    }

    bases
}

fn project_search_bases(start: &Path) -> Vec<PathBuf> {
    let ancestors = start.ancestors().map(Path::to_path_buf).collect::<Vec<_>>();
    if let Some(index) = ancestors.iter().position(|path| path.join(".git").exists()) {
        return ancestors.into_iter().take(index + 1).collect();
    }
    vec![start.to_path_buf()]
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|existing| existing == &candidate) {
        paths.push(candidate);
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
    pub disabled: bool,
    #[serde(default, alias = "maxInputTokens")]
    pub max_input_tokens: Option<u32>,
    #[serde(default, alias = "maxOutputTokens")]
    pub max_output_tokens: Option<u32>,
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
    /// Per-profile multi-turn budget enforced directly by the runtime.
    /// There is no separate hardcoded runtime iteration cap beyond this setting.
    #[serde(default = "default_max_iters", alias = "maxIters")]
    pub max_iters: usize,
    #[serde(
        default = "default_runtime_tool_failure_mode",
        alias = "toolFailureMode"
    )]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpServerConnectionState {
    Connected,
    Disconnected,
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

fn default_hook_timeout_ms() -> u64 {
    5_000
}

fn default_skills_walk_to_git_root() -> bool {
    true
}

fn default_skills_project_roots() -> Vec<PathBuf> {
    vec![
        PathBuf::from(".agent-harness/skills"),
        PathBuf::from(".codex/skills"),
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

fn default_mcp_enabled() -> bool {
    true
}

fn default_provider_timeout_ms() -> u64 {
    60_000
}

fn resolve_string_reference(value: &str, base_dir: Option<&Path>) -> Result<String, ConfigError> {
    if let Some(reference) = value
        .strip_prefix("{env:")
        .and_then(|reference| reference.strip_suffix('}'))
    {
        return Ok(env::var(reference).unwrap_or_default());
    }

    if let Some(reference) = value
        .strip_prefix("{file:")
        .and_then(|reference| reference.strip_suffix('}'))
    {
        let trimmed = reference.trim();
        if trimmed.is_empty() {
            return Ok(value.to_string());
        }

        let path = PathBuf::from(trimmed);
        let resolved_path = if path.is_absolute() {
            path
        } else if let Some(base_dir) = base_dir {
            base_dir.join(path)
        } else {
            path
        };

        return fs::read_to_string(&resolved_path).map_err(|source| ConfigError::ReadFile {
            path: resolved_path.display().to_string(),
            source,
        });
    }

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

fn resolve_config_value_references(
    value: &mut serde_json::Value,
    base_dir: Option<&Path>,
) -> Result<(), ConfigError> {
    match value {
        serde_json::Value::String(string) => {
            *string = resolve_string_reference(string, base_dir)?;
        }
        serde_json::Value::Array(values) => {
            for value in values {
                resolve_config_value_references(value, base_dir)?;
            }
        }
        serde_json::Value::Object(object) => {
            for value in object.values_mut() {
                resolve_config_value_references(value, base_dir)?;
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }

    Ok(())
}

const REQUIRED_INTERNAL_CONFIG_SECTIONS: [&str; 4] =
    ["integrations", "permissions", "providers", "runtime"];

const ALLOWED_INTERNAL_TOP_LEVEL_CONFIG_KEYS: [&str; 15] = [
    "$schema",
    "agents",
    "defaultAgent",
    "default_agent",
    "hooks",
    "hashlineEdit",
    "hashline_edit",
    "integrations",
    "lsp",
    "logging",
    "permissions",
    "providers",
    "runtime",
    "skills",
    "ui",
];

const ALLOWED_PUBLIC_TOP_LEVEL_CONFIG_KEYS: [&str; 28] = [
    "$schema",
    "agent",
    "agents",
    "backgroundTask",
    "categories",
    "defaultAgent",
    "default_agent",
    "deterministic",
    "hashlineEdit",
    "hashline_edit",
    "hooks",
    "instructions",
    "integrations",
    "logging",
    "lsp",
    "mcp",
    "model",
    "paths",
    "permission",
    "permissions",
    "profiles",
    "provider",
    "providers",
    "runtime",
    "skills",
    "smallModel",
    "small_model",
    "ui",
];

fn validate_public_root_config_object(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), ConfigError> {
    let mut unknown = object
        .keys()
        .filter(|key| !ALLOWED_PUBLIC_TOP_LEVEL_CONFIG_KEYS.contains(&key.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        unknown.sort_unstable();
        return Err(ConfigError::UnknownTopLevelKeys(format!(
            "unknown top-level config keys: {}; expected only {}",
            format_backticked_list(unknown.iter().copied()),
            format_backticked_list(ALLOWED_PUBLIC_TOP_LEVEL_CONFIG_KEYS)
        )));
    }

    Ok(())
}

fn validate_internal_root_config_object(
    object: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), ConfigError> {
    validate_internal_root_config_object_internal(object, true)
}

fn validate_internal_root_config_object_internal(
    object: &serde_json::Map<String, serde_json::Value>,
    require_required_sections: bool,
) -> Result<(), ConfigError> {
    if require_required_sections {
        let mut missing = REQUIRED_INTERNAL_CONFIG_SECTIONS
            .iter()
            .copied()
            .filter(|key| !object.contains_key(*key))
            .collect::<Vec<_>>();
        if !object.contains_key("agents") {
            missing.push("agents");
        }
        if !missing.is_empty() {
            missing.sort_unstable();
            return Err(ConfigError::MissingRequiredSections(missing.join(", ")));
        }
    }

    let mut unknown = object
        .keys()
        .filter(|key| !ALLOWED_INTERNAL_TOP_LEVEL_CONFIG_KEYS.contains(&key.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        unknown.sort_unstable();
        return Err(ConfigError::UnknownTopLevelKeys(format!(
            "unknown top-level config keys: {}; expected only {}",
            format_backticked_list(unknown.iter().copied()),
            format_backticked_list(ALLOWED_INTERNAL_TOP_LEVEL_CONFIG_KEYS)
        )));
    }

    Ok(())
}

fn default_internal_permissions_config() -> PermissionsConfig {
    PermissionsConfig {
        defaults: PermissionDefaultsConfig {
            edit: PermissionMode::Ask,
            shell: PermissionMode::Ask,
            network: PermissionMode::Ask,
            question: Some(PermissionMode::Ask),
            task: Some(PermissionMode::Ask),
            webfetch: Some(PermissionMode::Ask),
            websearch: Some(PermissionMode::Ask),
            codesearch: Some(PermissionMode::Ask),
            lsp: Some(PermissionMode::Ask),
        },
        shell_allowlist: ShellAllowlist::default(),
    }
}

fn default_internal_integrations_config() -> IntegrationsConfig {
    IntegrationsConfig {
        remote_search: RemoteSearchConfig::default(),
        mcp: McpConfig::default(),
    }
}

fn default_shipped_agents(
    model_ref: &str,
    small_model_ref: Option<&str>,
) -> BTreeMap<String, ProfileConfig> {
    let small_model_ref = small_model_ref.unwrap_or(model_ref).to_string();
    BTreeMap::from([
        (
            "build".to_string(),
            ProfileConfig {
                description: "Implementation lane: execute an approved plan and verify the result."
                    .to_string(),
                system_prompt: None,
                model_ref: model_ref.to_string(),
                variant: None,
                temperature: None,
                permissions: Some(ProfilePermissions {
                    edit: Some(PermissionMode::Allow),
                    shell: Some(PermissionMode::Allow),
                    network: Some(PermissionMode::Allow),
                    question: Some(PermissionMode::Allow),
                    task: Some(PermissionMode::Allow),
                    webfetch: Some(PermissionMode::Allow),
                    websearch: Some(PermissionMode::Allow),
                    codesearch: Some(PermissionMode::Allow),
                    lsp: Some(PermissionMode::Allow),
                }),
                max_iters: 24,
                tool_failure_mode: ToolFailureMode::ContinueAsToolMessage,
                plan_mode: false,
                exit_target_profile: None,
                tools: vec![
                    "todowrite",
                    "todoread",
                    "question",
                    "skill",
                    "websearch",
                    "webfetch",
                    "codesearch",
                    "lsp",
                    "read",
                    "glob",
                    "grep",
                    "list",
                    "write",
                    "edit",
                    "bash",
                    "batch",
                    "task",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
            },
        ),
        (
            "plan".to_string(),
            ProfileConfig {
                description: "Planning lane: produce an approved plan and hand off to build."
                    .to_string(),
                system_prompt: None,
                model_ref: small_model_ref.clone(),
                variant: None,
                temperature: None,
                permissions: Some(ProfilePermissions {
                    edit: Some(PermissionMode::Deny),
                    shell: Some(PermissionMode::Deny),
                    network: Some(PermissionMode::Allow),
                    question: Some(PermissionMode::Allow),
                    task: Some(PermissionMode::Deny),
                    webfetch: Some(PermissionMode::Allow),
                    websearch: Some(PermissionMode::Allow),
                    codesearch: Some(PermissionMode::Allow),
                    lsp: Some(PermissionMode::Allow),
                }),
                max_iters: 16,
                tool_failure_mode: ToolFailureMode::ContinueAsToolMessage,
                plan_mode: true,
                exit_target_profile: Some("build".to_string()),
                tools: vec![
                    "todowrite",
                    "todoread",
                    "question",
                    "skill",
                    "websearch",
                    "webfetch",
                    "codesearch",
                    "lsp",
                    "read",
                    "glob",
                    "grep",
                    "list",
                    "batch",
                    "plan_exit",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
            },
        ),
        (
            "tool_audit".to_string(),
            ProfileConfig {
                description:
                    "Evidence-first signoff profile for validating the shipped tool surface."
                        .to_string(),
                system_prompt: None,
                model_ref: small_model_ref,
                variant: None,
                temperature: None,
                permissions: None,
                max_iters: 20,
                tool_failure_mode: ToolFailureMode::ContinueAsToolMessage,
                plan_mode: false,
                exit_target_profile: None,
                tools: vec![
                    "skill",
                    "question",
                    "lsp",
                    "task",
                    "batch",
                    "invalid",
                    "todowrite",
                    "todoread",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
            },
        ),
    ])
}

fn public_agent_to_profile(
    name: &str,
    agent: PublicAgentConfig,
    default_model_ref: Option<&str>,
    small_model_ref: Option<&str>,
    base: Option<ProfileConfig>,
) -> Result<ProfileConfig, ConfigError> {
    let selected_model = agent.model.clone().or_else(|| {
        if agent.use_small_model {
            small_model_ref.map(str::to_string)
        } else {
            default_model_ref.map(str::to_string)
        }
    });
    let description = agent
        .description
        .or_else(|| base.as_ref().map(|profile| profile.description.clone()))
        .ok_or_else(|| {
            ConfigError::InvalidReference(format!(
                "agent `{name}` is missing `description`; provide `agent.{name}.description`"
            ))
        })?;
    let model_ref = selected_model
        .or_else(|| base.as_ref().map(|profile| profile.model_ref.clone()))
        .ok_or_else(|| {
            ConfigError::InvalidReference(format!(
                "agent `{name}` is missing `model`; provide `agent.{name}.model`, set `small_model`, or add a top-level `model`"
            ))
        })?;

    Ok(ProfileConfig {
        description,
        system_prompt: agent.system_prompt.or_else(|| {
            base.as_ref()
                .and_then(|profile| profile.system_prompt.clone())
        }),
        model_ref,
        variant: agent
            .variant
            .or_else(|| base.as_ref().and_then(|profile| profile.variant.clone())),
        temperature: agent
            .temperature
            .or_else(|| base.as_ref().and_then(|profile| profile.temperature)),
        permissions: agent
            .permission
            .map(translate_public_profile_permissions)
            .transpose()?
            .or_else(|| {
                base.as_ref()
                    .and_then(|profile| profile.permissions.clone())
            }),
        max_iters: if agent.max_iters == default_max_iters() {
            base.as_ref()
                .map(|profile| profile.max_iters)
                .unwrap_or(agent.max_iters)
        } else {
            agent.max_iters
        },
        tool_failure_mode: if matches!(agent.tool_failure_mode, ToolFailureMode::FailTurn) {
            base.as_ref()
                .map(|profile| profile.tool_failure_mode)
                .unwrap_or(agent.tool_failure_mode)
        } else {
            agent.tool_failure_mode
        },
        plan_mode: agent.plan_mode
            || base
                .as_ref()
                .map(|profile| profile.plan_mode)
                .unwrap_or(false),
        exit_target_profile: agent.exit_target_profile.or_else(|| {
            base.as_ref()
                .and_then(|profile| profile.exit_target_profile.clone())
        }),
        tools: if agent.tools.is_empty() {
            base.as_ref()
                .map(|profile| profile.tools.clone())
                .unwrap_or_default()
        } else {
            agent.tools
        },
    })
}

fn translate_public_profile_permissions(
    permissions: PublicProfilePermissions,
) -> Result<ProfilePermissions, ConfigError> {
    serde_json::from_value(serde_json::json!({
        "edit": permissions.edit,
        "shell": permissions.bash,
        "network": permissions.network,
        "question": permissions.question,
        "task": permissions.task,
        "webfetch": permissions.webfetch,
        "websearch": permissions.websearch,
        "codesearch": permissions.codesearch,
        "lsp": permissions.lsp,
    }))
    .map_err(|err| ConfigError::ParseJson5(err.to_string()))
}

fn translate_public_permission_value(
    value: serde_json::Value,
) -> Result<serde_json::Value, ConfigError> {
    if value
        .as_object()
        .map(|object| object.contains_key("defaults"))
        .unwrap_or(false)
    {
        return Ok(value);
    }

    let parsed: PublicPermissionConfig =
        serde_json::from_value(value).map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
    let fallback = default_internal_permissions_config();

    serde_json::to_value(PermissionsConfig {
        defaults: PermissionDefaultsConfig {
            edit: parsed.edit.unwrap_or(fallback.defaults.edit),
            shell: parsed.bash.unwrap_or(fallback.defaults.shell),
            network: parsed.network.unwrap_or(fallback.defaults.network),
            question: parsed.question.or(fallback.defaults.question),
            task: parsed.task.or(fallback.defaults.task),
            webfetch: parsed.webfetch.or(fallback.defaults.webfetch),
            websearch: parsed.websearch.or(fallback.defaults.websearch),
            codesearch: parsed.codesearch.or(fallback.defaults.codesearch),
            lsp: parsed.lsp.or(fallback.defaults.lsp),
        },
        shell_allowlist: parsed.shell_allowlist.unwrap_or(fallback.shell_allowlist),
    })
    .map_err(|err| ConfigError::ParseJson5(err.to_string()))
}

fn translate_public_runtime_root(
    root: serde_json::Value,
) -> Result<(serde_json::Value, Vec<String>), ConfigError> {
    let object = root.as_object().ok_or(ConfigError::InvalidRootObject)?;
    validate_public_root_config_object(object)?;

    let mut translated = serde_json::Map::new();

    if let Some(schema) = object.get("$schema").cloned() {
        translated.insert("$schema".to_string(), schema);
    }

    let mut providers = serde_json::json!({});
    if let Some(value) = object.get("providers") {
        merge_config_value(&mut providers, value.clone());
    }
    if let Some(value) = object.get("provider") {
        merge_config_value(&mut providers, value.clone());
    }
    translated.insert("providers".to_string(), providers);

    let model = object
        .get("model")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let small_model = object
        .get("small_model")
        .or_else(|| object.get("smallModel"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);

    let mut agents = BTreeMap::new();
    if let Some(value) = object.get("agents") {
        let legacy: BTreeMap<String, ProfileConfig> = serde_json::from_value(value.clone())
            .map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
        agents.extend(legacy);
    }
    for alias in ["categories", "profiles"] {
        if let Some(value) = object.get(alias) {
            let legacy: BTreeMap<String, ProfileConfig> = serde_json::from_value(value.clone())
                .map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
            agents.extend(legacy);
        }
    }

    let shipped = model
        .as_deref()
        .map(|default_model| default_shipped_agents(default_model, small_model.as_deref()))
        .unwrap_or_default();

    if let Some(value) = object.get("agent") {
        let public_agents: BTreeMap<String, PublicAgentConfig> =
            serde_json::from_value(value.clone())
                .map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
        for (name, public_agent) in public_agents {
            let base = agents.remove(&name).or_else(|| shipped.get(&name).cloned());
            let profile = public_agent_to_profile(
                &name,
                public_agent,
                model.as_deref(),
                small_model.as_deref(),
                base,
            )?;
            agents.insert(name, profile);
        }
    }
    if agents.is_empty() && !shipped.is_empty() {
        agents = shipped;
    }

    translated.insert(
        "agents".to_string(),
        serde_json::to_value(agents).map_err(|err| ConfigError::ParseJson5(err.to_string()))?,
    );

    if let Some(default_agent) = object
        .get("default_agent")
        .or_else(|| object.get("defaultAgent"))
        .cloned()
    {
        translated.insert("default_agent".to_string(), default_agent);
    }

    let mut permissions = serde_json::to_value(default_internal_permissions_config())
        .map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
    if let Some(value) = object.get("permissions") {
        merge_config_value(&mut permissions, value.clone());
    }
    if let Some(value) = object.get("permission") {
        merge_config_value(
            &mut permissions,
            translate_public_permission_value(value.clone())?,
        );
    }
    translated.insert("permissions".to_string(), permissions);

    let mut runtime = serde_json::json!({
        "background_tasks": {
            "default_concurrency": default_background_task_default_concurrency(),
            "provider_concurrency": default_background_task_provider_concurrency(),
            "model_concurrency": default_background_task_model_concurrency(),
            "stale_timeout_ms": default_background_task_stale_timeout_ms(),
            "message_staleness_timeout_ms": default_background_task_message_staleness_timeout_ms(),
        },
        "session_dir": default_session_dir(),
        "permissions": {
            "ask_timeout_ms": default_runtime_ask_timeout_ms(),
        },
        "prompt": {
            "wait_timeout_ms": default_prompt_wait_timeout_ms(),
        },
        "deterministic": {
            "enabled": false,
            "seed": 42,
        },
    });
    if let Some(value) = object.get("runtime") {
        merge_config_value(&mut runtime, value.clone());
    }
    if let Some(value) = object.get("backgroundTask") {
        if let Some(runtime_object) = runtime.as_object_mut() {
            runtime_object.insert("background_tasks".to_string(), value.clone());
        }
    }
    if let Some(value) = object.get("deterministic") {
        if let Some(runtime_object) = runtime.as_object_mut() {
            runtime_object.insert("deterministic".to_string(), value.clone());
        }
    }
    if let Some(value) = object.get("paths") {
        if let Some(session_dir) = value
            .as_object()
            .and_then(|paths| paths.get("session_dir").or_else(|| paths.get("sessionDir")))
        {
            if let Some(runtime_object) = runtime.as_object_mut() {
                runtime_object.insert("session_dir".to_string(), session_dir.clone());
            }
        }
    }
    translated.insert("runtime".to_string(), runtime);

    let mut integrations = serde_json::to_value(default_internal_integrations_config())
        .map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
    if let Some(value) = object.get("integrations") {
        merge_config_value(&mut integrations, value.clone());
    }
    if let Some(value) = object.get("mcp") {
        let mcp_value = serde_json::json!({ "servers": value.clone() });
        if let Some(integrations_object) = integrations.as_object_mut() {
            match integrations_object.get_mut("mcp") {
                Some(existing) => merge_config_value(existing, mcp_value),
                None => {
                    integrations_object.insert("mcp".to_string(), mcp_value);
                }
            }
        }
    }
    translated.insert("integrations".to_string(), integrations);

    for (key, value) in [
        ("hooks", object.get("hooks")),
        ("skills", object.get("skills")),
        ("lsp", object.get("lsp")),
        ("logging", object.get("logging")),
        ("ui", object.get("ui")),
        (
            "hashline_edit",
            object
                .get("hashline_edit")
                .or_else(|| object.get("hashlineEdit")),
        ),
    ] {
        if let Some(value) = value {
            translated.insert(key.to_string(), value.clone());
        }
    }

    let instructions = object
        .get("instructions")
        .map(|value| {
            serde_json::from_value::<InstructionList>(value.clone())
                .map(|parsed| parsed.entries())
                .map_err(|err| ConfigError::ParseJson5(err.to_string()))
        })
        .transpose()?
        .unwrap_or_default();

    Ok((serde_json::Value::Object(translated), instructions))
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

fn with_integrations_config_registry<T>(f: impl FnOnce(&mut Option<IntegrationsConfig>) -> T) -> T {
    match integrations_config_registry().lock() {
        Ok(mut guard) => f(&mut guard),
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            f(&mut guard)
        }
    }
}

fn with_mcp_server_connection_registry<T>(
    f: impl FnOnce(&mut BTreeMap<String, McpServerConnectionState>) -> T,
) -> T {
    match mcp_server_connection_registry().lock() {
        Ok(mut guard) => f(&mut guard),
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            f(&mut guard)
        }
    }
}

fn with_mcp_server_first_class_tool_id_registry<T>(
    f: impl FnOnce(&mut BTreeMap<String, BTreeMap<String, String>>) -> T,
) -> T {
    match mcp_server_first_class_tool_id_registry().lock() {
        Ok(mut guard) => f(&mut guard),
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            f(&mut guard)
        }
    }
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
    with_mcp_server_connection_registry(|registered| registered.get(server_name).copied())
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

    let Some((provider_name, model_name)) = parse_model_ref(&profile.model_ref) else {
        return Err(ConfigError::InvalidReference(format!(
            "agent `{profile_name}` has invalid `model_ref` `{}`; use `<provider>:<model>`",
            profile.model_ref
        )));
    };

    let provider = cfg.providers.get(provider_name).ok_or_else(|| {
        ConfigError::InvalidReference(format!(
            "agent `{profile_name}` references unknown provider `{provider_name}` in `model_ref` `{}`; available providers: {}",
            profile.model_ref,
            format_name_list(cfg.providers.keys().map(|name| name.as_str()))
        ))
    })?;

    let models = provider.models();
    let model = models.get(model_name).ok_or_else(|| {
        ConfigError::InvalidReference(format!(
            "agent `{profile_name}` references unknown model `{model_name}` in `model_ref` `{}`; available models for provider `{provider_name}`: {}",
            profile.model_ref,
            format_name_list(models.keys().map(|name| name.as_str()))
        ))
    })?;

    let variant = profile.variant.as_deref().map(|variant_name| {
        let variant = model.variants.get(variant_name).ok_or_else(|| {
            ConfigError::InvalidReference(format!(
                "agent `{profile_name}` references unknown variant `{variant_name}` for model `{}`; available variants: {}",
                profile.model_ref,
                format_name_list(model.variants.keys().map(|name| name.as_str()))
            ))
        })?;

        if variant.disabled {
            return Err(ConfigError::InvalidReference(format!(
                "agent `{profile_name}` references disabled variant `{variant_name}` for model `{}`; choose an enabled variant",
                profile.model_ref
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
    let max_input_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.max_input_tokens)
        .or(model.max_input_tokens);
    let max_output_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.max_output_tokens)
        .or(model.max_output_tokens);

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

fn build_resolved_model_catalog_entry(
    provider_name: &str,
    model_name: &str,
    model: &ModelConfig,
    provider: &ProviderConfig,
    variant: Option<(&str, &ModelVariantConfig)>,
) -> ResolvedModelCatalogEntry {
    let max_input_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.max_input_tokens)
        .or(model.max_input_tokens);
    let max_output_tokens = variant
        .and_then(|(_, variant_cfg)| variant_cfg.max_output_tokens)
        .or(model.max_output_tokens);

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
    if alias.is_empty() {
        return Ok(());
    }
    if target.is_empty() {
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
        let trimmed = self.trim();
        (!trimmed.is_empty()).then_some(trimmed)
    }

    fn set_value(&mut self, value: String) {
        *self = value;
    }
}

impl StringAliasTarget for Option<String> {
    fn current_value(&self) -> Option<&str> {
        self.as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
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

pub fn load_config_from_file(path: &Path) -> Result<HarnessConfig, ConfigError> {
    let raw = fs::read_to_string(path).map_err(|source| ConfigError::ReadFile {
        path: path.display().to_string(),
        source,
    })?;

    let (parsed, configured_instructions) =
        parse_config_from_str(&raw, path.parent().or(Some(Path::new("."))))?;
    finalize_loaded_config(parsed, Some(path), configured_instructions)
}

pub fn load_config_from_str(raw: &str) -> Result<HarnessConfig, ConfigError> {
    let (parsed, configured_instructions) = parse_config_from_str(raw, None)?;
    finalize_loaded_config(parsed, None, configured_instructions)
}

fn parse_config_from_str(
    raw: &str,
    base_dir: Option<&Path>,
) -> Result<(HarnessConfig, Vec<String>), ConfigError> {
    let root = parse_public_config_value_from_str(raw, base_dir)?;
    let (translated, configured_instructions) = translate_public_runtime_root(root)?;
    Ok((
        parse_internal_config_from_value(translated)?,
        configured_instructions,
    ))
}

pub fn load_resolved_config(
    explicit_path: Option<&Path>,
) -> Result<Option<LoadedConfig>, ConfigError> {
    let runtime_paths = resolve_config_layer_paths(explicit_path);
    let runtime_content = env::var("HARNESS_CONFIG_CONTENT").ok();
    if runtime_paths.is_empty() && runtime_content.is_none() {
        return Ok(None);
    }

    let tui_paths = resolve_tui_config_layer_paths(explicit_path);
    let config =
        load_resolved_config_from_paths(&runtime_paths, runtime_content.as_deref(), &tui_paths)?;
    let mut paths = runtime_paths.clone();
    for path in tui_paths {
        if !paths.iter().any(|existing| existing == &path) {
            paths.push(path);
        }
    }

    Ok(Some(LoadedConfig { config, paths }))
}

pub fn resolve_config_layer_paths(explicit_path: Option<&Path>) -> Vec<PathBuf> {
    if let Some(path) = explicit_path {
        return vec![path.to_path_buf()];
    }

    let mut paths = Vec::new();

    if let Some(global_path) = discover_xdg_runtime_config_path() {
        push_unique_path(&mut paths, global_path);
    }

    if let Some(env_path) = discover_runtime_config_env_path() {
        push_unique_path(&mut paths, env_path);
    }

    for local_path in discover_project_runtime_config_paths(
        env::current_dir()
            .ok()
            .as_deref()
            .unwrap_or_else(|| Path::new(".")),
    ) {
        push_unique_path(&mut paths, local_path);
    }

    paths
}

fn parse_internal_config_from_value(root: serde_json::Value) -> Result<HarnessConfig, ConfigError> {
    let object = root.as_object().ok_or(ConfigError::InvalidRootObject)?;
    validate_internal_root_config_object(object)?;

    let mut parsed: HarnessConfig =
        serde_json::from_value(root).map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
    parsed.normalize_public_config_aliases()?;
    parsed.sync_derived_runtime_sections();
    Ok(parsed)
}

fn finalize_loaded_config(
    mut parsed: HarnessConfig,
    config_path: Option<&Path>,
    configured_instructions: Vec<String>,
) -> Result<HarnessConfig, ConfigError> {
    if let Some(path) = config_path {
        resolve_discovered_prompt_assets(&mut parsed, path)?;
    }
    if !configured_instructions.is_empty() {
        let mut resolved =
            resolve_configured_instruction_entries(&configured_instructions, config_path)?;
        resolved.extend(parsed.instruction_files.clone());
        parsed.instruction_files = resolved;
    }
    parsed.validate_references()?;
    refresh_hook_runtime_config_registry(&parsed);
    refresh_skills_config_registry(&parsed);
    refresh_lsp_config_registry(&parsed);
    refresh_integrations_config_registry(&parsed);
    refresh_profile_model_metadata_registry(&parsed)?;
    Ok(parsed)
}

fn load_resolved_config_from_paths(
    runtime_paths: &[PathBuf],
    runtime_content: Option<&str>,
    tui_paths: &[PathBuf],
) -> Result<HarnessConfig, ConfigError> {
    let mut merged: Option<serde_json::Value> = None;
    let mut configured_instructions = Vec::new();

    for path in runtime_paths {
        let raw = fs::read_to_string(path).map_err(|source| ConfigError::ReadFile {
            path: path.display().to_string(),
            source,
        })?;
        let root = parse_public_config_value_from_str(&raw, path.parent())?;
        let (fragment, instructions) = translate_public_runtime_root(root)?;
        configured_instructions.extend(instructions);
        match &mut merged {
            Some(existing) => merge_config_value(existing, fragment),
            None => merged = Some(fragment),
        }
    }

    if let Some(runtime_content) = runtime_content {
        let root = parse_public_config_value_from_str(runtime_content, None)?;
        let (fragment, instructions) = translate_public_runtime_root(root)?;
        configured_instructions.extend(instructions);
        match &mut merged {
            Some(existing) => merge_config_value(existing, fragment),
            None => merged = Some(fragment),
        }
    }

    let merged = merged.ok_or_else(|| ConfigError::ReadFile {
        path: "<merged-config>".to_string(),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "no config files resolved"),
    })?;
    let primary_path = runtime_paths.last().map(PathBuf::as_path);
    let mut parsed = parse_internal_config_from_value(merged)?;
    if !tui_paths.is_empty() {
        let tui = load_merged_tui_config_from_files(tui_paths)?;
        apply_public_tui_config(&mut parsed, tui);
    }
    finalize_loaded_config(parsed, primary_path, configured_instructions)
}

fn parse_public_config_value_from_str(
    raw: &str,
    base_dir: Option<&Path>,
) -> Result<serde_json::Value, ConfigError> {
    let mut root: serde_json::Value =
        json5::from_str(raw).map_err(|err| ConfigError::ParseJson5(err.to_string()))?;

    let object = root.as_object().ok_or(ConfigError::InvalidRootObject)?;
    validate_public_root_config_object(object)?;
    resolve_config_value_references(&mut root, base_dir)?;
    Ok(root)
}

fn load_merged_tui_config_from_files(paths: &[PathBuf]) -> Result<PublicTuiConfig, ConfigError> {
    let mut merged: Option<serde_json::Value> = None;

    for path in paths {
        let raw = fs::read_to_string(path).map_err(|source| ConfigError::ReadFile {
            path: path.display().to_string(),
            source,
        })?;
        let fragment: serde_json::Value =
            json5::from_str(&raw).map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
        match &mut merged {
            Some(existing) => merge_config_value(existing, fragment),
            None => merged = Some(fragment),
        }
    }

    serde_json::from_value(merged.unwrap_or_else(|| serde_json::json!({})))
        .map_err(|err| ConfigError::ParseJson5(err.to_string()))
}

fn apply_public_tui_config(parsed: &mut HarnessConfig, tui: PublicTuiConfig) {
    parsed.ui.keybindings = tui.keybindings;
}

fn resolve_configured_instruction_entries(
    entries: &[String],
    config_path: Option<&Path>,
) -> Result<Vec<InstructionFile>, ConfigError> {
    let mut resolved = Vec::new();
    let base_dir = config_path.and_then(Path::parent);

    for (index, entry) in entries.iter().enumerate() {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }

        let candidate = base_dir
            .map(|base| base.join(trimmed))
            .filter(|path| path.exists())
            .or_else(|| {
                let path = PathBuf::from(trimmed);
                path.exists().then_some(path)
            });

        if let Some(path) = candidate {
            let content =
                fs::read_to_string(&path).map_err(|source| ConfigError::ReadMarkdownAsset {
                    path: path.display().to_string(),
                    source,
                })?;
            let content = content.trim().to_string();
            if !content.is_empty() {
                resolved.push(InstructionFile { path, content });
            }
            continue;
        }

        resolved.push(InstructionFile {
            path: PathBuf::from(format!("<config instructions {}>", index + 1)),
            content: trimmed.to_string(),
        });
    }

    Ok(resolved)
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

pub fn harness_schema_pretty_json() -> Result<String, ConfigError> {
    let schema = schema_for!(PublicRuntimeConfig);
    serde_json::to_string_pretty(&schema)
        .map_err(|err| ConfigError::SerializeSchema(err.to_string()))
}

pub fn harness_tui_schema_pretty_json() -> Result<String, ConfigError> {
    let schema = schema_for!(PublicTuiConfig);
    serde_json::to_string_pretty(&schema)
        .map_err(|err| ConfigError::SerializeSchema(err.to_string()))
}

pub fn resolve_config_path(explicit_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit_path {
        return Some(path.to_path_buf());
    }

    resolve_config_layer_paths(None).into_iter().last()
}

fn resolve_tui_config_layer_paths(explicit_path: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(global_path) = discover_xdg_tui_config_path() {
        push_unique_path(&mut paths, global_path);
    }

    if let Some(env_path) = discover_tui_config_env_path() {
        push_unique_path(&mut paths, env_path);
    }

    let local_base = env::current_dir().unwrap_or_else(|_| {
        explicit_path
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    });
    for local_path in discover_project_tui_config_paths(&local_base) {
        push_unique_path(&mut paths, local_path);
    }

    paths
}

fn discover_xdg_runtime_config_path() -> Option<PathBuf> {
    config_home_dir().and_then(|base| {
        [
            base.join("harness").join("harness.jsonc"),
            base.join("harness").join("harness.json"),
            base.join("harness").join("config.jsonc"),
        ]
        .into_iter()
        .find(|path| path.exists())
    })
}

fn discover_xdg_tui_config_path() -> Option<PathBuf> {
    config_home_dir().and_then(|base| {
        [
            base.join("harness").join("tui.jsonc"),
            base.join("harness").join("tui.json"),
        ]
        .into_iter()
        .find(|path| path.exists())
    })
}

fn config_home_dir() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
}

fn discover_runtime_config_env_path() -> Option<PathBuf> {
    env::var_os("HARNESS_CONFIG")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn discover_tui_config_env_path() -> Option<PathBuf> {
    env::var_os("HARNESS_TUI_CONFIG")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn discover_project_runtime_config_paths(start: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    for base in project_config_search_bases(start) {
        for relative in [
            Path::new("harness.jsonc"),
            Path::new("harness.json"),
            Path::new(".agent-harness/harness.jsonc"),
            Path::new(".agent-harness/harness.json"),
        ] {
            let candidate = base.join(relative);
            if candidate.exists() {
                paths.push(candidate);
            }
        }
    }

    paths
}

fn discover_project_tui_config_paths(start: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    for base in project_config_search_bases(start) {
        for relative in [
            Path::new("tui.jsonc"),
            Path::new("tui.json"),
            Path::new(".agent-harness/tui.jsonc"),
            Path::new(".agent-harness/tui.json"),
        ] {
            let candidate = base.join(relative);
            if candidate.exists() {
                paths.push(candidate);
            }
        }
    }

    paths
}

fn project_config_search_bases(start: &Path) -> Vec<PathBuf> {
    let mut bases = project_search_bases(start);
    bases.reverse();
    bases
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::OsString, sync::Mutex};

    static CONFIG_DISCOVERY_TEST_LOCK: Mutex<()> = Mutex::new(());
    static CONFIG_ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[allow(unsafe_code)]
    fn with_env_var_state<T>(name: &str, value: Option<&str>, run: impl FnOnce() -> T) -> T {
        let _lock = CONFIG_ENV_TEST_LOCK
            .lock()
            .expect("config env test lock should not be poisoned");
        let previous = env::var_os(name);

        match value {
            Some(value) => unsafe { env::set_var(name, value) },
            None => unsafe { env::remove_var(name) },
        }

        let result = run();

        match previous {
            Some(value) => unsafe { env::set_var(name, value) },
            None => unsafe { env::remove_var(name) },
        }

        result
    }

    struct DiscoveryTestContext {
        previous_cwd: PathBuf,
        previous_xdg_config_home: Option<OsString>,
        previous_home: Option<OsString>,
        previous_harness_config: Option<OsString>,
        previous_harness_config_content: Option<OsString>,
        previous_harness_tui_config: Option<OsString>,
    }

    impl DiscoveryTestContext {
        fn new(cwd: &Path, xdg_config_home: Option<&Path>) -> Self {
            let previous_cwd = env::current_dir().expect("capture current dir");
            let previous_xdg_config_home = env::var_os("XDG_CONFIG_HOME");
            let previous_home = env::var_os("HOME");
            let previous_harness_config = env::var_os("HARNESS_CONFIG");
            let previous_harness_config_content = env::var_os("HARNESS_CONFIG_CONTENT");
            let previous_harness_tui_config = env::var_os("HARNESS_TUI_CONFIG");

            env::set_current_dir(cwd).expect("set test current dir");
            match xdg_config_home {
                Some(path) => env::set_var("XDG_CONFIG_HOME", path),
                None => env::remove_var("XDG_CONFIG_HOME"),
            }
            env::set_var("HOME", cwd);
            env::remove_var("HARNESS_CONFIG");
            env::remove_var("HARNESS_CONFIG_CONTENT");
            env::remove_var("HARNESS_TUI_CONFIG");

            Self {
                previous_cwd,
                previous_xdg_config_home,
                previous_home,
                previous_harness_config,
                previous_harness_config_content,
                previous_harness_tui_config,
            }
        }
    }

    impl Drop for DiscoveryTestContext {
        fn drop(&mut self) {
            let _ = env::set_current_dir(&self.previous_cwd);
            match &self.previous_xdg_config_home {
                Some(value) => env::set_var("XDG_CONFIG_HOME", value),
                None => env::remove_var("XDG_CONFIG_HOME"),
            }
            match &self.previous_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }
            match &self.previous_harness_config {
                Some(value) => env::set_var("HARNESS_CONFIG", value),
                None => env::remove_var("HARNESS_CONFIG"),
            }
            match &self.previous_harness_config_content {
                Some(value) => env::set_var("HARNESS_CONFIG_CONTENT", value),
                None => env::remove_var("HARNESS_CONFIG_CONTENT"),
            }
            match &self.previous_harness_tui_config {
                Some(value) => env::set_var("HARNESS_TUI_CONFIG", value),
                None => env::remove_var("HARNESS_TUI_CONFIG"),
            }
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
    fn example_config_parses() {
        let agents = r#"
            deep: {
              description: "Default deep execution profile",
              model_ref: "default:gpt-4o-mini",
              tools: ["read"],
            },
            tool_audit: {
              description: "Audit profile",
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
            parsed.agents["tool_audit"].tool_failure_mode,
            ToolFailureMode::ContinueAsToolMessage
        );
        assert_eq!(parsed.agents["tool_audit"].max_iters, 20);
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
            "agent `deep` references unknown provider `missing` in `model_ref` `missing:gpt-4o-mini`; available providers: default"
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
            "agent `deep` references unknown `exit_target_profile` `ops`; available agents: deep"
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
              plan_mode: true,
              exit_target_profile: "build",
              permissions: {
                edit: "deny",
                shell: "deny"
              },
              tools: ["fs.read", "plan.exit"]
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
        assert!(parsed.agents["plan"].plan_mode);
        assert_eq!(
            parsed.agents["plan"].exit_target_profile.as_deref(),
            Some("build")
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
            },
            plan: {
              system_prompt: "Plan work"
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
        assert_eq!(
            parsed.agents["plan"].tool_failure_mode,
            ToolFailureMode::ContinueAsToolMessage
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
        let _ctx = DiscoveryTestContext::new(temp.path(), None);
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
        assert!(properties.contains_key("instructions"));
        assert!(!properties.contains_key("categories"));
        assert!(!properties.contains_key("profiles"));
        assert!(!properties.contains_key("runtime"));
        assert!(!properties.contains_key("integrations"));
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
        let xdg_config = xdg_root.join("harness/harness.jsonc");
        let cwd_config = temp.path().join("harness.jsonc");

        fs::create_dir_all(xdg_config.parent().expect("xdg parent"))
            .expect("create xdg config dir");
        fs::write(&xdg_config, "xdg").expect("write xdg config");
        fs::write(&cwd_config, "cwd").expect("write cwd config");

        let _context = DiscoveryTestContext::new(temp.path(), Some(&xdg_root));

        assert_eq!(resolve_config_path(None), Some(cwd_config));
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

        let _context = DiscoveryTestContext::new(temp.path(), Some(&xdg_root));

        assert_eq!(
            resolve_config_layer_paths(None),
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

        let _context = DiscoveryTestContext::new(&nested, Some(&xdg_root));
        let env_config_value = env_config.to_str().expect("env config utf-8").to_string();
        with_env_var_state("HARNESS_CONFIG", Some(&env_config_value), || {
            assert_eq!(
                resolve_config_layer_paths(None),
                vec![
                    xdg_config,
                    env_config,
                    repo_config,
                    repo_dot_config,
                    nested_config,
                    nested_dot_config,
                ]
            );
        });
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

        let _context = DiscoveryTestContext::new(temp.path(), Some(&xdg_root));

        let loaded = load_resolved_config(None)
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

        let _context = DiscoveryTestContext::new(temp.path(), None);
        with_env_var_state(
            "HARNESS_CONFIG_CONTENT",
            Some("{ permission: { bash: \"allow\" }, default_agent: \"plan\" }"),
            || {
                let loaded = load_resolved_config(None)
                    .expect("load config")
                    .expect("config should resolve");
                assert!(matches!(
                    loaded.config.permissions.defaults.shell,
                    PermissionMode::Allow
                ));
                assert_eq!(loaded.config.default_agent.as_deref(), Some("plan"));
            },
        );
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

        let _context = DiscoveryTestContext::new(temp.path(), Some(&xdg_root));

        let loaded = load_resolved_config(Some(&explicit_config))
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
    fn runtime_profile_tool_failure_mode_defaults_to_continue_as_tool_message() {
        let cfg = config_fixture(&deep_profile(r#"tools: ["read"],"#), "test-key", None, None);

        let parsed =
            load_config_from_str(&cfg).expect("config with default tool failure mode must parse");
        assert_eq!(parsed.agents["deep"].max_iters, default_max_iters());
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
        assert_eq!(parsed.agents["deep"].max_iters, 24);
        assert_eq!(
            parsed.agents["deep"].system_prompt.as_deref(),
            Some("Be precise.")
        );
    }

    #[test]
    fn env_var_default_fallback_works() {
        with_env_var_state("HARNESS_CONFIG_TEST_API_KEY_FALLBACK", None, || {
            let cfg = config_fixture(
                &deep_profile(r#"tools: ["fs.read"],"#),
                "${HARNESS_CONFIG_TEST_API_KEY_FALLBACK:-fallback-key}",
                None,
                None,
            );

            let parsed =
                load_config_from_str(&cfg).expect("config with fallback env reference must parse");
            let ProviderConfig::OpenAiCompatible(provider) =
                parsed.providers.get("default").unwrap();
            assert_eq!(provider.api_key, "fallback-key");
        });
    }

    #[test]
    fn env_var_default_fallback_uses_fallback_for_empty_var() {
        with_env_var_state("HARNESS_CONFIG_TEST_API_KEY_FALLBACK", Some(""), || {
            let cfg = config_fixture(
                &deep_profile(r#"tools: ["fs.read"],"#),
                "${HARNESS_CONFIG_TEST_API_KEY_FALLBACK:-fallback-key}",
                None,
                None,
            );

            let parsed = load_config_from_str(&cfg)
                .expect("config with empty fallback env reference must parse");
            let ProviderConfig::OpenAiCompatible(provider) =
                parsed.providers.get("default").unwrap();
            assert_eq!(provider.api_key, "fallback-key");
        });
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
    fn missing_openai_api_key_errors_even_for_cliproxy_loopback_base_url() {
        let err = resolve_string_reference("${OPENAI_API_KEY}", None)
            .expect_err("loopback providers should still require OPENAI_API_KEY");

        assert_eq!(
            err.to_string(),
            "environment variable `OPENAI_API_KEY` referenced in config is not set"
        );
    }

    #[test]
    fn configured_openai_api_key_env_reference_resolves_without_fallback() {
        with_env_var_state("OPENAI_API_KEY", Some("test-openai-api-key"), || {
            let resolved = resolve_string_reference("${OPENAI_API_KEY}", None)
                .expect("OPENAI_API_KEY should resolve when it is set");

            assert_eq!(resolved, "test-openai-api-key");
        });
    }

    #[test]
    fn upstream_env_reference_uses_empty_string_when_missing() {
        with_env_var_state("HARNESS_CONFIG_TEST_OPTIONAL_EMPTY", None, || {
            let cfg = config_fixture(
                &deep_profile(r#"tools: ["fs.read"],"#),
                "{env:HARNESS_CONFIG_TEST_OPTIONAL_EMPTY}",
                None,
                None,
            );

            let parsed = load_config_from_str(&cfg).expect("upstream env reference should parse");
            let ProviderConfig::OpenAiCompatible(provider) =
                parsed.providers.get("default").unwrap();
            assert_eq!(provider.api_key, "");
        });
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
        let _ctx = DiscoveryTestContext::new(&repo, None);

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
        assert_eq!(build.max_iters, 18);
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
        let _ctx = DiscoveryTestContext::new(&repo, None);

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
        let _ctx = DiscoveryTestContext::new(&repo, None);

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
        let _ctx = DiscoveryTestContext::new(&repo, None);

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
        let _ctx = DiscoveryTestContext::new(&outside, None);

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
        let _ctx = DiscoveryTestContext::new(&repo, None);

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
        let _ctx = DiscoveryTestContext::new(&repo, None);

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
}
