// allow: SIZE_OK — public config contract (typed schema + validation + aliases)
use schemars::generate::SchemaSettings;
use serde_json::Value; // allow: DYNAMIC — type alias for internal JSON processing in helper functions

use super::*;

use self::agents::{default_shipped_agents, public_agent_to_profile};

mod agents;
mod contract;
mod normalization;
pub(crate) use normalization::translate_public_formatter_config;
pub use self::agents::{PublicAgentConfig, PublicAgentMap, PublicAgentTools};
pub use self::contract::{
    public_config_contract, PublicConfigAlias, PublicConfigAliasScope, PublicConfigCompactionKnob,
    PublicConfigContract, PublicConfigKeyStatus, PublicConfigPermissionName, PublicConfigSurface,
    PublicConfigTopLevelKey, PublicUnsupportedInactiveValue,
};

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
    #[serde(
        rename = "model_profile",
        default,
        alias = "modelProfile",
        alias = "model_profiles"
    )]
    pub model_profiles: BTreeMap<String, ModelProfileConfig>,
    #[serde(default)]
    pub agent: PublicAgentMap,
    #[serde(default)]
    pub mode: PublicAgentMap,
    #[serde(default, alias = "defaultAgent")]
    pub default_agent: Option<String>,
    #[serde(default)]
    pub permission: PublicPermissionValue,
    #[serde(default)]
    pub mcp: BTreeMap<String, McpServerConfig>,
    #[serde(default)]
    pub runtime: PublicRuntimeSettingsConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default)]
    pub instructions: Option<InstructionList>,
    #[serde(default)]
    #[schemars(skip)]
    pub shell: Option<String>,
    #[serde(default, rename = "logLevel")]
    #[schemars(skip)]
    pub log_level: Option<String>,
    #[serde(default)]
    pub server: Option<serde_json::Value>, // allow: DYNAMIC — MCP plugin options are intentionally untyped
    #[serde(default)]
    pub command: Option<serde_json::Value>, // allow: DYNAMIC — MCP plugin options are intentionally untyped
    #[serde(default)]
    #[schemars(skip)]
    pub snapshot: Option<bool>,
    #[serde(default)]
    pub autoshare: Option<bool>,
    #[serde(default)]
    pub disabled_providers: Option<Vec<String>>,
    #[serde(default)]
    pub enabled_providers: Option<Vec<String>>,
    #[serde(default)]
    #[schemars(skip)]
    pub username: Option<String>,
    #[serde(default)]
    pub formatter: Option<FormatterConfig>,
    #[serde(default)]
    pub lsp: Option<LspConfig>,
    #[serde(default)]
    #[schemars(skip)]
    pub layout: Option<String>,
    #[serde(default)]
    #[schemars(skip)]
    pub tools: Option<BTreeMap<String, bool>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicRuntimeSettingsConfig {
    #[serde(default)]
    pub compaction: CompactionRuntimeConfig,
    #[serde(default, alias = "providerRetry")]
    pub provider_retry: ProviderRetryRuntimeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PublicPermissionValue {
    Mode(PermissionMode),
    Config(PublicPermissionConfig),
}

impl Default for PublicPermissionValue {
    fn default() -> Self {
        Self::Config(PublicPermissionConfig::default())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PublicRulePermissionValue {
    Mode(PermissionMode),
    Rules(BTreeMap<String, PermissionMode>),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicPermissionConfig {
    #[serde(default, rename = "*")]
    pub fallback: Option<PermissionMode>,
    #[serde(default)]
    pub edit: Option<PublicRulePermissionValue>,
    #[serde(default, alias = "shell")]
    pub bash: Option<PublicRulePermissionValue>,
    #[serde(default)]
    pub question: Option<PermissionMode>,
    #[serde(default)]
    pub task: Option<PublicRulePermissionValue>,
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
    #[serde(default, rename = "*")]
    pub fallback: Option<PermissionMode>,
    #[serde(default)]
    pub edit: Option<PublicRulePermissionValue>,
    #[serde(default, alias = "shell")]
    pub bash: Option<PublicRulePermissionValue>,
    #[serde(default)]
    pub question: Option<PermissionMode>,
    #[serde(default)]
    pub task: Option<PublicRulePermissionValue>,
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

pub(super) fn validate_public_root_config_object(
    object: &serde_json::Map<String, Value>,
) -> Result<(), ConfigError> {
    let contract = public_config_contract();
    let mut unknown = object
        .keys()
        .filter(|key| contract.runtime_top_level_key(key).is_none())
        .map(String::as_str)
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        unknown.sort_unstable();
        let allowed = contract
            .runtime_top_level_keys
            .iter()
            .map(|entry| entry.name);
        return Err(ConfigError::UnknownTopLevelKeys(format!(
            "unknown top-level config keys: {}; expected only {}",
            format_backticked_list(unknown.iter().copied()),
            format_backticked_list(allowed)
        )));
    }

    let mut unsupported_active = contract
        .runtime_top_level_keys
        .iter()
        .filter(|entry| entry.status == PublicConfigKeyStatus::UnsupportedActive)
        .filter_map(|entry| {
            let value = object.get(entry.name)?;
            let inactive = entry
                .inactive_value
                .is_some_and(|inactive| inactive.matches(value));
            (!inactive).then_some(entry.name)
        })
        .collect::<Vec<_>>();
    if !unsupported_active.is_empty() {
        unsupported_active.sort_unstable();
        return Err(ConfigError::RetiredConfigKeys(format!(
            concat!(
                "unsupported active ",
                "unsupported active retired config keys: {}; this harness accepts inert compatibility settings, but does not execute server, command, plugin, sharing, update, or enterprise product features"
            ),
            format_backticked_list(unsupported_active)
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
        fallback: None,
        rules: PermissionRuleSet::default(),
        shell_allowlist: ShellAllowlist::default(),
    }
}

fn public_rule_mode(value: &Option<PublicRulePermissionValue>) -> Option<PermissionMode> {
    match value {
        Some(PublicRulePermissionValue::Mode(mode)) => Some(mode.clone()),
        Some(PublicRulePermissionValue::Rules(_)) | None => None,
    }
}

fn public_selector_rules(
    kind: &str,
    value: Option<PublicRulePermissionValue>,
) -> Result<Vec<PermissionSelectorRule>, ConfigError> {
    match value {
        Some(PublicRulePermissionValue::Rules(rules)) => rules
            .into_iter()
            .map(|(selector, mode)| {
                Ok(PermissionSelectorRule {
                    selector: public_permission_selector(kind, &selector)?,
                    mode,
                })
            })
            .collect(),
        Some(PublicRulePermissionValue::Mode(_)) | None => Ok(Vec::new()),
    }
}

fn public_permission_selector(
    kind: &str,
    selector: &str,
) -> Result<PermissionSelector, ConfigError> {
    match kind {
        "bash" => public_bash_selector(selector),
        "edit" => public_edit_selector(selector),
        "task" => public_task_selector(selector),
        _ => Err(ConfigError::InvalidReference(format!(
            "permission selector rules are only supported for `bash`, `edit`, and `task`, not `{kind}`"
        ))),
    }
}

fn public_task_selector(selector: &str) -> Result<PermissionSelector, ConfigError> {
    let trimmed = selector.trim();
    if trimmed == "*" {
        return Ok(PermissionSelector::CatchAll);
    }
    if trimmed.is_empty() || trimmed.starts_with('/') {
        return Err(ConfigError::InvalidReference(format!(
            "invalid task permission selector `{selector}`; use an agent name, glob pattern, or `*`"
        )));
    }
    if trimmed.contains('*') {
        return Ok(PermissionSelector::Glob(trimmed.to_string()));
    }
    Ok(PermissionSelector::Exact(trimmed.to_string()))
}

fn public_bash_selector(selector: &str) -> Result<PermissionSelector, ConfigError> {
    let trimmed = selector.trim();
    if trimmed == "*" {
        return Ok(PermissionSelector::CatchAll);
    }
    if trimmed.is_empty() || trimmed.starts_with('/') {
        return Err(ConfigError::InvalidReference(format!(
            "invalid bash permission selector `{selector}`; use an exact command, trailing `*` prefix, or `*`"
        )));
    }
    if let Some(prefix) = trimmed.strip_suffix('*') {
        if prefix.is_empty() || prefix.contains('*') {
            return Err(ConfigError::InvalidReference(format!(
                "invalid bash permission selector `{selector}`; only a single trailing `*` prefix is supported"
            )));
        }
        return Ok(PermissionSelector::Prefix(prefix.to_string()));
    }
    if trimmed.contains('*') {
        return Err(ConfigError::InvalidReference(format!(
            "invalid bash permission selector `{selector}`; only a trailing `*` prefix is supported"
        )));
    }
    Ok(PermissionSelector::Exact(trimmed.to_string()))
}

fn public_edit_selector(selector: &str) -> Result<PermissionSelector, ConfigError> {
    let trimmed = selector.trim();
    if trimmed == "*" {
        return Ok(PermissionSelector::CatchAll);
    }
    if let Some(prefix) = trimmed.strip_suffix("/**") {
        let normalized = normalize_public_workspace_selector(prefix).ok_or_else(|| {
            ConfigError::InvalidReference(format!(
                "invalid edit permission selector `{selector}`; path prefixes must be workspace-relative and end with `/**`"
            ))
        })?;
        return Ok(PermissionSelector::Prefix(format!("{normalized}/")));
    }
    if trimmed.contains('*') {
        return Err(ConfigError::InvalidReference(format!(
            "invalid edit permission selector `{selector}`; only trailing `/**` prefixes or `*` are supported"
        )));
    }
    let normalized = normalize_public_workspace_selector(trimmed).ok_or_else(|| {
        ConfigError::InvalidReference(format!(
            "invalid edit permission selector `{selector}`; use a workspace-relative path, trailing `/**` prefix, or `*`"
        ))
    })?;
    Ok(PermissionSelector::Exact(normalized))
}

fn normalize_public_workspace_selector(selector: &str) -> Option<String> {
    crate::path_selector::normalize_workspace_relative_path(Path::new(selector.trim()))
}

fn default_internal_integrations_config() -> IntegrationsConfig {
    IntegrationsConfig {
        remote_search: RemoteSearchConfig::default(),
        mcp: McpConfig::default(),
    }
}

pub(super) fn translate_public_runtime_root(
    root: Value,
) -> Result<(Value, Vec<String>), ConfigError> {
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
    if let Some(value) = object.get("disabled_providers").cloned() {
        translated.insert("disabled_providers".to_string(), value);
    }
    if let Some(value) = object.get("enabled_providers").cloned() {
        translated.insert("enabled_providers".to_string(), value);
    }

    let model = object
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_string);
    let small_model = object
        .get("small_model")
        .or_else(|| object.get("smallModel"))
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut model_profiles = serde_json::json!({});
    for key in ["model_profile", "modelProfile", "model_profiles"] {
        if let Some(value) = object.get(key) {
            merge_config_value(&mut model_profiles, value.clone());
        }
    }
    translated.insert("model_profile".to_string(), model_profiles);

    let mut agents = BTreeMap::new();
    if let Some(value) = object.get("agents") {
        let mut legacy: BTreeMap<String, ProfileConfig> = serde_json::from_value(value.clone())
            .map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
        for profile in legacy.values_mut() {
            profile.model_ref_explicit = true;
        }
        agents.extend(legacy);
    }
    for alias in ["categories", "profiles"] {
        if let Some(value) = object.get(alias) {
            let mut legacy: BTreeMap<String, ProfileConfig> = serde_json::from_value(value.clone())
                .map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
            for profile in legacy.values_mut() {
                profile.model_ref_explicit = true;
            }
            agents.extend(legacy);
        }
    }

    let shipped = model
        .as_deref()
        .map(|model_ref| default_shipped_agents(model_ref, small_model.as_deref()))
        .unwrap_or_default();

    let mut disabled_agents = BTreeSet::new();
    for key in ["mode", "agent"] {
        if let Some(value) = object.get(key) {
            let public_agents: PublicAgentMap = serde_json::from_value(value.clone())
                .map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
            for (name, public_agent) in public_agents.into_entries() {
                if public_agent.disable || public_agent.enable == Some(false) {
                    agents.remove(&name);
                    disabled_agents.insert(name);
                    continue;
                }
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
    }
    for (name, profile) in shipped {
        if !disabled_agents.contains(&name) {
            agents.entry(name).or_insert(profile);
        }
    }

    translated.insert(
        "agents".to_string(),
        serde_json::to_value(agents).map_err(|err| ConfigError::ParseJson5(err.to_string()))?,
    );

    if let Some(small_model) = &small_model {
        translated.insert(
            "small_model".to_string(),
            Value::String(small_model.clone()),
        );
    }

    if !disabled_agents.is_empty() {
        translated.insert(
            "disabled_agents".to_string(),
            serde_json::to_value(&disabled_agents).unwrap_or(Value::Array(Vec::new())),
        );
    }

    if let Some(default_agent) = object
        .get("default_agent")
        .or_else(|| object.get("defaultAgent"))
        .cloned()
    {
        if let Some(default_agent_name) = default_agent.as_str() {
            if disabled_agents.contains(default_agent_name.trim()) {
                return Err(ConfigError::InvalidReference(format!(
                    "default_agent `{}` references a disabled agent",
                    default_agent_name.trim()
                )));
            }
        }
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
            normalization::translate_public_permission_value(value.clone())?,
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
        "compaction": {
            "model_backed": false,
            "split_oversized_turns": false,
            "auto_retry_overflow": true,
            "structured_summary_contract": true,
            "estimated_token_triggers": true,
            "fallback_input_tokens": 32768,
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
    normalization::canonicalize_runtime_aliases(&mut runtime);
    translated.insert("runtime".to_string(), runtime);

    let mut integrations = serde_json::to_value(default_internal_integrations_config())
        .map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
    if let Some(value) = object.get("integrations") {
        merge_config_value(&mut integrations, value.clone());
    }
    if let Some(value) = object.get("mcp") {
        let mcp_value =
            serde_json::json!({ "servers": normalization::normalize_public_mcp_servers(value.clone()) });
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

    if let Some(value) = object.get("skills") {
        translated.insert("skills".to_string(), normalization::normalize_public_skills_config(value));
    }

    if let Some(value) = object.get("lsp").and_then(normalization::normalize_public_lsp_config) {
        translated.insert("lsp".to_string(), value);
    }

    let formatter = normalization::translate_public_formatter_config(object.get("formatter"))?;
    translated.insert(
        "formatter".to_string(),
        serde_json::to_value(formatter).map_err(|err| ConfigError::ParseJson5(err.to_string()))?,
    );

    let instructions = object
        .get("instructions")
        .map(|value| {
            serde_json::from_value::<InstructionList>(value.clone())
                .map(|parsed| parsed.entries())
                .map_err(|err| ConfigError::ParseJson5(err.to_string()))
        })
        .transpose()?
        .unwrap_or_default();

    Ok((Value::Object(translated), instructions))
}

pub fn harness_schema_pretty_json() -> Result<String, ConfigError> {
    let mut schema = serde_json::to_value(
        SchemaSettings::draft07()
            .into_generator()
            .into_root_schema_for::<PublicRuntimeConfig>(),
    )
    .map_err(|err| ConfigError::SerializeSchema(err.to_string()))?;
    let definitions = schema
        .get_mut("definitions")
        .and_then(Value::as_object_mut);
    if let Some(definitions) = definitions {
        if let Some(agent_map) = definitions
            .get_mut("PublicAgentMap")
            .and_then(Value::as_object_mut)
        {
            agent_map.insert(
                "additionalProperties".to_string(),
                serde_json::json!({ "$ref": "#/definitions/PublicAgentConfig" }),
            );
            agent_map.insert(
                "description".to_string(),
                Value::String(
                    "Named agent definitions. Built-in Harness-compatible agents are explicit so editors can complete them, and custom names are accepted through the same shape."
                        .to_string(),
                ),
            );
        }
        if let Some(disable_property) = definitions
            .get_mut("PublicAgentConfig")
            .and_then(|agent_config| agent_config.get_mut("properties"))
            .and_then(Value::as_object_mut)
            .and_then(|properties| properties.get_mut("disable"))
            .and_then(Value::as_object_mut)
        {
            disable_property.insert(
                "description".to_string(),
                Value::String(
                    "Compatibility negative toggle. Equivalent to `enable: false`.".to_string(),
                ),
            );
        }
    }
    serde_json::to_string_pretty(&schema)
        .map_err(|err| ConfigError::SerializeSchema(err.to_string()))
}

pub fn harness_tui_schema_pretty_json() -> Result<String, ConfigError> {
    let schema = SchemaSettings::draft07()
        .into_generator()
        .into_root_schema_for::<PublicTuiConfig>();
    serde_json::to_string_pretty(&schema)
        .map_err(|err| ConfigError::SerializeSchema(err.to_string()))
}
