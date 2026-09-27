use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct FormatterOverride {
    pub disabled: bool,
    pub command: Option<Vec<String>>,
    pub environment: Option<BTreeMap<String, String>>,
    pub extensions: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct HooksConfig {
    pub lifecycle: Vec<LifecycleHookConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(deny_unknown_fields)]
pub struct LifecycleHookConfig {
    #[serde(default, alias = "name")]
    pub id: Option<String>,
    pub event: HookLifecycleEvent,
    pub command: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default = "hook_timeout", alias = "timeoutMs")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub critical: bool,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}
const fn hook_timeout() -> u64 {
    5000
}

impl LifecycleHookConfig {
    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        use std::path::Component;
        if self.command.is_empty()
            || self
                .command
                .iter()
                .any(|s| s.is_empty() || s.contains('\0'))
            || !(1..=300_000).contains(&self.timeout_ms)
            || self.id.as_ref().is_some_and(|s| {
                s.trim().is_empty() || s.len() > 256 || s.chars().any(char::is_control)
            })
            || self.cwd.as_ref().is_some_and(|s| {
                s.contains('\0')
                    || PathBuf::from(s)
                        .components()
                        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
            })
            || self.env.iter().any(|(key, value)| {
                key.is_empty()
                    || key.contains(['=', '\0'])
                    || value.contains('\0')
                    || key.starts_with("HARNESS_HOOK_")
            })
            || self
                .command
                .iter()
                .map(String::len)
                .chain(self.env.iter().map(|(k, v)| k.len() + v.len()))
                .sum::<usize>()
                > 128 * 1024
        {
            return Err(ConfigError("invalid hook: use a command, a relative cwd, a 1–300000 ms timeout and at most 128 KiB of arguments/environment; HARNESS_HOOK_* is reserved".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq, Copy)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct HookRuntimeConfig {
    pub hooks: HooksConfig,
    pub shell_allowlist: ShellAllowlist,
    #[serde(skip)]
    pub suppress_execution: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct SkillsConfig {
    #[serde(alias = "projectRoots", alias = "paths")]
    pub project_roots: Vec<PathBuf>,
    #[serde(alias = "globalRoots")]
    pub global_roots: Vec<PathBuf>,
    pub urls: Vec<String>,
    #[serde(alias = "disabledIds")]
    pub disabled: Vec<String>,
    #[serde(alias = "walkToGitRoot")]
    pub walk_to_git_root: bool,
    pub permissions: BTreeMap<String, PermissionMode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct LspConfig {
    pub disabled: bool,
    pub servers: BTreeMap<String, LspServerConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct LspServerConfig {
    pub disabled: bool,
    pub command: Option<Vec<String>>,
    pub extensions: Option<Vec<String>>,
    pub env: BTreeMap<String, String>,
    pub initialization: Option<serde_json::Value>,
}

impl HookLifecycleEvent {
    pub const fn as_str(self) -> &'static str {
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
