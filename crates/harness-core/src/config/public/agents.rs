// allow: SIZE_OK — public agent schema, shipped profile table, and permission translation
use super::*;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicAgentMap {
    #[serde(default)]
    pub default: PublicAgentConfig,
    #[serde(default)]
    pub explore: PublicAgentConfig,
    #[serde(default)]
    pub general: PublicAgentConfig,
    #[serde(default)]
    pub librarian: PublicAgentConfig,
}

impl PublicAgentMap {
    pub(super) fn into_entries(self) -> [(&'static str, PublicAgentConfig); 4] {
        [
            ("default", self.default),
            ("explore", self.explore),
            ("general", self.general),
            ("librarian", self.librarian),
        ]
    }
}

/// Configuration shared by the primary agent and named subagents.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicAgentConfig {
    #[serde(default, alias = "systemPrompt", alias = "prompt")]
    pub system_prompt: Option<String>,
    #[serde(default, alias = "model_ref", alias = "modelRef")]
    pub model: Option<String>,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default, alias = "topP")]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub options: BTreeMap<String, serde_json::Value>,
    #[serde(default, alias = "permissions")]
    pub permission: Option<PublicProfilePermissions>,
    #[serde(default, alias = "maxIters", alias = "steps", alias = "maxSteps")]
    pub max_iters: Option<usize>,
    #[serde(default, alias = "toolFailureMode")]
    pub tool_failure_mode: Option<ToolFailureMode>,
    #[serde(default)]
    pub tools: PublicAgentTools,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PublicAgentTools {
    List(Vec<String>),
    Map(BTreeMap<String, bool>),
}

impl Default for PublicAgentTools {
    fn default() -> Self {
        Self::List(Vec::new())
    }
}

impl PublicAgentTools {
    pub fn tool_ids(self) -> Vec<String> {
        match self {
            Self::List(tools) => tools,
            Self::Map(tools) => tools
                .into_iter()
                .filter_map(|(tool, enabled)| enabled.then_some(tool))
                .collect(),
        }
    }
}

pub(super) fn shipped_agent_profiles(model_ref: &str) -> BTreeMap<String, ProfileConfig> {
    BTreeMap::from([
        (
            "default".to_string(),
            profile(
                model_ref,
                ShippedProfileSpec {
                    description: "General-purpose Harness agent.",
                    mode: AgentMode::Primary,
                    tools: primary_tools(),
                    permissions: None,
                },
            ),
        ),
        (
            "explore".to_string(),
            profile(
                model_ref,
                ShippedProfileSpec {
                    description: "Read-only codebase exploration subagent.",
                    mode: AgentMode::Subagent,
                    tools: research_tools(),
                    permissions: Some(explore_permissions()),
                },
            ),
        ),
        (
            "general".to_string(),
            profile(
                model_ref,
                ShippedProfileSpec {
                    description: "General-purpose implementation and research subagent.",
                    mode: AgentMode::Subagent,
                    tools: general_tools(),
                    permissions: Some(general_permissions()),
                },
            ),
        ),
        (
            "librarian".to_string(),
            profile(
                model_ref,
                ShippedProfileSpec {
                    description: "Documentation and external research subagent.",
                    mode: AgentMode::Subagent,
                    tools: librarian_tools(),
                    permissions: Some(librarian_permissions()),
                },
            ),
        ),
    ])
}

pub(super) fn public_agent_to_profile(
    agent: PublicAgentConfig,
    default_model_ref: Option<&str>,
    base: ProfileConfig,
) -> Result<ProfileConfig, ConfigError> {
    let model_ref_explicit = agent.model.is_some() || base.model_ref_explicit;
    let model_ref = agent
        .model
        .or_else(|| default_model_ref.map(str::to_string))
        .unwrap_or_else(|| base.model_ref.clone());
    let tools = agent.tools.tool_ids();
    let options = if agent.options.is_empty() {
        base.options.clone()
    } else {
        agent.options
    };

    Ok(ProfileConfig {
        name: None,
        description: base.description,
        system_prompt: agent.system_prompt.or(base.system_prompt),
        model_ref,
        model_ref_explicit,
        variant: agent.variant.or(base.variant),
        temperature: agent.temperature.or(base.temperature),
        top_p: agent.top_p.or(base.top_p),
        mode: base.mode,
        hidden: false,
        color: None,
        options,
        permissions: agent
            .permission
            .map(translate_public_profile_permissions)
            .transpose()?
            .or(base.permissions),
        max_iters: agent.max_iters.or(base.max_iters),
        tool_failure_mode: agent.tool_failure_mode.unwrap_or(base.tool_failure_mode),
        tools: if tools.is_empty() { base.tools } else { tools },
    })
}

struct ShippedProfileSpec {
    description: &'static str,
    mode: AgentMode,
    tools: Vec<String>,
    permissions: Option<ProfilePermissions>,
}

fn profile(model_ref: &str, spec: ShippedProfileSpec) -> ProfileConfig {
    ProfileConfig {
        name: None,
        description: spec.description.to_string(),
        system_prompt: None,
        model_ref: model_ref.to_string(),
        model_ref_explicit: false,
        variant: None,
        temperature: None,
        top_p: None,
        mode: spec.mode,
        hidden: false,
        color: None,
        options: BTreeMap::new(),
        permissions: spec.permissions,
        max_iters: None,
        tool_failure_mode: default_runtime_tool_failure_mode(),
        tools: spec.tools,
    }
}

fn primary_tools() -> Vec<String> {
    tool_ids(&[
        "todowrite",
        "todoread",
        "question",
        "task",
        "background_output",
        "background_cancel",
        "skill",
        "websearch",
        "webfetch",
        "codesearch",
        "ast_grep_search",
        "lsp",
        "read",
        "glob",
        "grep",
        "list",
        "session_list",
        "session_read",
        "session_search",
        "session_info",
        "edit",
        "write",
        "apply_patch",
        "bash",
        "batch",
    ])
}

fn general_tools() -> Vec<String> {
    tool_ids(&[
        "question",
        "skill",
        "websearch",
        "webfetch",
        "codesearch",
        "ast_grep_search",
        "lsp",
        "read",
        "glob",
        "grep",
        "list",
        "session_list",
        "session_read",
        "session_search",
        "session_info",
        "edit",
        "write",
        "apply_patch",
        "bash",
        "batch",
    ])
}

fn research_tools() -> Vec<String> {
    tool_ids(&[
        "bash",
        "webfetch",
        "websearch",
        "read",
        "glob",
        "grep",
        "list",
        "session_list",
        "session_read",
        "session_search",
        "session_info",
        "ast_grep_search",
        "batch",
    ])
}

fn librarian_tools() -> Vec<String> {
    tool_ids(&[
        "bash",
        "webfetch",
        "websearch",
        "codesearch",
        "lsp",
        "read",
        "glob",
        "grep",
        "list",
        "session_list",
        "session_read",
        "session_search",
        "session_info",
        "ast_grep_search",
        "batch",
    ])
}

fn tool_ids(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| (*id).to_string()).collect()
}

fn general_permissions() -> ProfilePermissions {
    ProfilePermissions {
        edit: Some(PermissionMode::Allow),
        shell: Some(PermissionMode::Allow),
        network: Some(PermissionMode::Allow),
        question: Some(PermissionMode::Deny),
        task: Some(PermissionMode::Deny),
        webfetch: Some(PermissionMode::Allow),
        websearch: Some(PermissionMode::Allow),
        codesearch: Some(PermissionMode::Allow),
        lsp: Some(PermissionMode::Allow),
        ..ProfilePermissions::default()
    }
}

fn explore_permissions() -> ProfilePermissions {
    ProfilePermissions {
        edit: Some(PermissionMode::Deny),
        shell: Some(PermissionMode::Allow),
        network: Some(PermissionMode::Allow),
        question: Some(PermissionMode::Deny),
        task: Some(PermissionMode::Deny),
        todowrite: Some(PermissionMode::Deny),
        webfetch: Some(PermissionMode::Allow),
        websearch: Some(PermissionMode::Allow),
        codesearch: Some(PermissionMode::Deny),
        lsp: Some(PermissionMode::Deny),
        ..ProfilePermissions::default()
    }
}

fn librarian_permissions() -> ProfilePermissions {
    ProfilePermissions {
        edit: Some(PermissionMode::Deny),
        shell: Some(PermissionMode::Allow),
        network: Some(PermissionMode::Allow),
        question: Some(PermissionMode::Deny),
        task: Some(PermissionMode::Deny),
        todowrite: Some(PermissionMode::Deny),
        webfetch: Some(PermissionMode::Allow),
        websearch: Some(PermissionMode::Allow),
        codesearch: Some(PermissionMode::Allow),
        lsp: Some(PermissionMode::Allow),
        ..ProfilePermissions::default()
    }
}

fn translate_public_profile_permissions(
    permissions: PublicProfilePermissions,
) -> Result<ProfilePermissions, ConfigError> {
    let edit = public_rule_mode(&permissions.edit);
    let shell = public_rule_mode(&permissions.bash);
    let task = public_rule_mode(&permissions.task);
    let read = public_rule_mode(&permissions.read);
    let external_directory = public_rule_mode(&permissions.external_directory);
    let edit_rules = public_selector_rules("edit", permissions.edit)?;
    let shell_rules = public_selector_rules("bash", permissions.bash)?;
    let task_rules = public_selector_rules("task", permissions.task)?;
    let read_rules = public_selector_rules("read", permissions.read)?;
    let external_directory_rules =
        public_selector_rules("external_directory", permissions.external_directory)?;

    Ok(ProfilePermissions {
        fallback: permissions.fallback,
        edit,
        shell,
        network: permissions.network,
        question: permissions.question,
        task,
        todowrite: None,
        webfetch: permissions.webfetch,
        websearch: permissions.websearch,
        codesearch: permissions.codesearch,
        lsp: permissions.lsp,
        read,
        external_directory,
        doom_loop: permissions.doom_loop,
        rules: PermissionRuleSet {
            shell: shell_rules,
            edit: edit_rules,
            task: task_rules,
            read: read_rules,
            external_directory: external_directory_rules,
        },
    })
}
