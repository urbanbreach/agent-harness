use schemars::schema_for;

use super::*;

const ALLOWED_PUBLIC_TOP_LEVEL_CONFIG_KEYS: &[&str] = &[
    "$schema",
    "providers",
    "provider",
    "model",
    "small_model",
    "smallModel",
    "model_profile",
    "modelProfile",
    "model_profiles",
    "agents",
    "agent",
    "categories",
    "profiles",
    "default_agent",
    "defaultAgent",
    "permissions",
    "permission",
    "runtime",
    "backgroundTask",
    "paths",
    "deterministic",
    "integrations",
    "mcp",
    "hooks",
    "skills",
    "lsp",
    "logging",
    "ui",
    "hashline_edit",
    "hashlineEdit",
    "instructions",
];

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
    pub agent: BTreeMap<String, PublicAgentConfig>,
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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicRuntimeSettingsConfig {
    #[serde(default)]
    pub compaction: CompactionRuntimeConfig,
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
    #[serde(default)]
    pub tools: Vec<String>,
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
    #[serde(default, rename = "*")]
    pub fallback: Option<PermissionMode>,
    #[serde(default)]
    pub edit: Option<PublicRulePermissionValue>,
    #[serde(default, alias = "shell")]
    pub bash: Option<PublicRulePermissionValue>,
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

pub(super) fn validate_public_root_config_object(
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
            format_backticked_list(ALLOWED_PUBLIC_TOP_LEVEL_CONFIG_KEYS.iter().copied())
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
        _ => Err(ConfigError::InvalidReference(format!(
            "permission selector rules are only supported for `bash` and `edit`, not `{kind}`"
        ))),
    }
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
    let path = Path::new(selector.trim());
    if path.is_absolute() {
        return None;
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn default_internal_integrations_config() -> IntegrationsConfig {
    IntegrationsConfig {
        remote_search: RemoteSearchConfig::default(),
        mcp: McpConfig::default(),
    }
}

fn canonicalize_object_aliases(
    object: &mut serde_json::Map<String, serde_json::Value>,
    aliases: &[(&str, &str)],
) {
    for (alias, canonical) in aliases {
        if let Some(value) = object.remove(*alias) {
            match object.get_mut(*canonical) {
                Some(existing) => merge_config_value(existing, value),
                None => {
                    object.insert((*canonical).to_string(), value);
                }
            }
        }
    }
}

fn canonicalize_runtime_aliases(runtime: &mut serde_json::Value) {
    let Some(runtime_object) = runtime.as_object_mut() else {
        return;
    };

    canonicalize_object_aliases(
        runtime_object,
        &[
            ("backgroundTasks", "background_tasks"),
            ("sessionDir", "session_dir"),
        ],
    );

    if let Some(background_tasks) = runtime_object
        .get_mut("background_tasks")
        .and_then(serde_json::Value::as_object_mut)
    {
        canonicalize_object_aliases(
            background_tasks,
            &[
                ("defaultConcurrency", "default_concurrency"),
                ("providerConcurrency", "provider_concurrency"),
                ("modelConcurrency", "model_concurrency"),
                ("staleTimeoutMs", "stale_timeout_ms"),
                ("messageStalenessTimeoutMs", "message_staleness_timeout_ms"),
            ],
        );
    }

    if let Some(permissions) = runtime_object
        .get_mut("permissions")
        .and_then(serde_json::Value::as_object_mut)
    {
        canonicalize_object_aliases(permissions, &[("askTimeoutMs", "ask_timeout_ms")]);
    }

    if let Some(prompt) = runtime_object
        .get_mut("prompt")
        .and_then(serde_json::Value::as_object_mut)
    {
        canonicalize_object_aliases(prompt, &[("waitTimeoutMs", "wait_timeout_ms")]);
    }

    if let Some(compaction) = runtime_object
        .get_mut("compaction")
        .and_then(serde_json::Value::as_object_mut)
    {
        canonicalize_object_aliases(
            compaction,
            &[
                ("modelBacked", "model_backed"),
                ("modelRef", "model_ref"),
                ("model", "model_ref"),
                ("splitOversizedTurns", "split_oversized_turns"),
                ("autoRetryOverflow", "auto_retry_overflow"),
                ("structuredSummaryContract", "structured_summary_contract"),
                ("estimatedTokenTriggers", "estimated_token_triggers"),
                ("fallbackInputTokens", "fallback_input_tokens"),
            ],
        );
    }
}

fn default_shipped_agents(model_ref: &str) -> BTreeMap<String, ProfileConfig> {
    BTreeMap::from([(
        "build".to_string(),
        ProfileConfig {
            description: "Implementation lane: execute the requested work and verify the result."
                .to_string(),
            system_prompt: None,
            model_ref: model_ref.to_string(),
            variant: None,
            temperature: None,
            permissions: Some(ProfilePermissions {
                fallback: None,
                edit: Some(PermissionMode::Allow),
                shell: Some(PermissionMode::Allow),
                network: Some(PermissionMode::Allow),
                question: Some(PermissionMode::Allow),
                task: Some(PermissionMode::Allow),
                webfetch: Some(PermissionMode::Allow),
                websearch: Some(PermissionMode::Allow),
                codesearch: Some(PermissionMode::Allow),
                lsp: Some(PermissionMode::Allow),
                rules: PermissionRuleSet::default(),
            }),
            max_iters: 24,
            tool_failure_mode: ToolFailureMode::ContinueAsToolMessage,
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
                "edit",
                "bash",
                "batch",
                "task",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        },
    )])
}

fn fallback_public_agent_description(name: &str) -> String {
    let words = name
        .split(['_', '-', ' '])
        .filter(|part| !part.is_empty())
        .filter_map(|part| {
            let mut chars = part.chars();
            let first = chars.next()?;
            Some(format!("{}{}", first.to_uppercase(), chars.as_str()))
        })
        .collect::<Vec<_>>();
    let humanized = if words.is_empty() {
        name.to_string()
    } else {
        words.join(" ")
    };
    format!("The {humanized} agent")
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
        .unwrap_or_else(|| fallback_public_agent_description(name));
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
    let edit = public_rule_mode(&permissions.edit);
    let shell = public_rule_mode(&permissions.bash);
    let edit_rules = public_selector_rules("edit", permissions.edit)?;
    let shell_rules = public_selector_rules("bash", permissions.bash)?;

    Ok(ProfilePermissions {
        fallback: permissions.fallback,
        edit,
        shell,
        network: permissions.network,
        question: permissions.question,
        task: permissions.task,
        webfetch: permissions.webfetch,
        websearch: permissions.websearch,
        codesearch: permissions.codesearch,
        lsp: permissions.lsp,
        rules: PermissionRuleSet {
            shell: shell_rules,
            edit: edit_rules,
        },
    })
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

    let parsed: PublicPermissionValue =
        serde_json::from_value(value).map_err(|err| ConfigError::ParseJson5(err.to_string()))?;
    let fallback = default_internal_permissions_config();

    let parsed = match parsed {
        PublicPermissionValue::Config(parsed) => parsed,
        PublicPermissionValue::Mode(mode) => {
            return serde_json::to_value(PermissionsConfig {
                defaults: PermissionDefaultsConfig {
                    edit: mode.clone(),
                    shell: mode.clone(),
                    network: mode.clone(),
                    question: Some(mode.clone()),
                    task: Some(mode.clone()),
                    webfetch: Some(mode.clone()),
                    websearch: Some(mode.clone()),
                    codesearch: Some(mode.clone()),
                    lsp: Some(mode),
                },
                fallback: None,
                rules: PermissionRuleSet::default(),
                shell_allowlist: fallback.shell_allowlist,
            })
            .map_err(|err| ConfigError::ParseJson5(err.to_string()));
        }
    };

    let global = parsed.fallback.clone();
    let edit = public_rule_mode(&parsed.edit)
        .or_else(|| global.clone())
        .unwrap_or(fallback.defaults.edit);
    let shell = public_rule_mode(&parsed.bash)
        .or_else(|| global.clone())
        .unwrap_or(fallback.defaults.shell);
    let edit_rules = public_selector_rules("edit", parsed.edit)?;
    let shell_rules = public_selector_rules("bash", parsed.bash)?;

    serde_json::to_value(PermissionsConfig {
        defaults: PermissionDefaultsConfig {
            edit,
            shell,
            network: parsed
                .network
                .or_else(|| global.clone())
                .unwrap_or(fallback.defaults.network),
            question: parsed
                .question
                .or_else(|| global.clone())
                .or(fallback.defaults.question),
            task: parsed
                .task
                .or_else(|| global.clone())
                .or(fallback.defaults.task),
            webfetch: parsed
                .webfetch
                .or_else(|| global.clone())
                .or(fallback.defaults.webfetch),
            websearch: parsed
                .websearch
                .or_else(|| global.clone())
                .or(fallback.defaults.websearch),
            codesearch: parsed
                .codesearch
                .or_else(|| global.clone())
                .or(fallback.defaults.codesearch),
            lsp: parsed
                .lsp
                .or_else(|| global.clone())
                .or(fallback.defaults.lsp),
        },
        fallback: parsed.fallback,
        rules: PermissionRuleSet {
            shell: shell_rules,
            edit: edit_rules,
        },
        shell_allowlist: parsed.shell_allowlist.unwrap_or(fallback.shell_allowlist),
    })
    .map_err(|err| ConfigError::ParseJson5(err.to_string()))
}

pub(super) fn translate_public_runtime_root(
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

    let mut model_profiles = serde_json::json!({});
    for key in ["model_profile", "modelProfile", "model_profiles"] {
        if let Some(value) = object.get(key) {
            merge_config_value(&mut model_profiles, value.clone());
        }
    }
    translated.insert("model_profile".to_string(), model_profiles);

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
        .map(default_shipped_agents)
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
    for (name, profile) in shipped {
        agents.entry(name).or_insert(profile);
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
    canonicalize_runtime_aliases(&mut runtime);
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
