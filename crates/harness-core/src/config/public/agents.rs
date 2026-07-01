use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::*;

const SUMMARY_AGENT_SYSTEM_PROMPT: &str = r#"Summarize what was done in this conversation. Write like a pull request description.

Rules:
- 2-3 sentences max
- Describe the changes made, not the process
- Do not mention running tests, builds, or other validation steps
- Do not explain what the user asked for
- Write in first person (I added..., I fixed...)
- Never ask questions or add new questions
- If the conversation ends with an unanswered question to the user, preserve that exact question
- If the conversation ends with an imperative statement or request to the user, always include that exact request in the summary"#;

const COMPACTION_AGENT_SYSTEM_PROMPT: &str = r#"You are an anchored context summarization assistant for coding sessions.

Summarize only the conversation history you are given. The newest turns may be kept verbatim outside your summary, so focus on the older context that still matters for continuing the work.

If the prompt includes a previous summary, treat it as the current anchored summary. Update it with the new history by preserving still-true details, removing stale details, and merging in new facts.

Always follow the exact output structure requested by the user prompt. Keep every section, preserve exact file paths and identifiers when known, and prefer terse bullets over paragraphs.

Do not answer the conversation itself. Do not mention that you are summarizing, compacting, or merging context. Respond in the same language as the conversation."#;

const CATEGORY_ROUTING_TOOLS: [&str; 20] = [
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
];

/// Named agent definitions. Built-in upstream-compatible agents are explicit so
/// editors can complete them, and custom names are accepted through the same
/// shape.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublicAgentMap {
    #[serde(default)]
    pub build: Option<PublicAgentConfig>,
    #[serde(default)]
    pub plan: Option<PublicAgentConfig>,
    #[serde(default)]
    pub general: Option<PublicAgentConfig>,
    #[serde(default)]
    pub explore: Option<PublicAgentConfig>,
    #[serde(default)]
    #[serde(rename = "visual-engineering", alias = "visualEngineering")]
    pub visual_engineering: Option<PublicAgentConfig>,
    #[serde(default)]
    pub artistry: Option<PublicAgentConfig>,
    #[serde(default)]
    pub ultrabrain: Option<PublicAgentConfig>,
    #[serde(default)]
    pub deep: Option<PublicAgentConfig>,
    #[serde(default)]
    pub quick: Option<PublicAgentConfig>,
    #[serde(default, rename = "unspecified-low", alias = "unspecifiedLow")]
    pub unspecified_low: Option<PublicAgentConfig>,
    #[serde(default, rename = "unspecified-high", alias = "unspecifiedHigh")]
    pub unspecified_high: Option<PublicAgentConfig>,
    #[serde(default)]
    pub writing: Option<PublicAgentConfig>,
    #[serde(default)]
    pub title: Option<PublicAgentConfig>,
    #[serde(default)]
    pub summary: Option<PublicAgentConfig>,
    #[serde(default)]
    pub compaction: Option<PublicAgentConfig>,
    #[serde(default, flatten)]
    pub custom: BTreeMap<String, PublicAgentConfig>,
}

impl PublicAgentMap {
    pub fn is_empty(&self) -> bool {
        self.build.is_none()
            && self.plan.is_none()
            && self.general.is_none()
            && self.explore.is_none()
            && self.visual_engineering.is_none()
            && self.artistry.is_none()
            && self.ultrabrain.is_none()
            && self.deep.is_none()
            && self.quick.is_none()
            && self.unspecified_low.is_none()
            && self.unspecified_high.is_none()
            && self.writing.is_none()
            && self.title.is_none()
            && self.summary.is_none()
            && self.compaction.is_none()
            && self.custom.is_empty()
    }

    pub(super) fn into_entries(self) -> BTreeMap<String, PublicAgentConfig> {
        let mut agents = self.custom;
        for (name, agent) in [
            ("build", self.build),
            ("plan", self.plan),
            ("general", self.general),
            ("explore", self.explore),
            ("visual-engineering", self.visual_engineering),
            ("artistry", self.artistry),
            ("ultrabrain", self.ultrabrain),
            ("deep", self.deep),
            ("quick", self.quick),
            ("unspecified-low", self.unspecified_low),
            ("unspecified-high", self.unspecified_high),
            ("writing", self.writing),
            ("title", self.title),
            ("summary", self.summary),
            ("compaction", self.compaction),
        ] {
            if let Some(agent) = agent {
                agents.insert(name.to_string(), agent);
            }
        }
        agents
    }
}

/// Agent override or custom agent definition.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublicAgentConfig {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, alias = "systemPrompt", alias = "prompt")]
    pub system_prompt: Option<String>,
    #[serde(default, alias = "model_ref", alias = "modelRef")]
    pub model: Option<String>,
    #[serde(default, alias = "smallModel")]
    pub use_small_model: bool,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default, alias = "topP")]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub mode: Option<AgentMode>,
    #[serde(default)]
    pub hidden: Option<bool>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub options: BTreeMap<String, serde_json::Value>,
    /// Set false to disable this agent. Set true to document that a shipped
    /// default remains active. `enabled` is accepted as an alias.
    #[serde(default, alias = "enabled")]
    pub enable: Option<bool>,
    /// Upstream-compatible negative toggle. Equivalent to `enable: false`.
    #[serde(default)]
    pub disable: bool,
    #[serde(default, alias = "permissions")]
    pub permission: Option<PublicProfilePermissions>,
    #[serde(default, alias = "maxIters", alias = "steps", alias = "maxSteps")]
    pub max_iters: Option<usize>,
    #[serde(default, alias = "toolFailureMode")]
    pub tool_failure_mode: ToolFailureMode,
    #[serde(default)]
    pub tools: PublicAgentTools,
    #[serde(default, flatten)]
    pub extra_options: BTreeMap<String, serde_json::Value>,
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

pub(super) fn default_shipped_agents(
    model_ref: &str,
    small_model_ref: Option<&str>,
) -> BTreeMap<String, ProfileConfig> {
    BTreeMap::from([
        (
            crate::plan::BUILD_AGENT_NAME.to_string(),
            ProfileConfig {
                name: None,
                description:
                    "Implementation lane: execute the requested work and verify the result."
                        .to_string(),
                system_prompt: None,
                model_ref: model_ref.to_string(),
                model_ref_explicit: false,
                variant: None,
                temperature: None,
                top_p: None,
                mode: AgentMode::Primary,
                hidden: false,
                color: None,
                options: BTreeMap::new(),
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
                max_iters: None,
                tool_failure_mode: ToolFailureMode::ContinueAsToolMessage,
                tools: vec![
                    "todowrite",
                    "todoread",
                    "question",
                    crate::plan::PLAN_ENTER_TOOL_ID,
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
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
                enabled: None,
            },
        ),
        (
            crate::plan::PLAN_AGENT_NAME.to_string(),
            ProfileConfig {
                name: None,
                description: "Plan mode. Disallows all edit tools except the active plan file."
                    .to_string(),
                system_prompt: None,
                model_ref: model_ref.to_string(),
                model_ref_explicit: false,
                variant: None,
                temperature: None,
                top_p: None,
                mode: AgentMode::Primary,
                hidden: false,
                color: None,
                options: BTreeMap::new(),
                permissions: Some(ProfilePermissions {
                    fallback: None,
                    edit: None,
                    shell: Some(PermissionMode::Ask),
                    network: Some(PermissionMode::Allow),
                    question: Some(PermissionMode::Allow),
                    task: Some(PermissionMode::Allow),
                    webfetch: Some(PermissionMode::Allow),
                    websearch: Some(PermissionMode::Allow),
                    codesearch: Some(PermissionMode::Allow),
                    lsp: Some(PermissionMode::Allow),
                    rules: PermissionRuleSet {
                        edit: vec![
                            PermissionSelectorRule {
                                selector: PermissionSelector::CatchAll,
                                mode: PermissionMode::Deny,
                            },
                            PermissionSelectorRule {
                                selector: PermissionSelector::Prefix(format!(
                                    "{}/",
                                    crate::plan::PLAN_DIR
                                )),
                                mode: PermissionMode::Allow,
                            },
                        ],
                        shell: Vec::new(),
                        task: Vec::new(),
                    },
                }),
                max_iters: None,
                tool_failure_mode: ToolFailureMode::ContinueAsToolMessage,
                tools: vec![
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
                    "bash",
                    crate::plan::PLAN_EXIT_TOOL_ID,
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
                enabled: None,
            },
        ),
        (
            "explore".to_string(),
            ProfileConfig {
                name: None,
                description:
                    "Read-only contextual codebase search agent for finding files, patterns, and conventions."
                        .to_string(),
                system_prompt: None,
                model_ref: model_ref.to_string(),
                model_ref_explicit: false,
                variant: None,
                temperature: None,
                top_p: None,
                mode: AgentMode::Subagent,
                hidden: false,
                color: None,
                options: BTreeMap::new(),
                permissions: Some(ProfilePermissions {
                    fallback: None,
                    edit: Some(PermissionMode::Deny),
                    shell: Some(PermissionMode::Deny),
                    network: Some(PermissionMode::Deny),
                    question: Some(PermissionMode::Allow),
                    task: Some(PermissionMode::Deny),
                    webfetch: Some(PermissionMode::Deny),
                    websearch: Some(PermissionMode::Deny),
                    codesearch: Some(PermissionMode::Deny),
                    lsp: Some(PermissionMode::Allow),
                    rules: PermissionRuleSet::default(),
                }),
                max_iters: None,
                tool_failure_mode: ToolFailureMode::ContinueAsToolMessage,
                tools: vec![
                    "question",
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
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
                enabled: None,
            },
        ),
        (
            "general".to_string(),
            ProfileConfig {
                name: None,
                description:
                    "General-purpose implementation and research subagent for focused multi-step work."
                        .to_string(),
                system_prompt: None,
                model_ref: model_ref.to_string(),
                model_ref_explicit: false,
                variant: None,
                temperature: None,
                top_p: None,
                mode: AgentMode::Subagent,
                hidden: false,
                color: None,
                options: BTreeMap::new(),
                permissions: Some(ProfilePermissions {
                    fallback: None,
                    edit: Some(PermissionMode::Allow),
                    shell: Some(PermissionMode::Allow),
                    network: Some(PermissionMode::Allow),
                    question: Some(PermissionMode::Allow),
                    task: Some(PermissionMode::Deny),
                    webfetch: Some(PermissionMode::Allow),
                    websearch: Some(PermissionMode::Allow),
                    codesearch: Some(PermissionMode::Allow),
                    lsp: Some(PermissionMode::Allow),
                    rules: PermissionRuleSet::default(),
                }),
                max_iters: None,
                tool_failure_mode: ToolFailureMode::ContinueAsToolMessage,
                tools: vec![
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
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
                enabled: None,
            },
        ),
        category_routing_profile(
            "visual-engineering",
            "Frontend, UI/UX, layout, styling, animation, and visual design subagent.",
            model_ref,
        ),
        category_routing_profile(
            "artistry",
            "Complex creative problem-solving subagent for ambiguous product or implementation work.",
            model_ref,
        ),
        category_routing_profile(
            "ultrabrain",
            "Hard logic, architecture, algorithms, and deep debugging subagent.",
            model_ref,
        ),
        category_routing_profile(
            "deep",
            "Autonomous research and end-to-end implementation subagent.",
            model_ref,
        ),
        category_routing_profile(
            "quick",
            "Small, low-risk implementation or cleanup subagent.",
            model_ref,
        ),
        category_routing_profile(
            "unspecified-low",
            "Low-to-moderate fallback subagent for contained uncategorized tasks.",
            model_ref,
        ),
        category_routing_profile(
            "unspecified-high",
            "High-effort fallback subagent for uncategorized complex tasks.",
            model_ref,
        ),
        category_routing_profile(
            "writing",
            "Documentation, prose, technical writing, and editing subagent.",
            model_ref,
        ),
        (
            crate::session_title::TITLE_AGENT_NAME.to_string(),
            ProfileConfig {
                name: None,
                description: "Hidden title generation agent.".to_string(),
                system_prompt: Some(crate::session_title::TITLE_AGENT_SYSTEM_PROMPT.to_string()),
                model_ref: small_model_ref.unwrap_or(model_ref).to_string(),
                model_ref_explicit: small_model_ref.is_some(),
                variant: None,
                temperature: Some(crate::session_title::TITLE_AGENT_TEMPERATURE),
                top_p: None,
                mode: AgentMode::Primary,
                hidden: true,
                color: None,
                options: BTreeMap::new(),
                permissions: Some(ProfilePermissions {
                    fallback: Some(PermissionMode::Deny),
                    edit: Some(PermissionMode::Deny),
                    shell: Some(PermissionMode::Deny),
                    network: Some(PermissionMode::Deny),
                    question: Some(PermissionMode::Deny),
                    task: Some(PermissionMode::Deny),
                    webfetch: Some(PermissionMode::Deny),
                    websearch: Some(PermissionMode::Deny),
                    codesearch: Some(PermissionMode::Deny),
                    lsp: Some(PermissionMode::Deny),
                    rules: PermissionRuleSet::default(),
                }),
                max_iters: None,
                tool_failure_mode: ToolFailureMode::FailTurn,
                tools: Vec::new(),
                enabled: None,
            },
        ),
        (
            "summary".to_string(),
            ProfileConfig {
                name: None,
                description: "Hidden session summary agent.".to_string(),
                system_prompt: Some(SUMMARY_AGENT_SYSTEM_PROMPT.to_string()),
                model_ref: small_model_ref.unwrap_or(model_ref).to_string(),
                model_ref_explicit: small_model_ref.is_some(),
                variant: None,
                temperature: None,
                top_p: None,
                mode: AgentMode::Primary,
                hidden: true,
                color: None,
                options: BTreeMap::new(),
                permissions: Some(ProfilePermissions {
                    fallback: Some(PermissionMode::Deny),
                    edit: Some(PermissionMode::Deny),
                    shell: Some(PermissionMode::Deny),
                    network: Some(PermissionMode::Deny),
                    question: Some(PermissionMode::Deny),
                    task: Some(PermissionMode::Deny),
                    webfetch: Some(PermissionMode::Deny),
                    websearch: Some(PermissionMode::Deny),
                    codesearch: Some(PermissionMode::Deny),
                    lsp: Some(PermissionMode::Deny),
                    rules: PermissionRuleSet::default(),
                }),
                max_iters: None,
                tool_failure_mode: ToolFailureMode::FailTurn,
                tools: Vec::new(),
                enabled: None,
            },
        ),
        (
            "compaction".to_string(),
            ProfileConfig {
                name: None,
                description: "Hidden provider-context compaction agent.".to_string(),
                system_prompt: Some(COMPACTION_AGENT_SYSTEM_PROMPT.to_string()),
                model_ref: model_ref.to_string(),
                model_ref_explicit: false,
                variant: None,
                temperature: None,
                top_p: None,
                mode: AgentMode::Primary,
                hidden: true,
                color: None,
                options: BTreeMap::new(),
                permissions: Some(ProfilePermissions {
                    fallback: Some(PermissionMode::Deny),
                    edit: Some(PermissionMode::Deny),
                    shell: Some(PermissionMode::Deny),
                    network: Some(PermissionMode::Deny),
                    question: Some(PermissionMode::Deny),
                    task: Some(PermissionMode::Deny),
                    webfetch: Some(PermissionMode::Deny),
                    websearch: Some(PermissionMode::Deny),
                    codesearch: Some(PermissionMode::Deny),
                    lsp: Some(PermissionMode::Deny),
                    rules: PermissionRuleSet::default(),
                }),
                max_iters: None,
                tool_failure_mode: ToolFailureMode::FailTurn,
                tools: Vec::new(),
                enabled: None,
            },
        ),
    ])
}

fn category_routing_profile(
    name: &str,
    description: &str,
    model_ref: &str,
) -> (String, ProfileConfig) {
    (
        name.to_string(),
        ProfileConfig {
            name: None,
            description: description.to_string(),
            system_prompt: None,
            model_ref: model_ref.to_string(),
            model_ref_explicit: false,
            variant: None,
            temperature: None,
            top_p: None,
            mode: AgentMode::Subagent,
            hidden: false,
            color: None,
            options: BTreeMap::new(),
            permissions: Some(ProfilePermissions {
                fallback: None,
                edit: Some(PermissionMode::Allow),
                shell: Some(PermissionMode::Allow),
                network: Some(PermissionMode::Allow),
                question: Some(PermissionMode::Allow),
                task: Some(PermissionMode::Deny),
                webfetch: Some(PermissionMode::Allow),
                websearch: Some(PermissionMode::Allow),
                codesearch: Some(PermissionMode::Allow),
                lsp: Some(PermissionMode::Allow),
                rules: PermissionRuleSet::default(),
            }),
            max_iters: None,
            tool_failure_mode: ToolFailureMode::ContinueAsToolMessage,
            tools: CATEGORY_ROUTING_TOOLS
                .into_iter()
                .map(str::to_string)
                .collect(),
            enabled: None,
        },
    )
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

pub(super) fn public_agent_to_profile(
    name: &str,
    agent: PublicAgentConfig,
    default_model_ref: Option<&str>,
    small_model_ref: Option<&str>,
    base: Option<ProfileConfig>,
) -> Result<ProfileConfig, ConfigError> {
    let model_ref_explicit = agent.model.is_some()
        || agent.use_small_model
        || base
            .as_ref()
            .map(|profile| profile.model_ref_explicit)
            .unwrap_or(false);
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
    let configured_tools = agent.tools.tool_ids();
    let mut configured_options = agent.options;
    configured_options.extend(agent.extra_options);
    let options = if configured_options.is_empty() {
        base.as_ref()
            .map(|profile| profile.options.clone())
            .unwrap_or_default()
    } else {
        let mut options = base
            .as_ref()
            .map(|profile| profile.options.clone())
            .unwrap_or_default();
        options.extend(configured_options);
        options
    };

    Ok(ProfileConfig {
        name: agent
            .name
            .or_else(|| base.as_ref().and_then(|profile| profile.name.clone())),
        description,
        system_prompt: agent.system_prompt.or_else(|| {
            base.as_ref()
                .and_then(|profile| profile.system_prompt.clone())
        }),
        model_ref,
        model_ref_explicit,
        variant: agent
            .variant
            .or_else(|| base.as_ref().and_then(|profile| profile.variant.clone())),
        temperature: agent
            .temperature
            .or_else(|| base.as_ref().and_then(|profile| profile.temperature)),
        top_p: agent
            .top_p
            .or_else(|| base.as_ref().and_then(|profile| profile.top_p)),
        mode: agent
            .mode
            .or_else(|| base.as_ref().map(|profile| profile.mode))
            .unwrap_or_default(),
        hidden: agent
            .hidden
            .or_else(|| base.as_ref().map(|profile| profile.hidden))
            .unwrap_or(false),
        color: agent
            .color
            .or_else(|| base.as_ref().and_then(|profile| profile.color.clone())),
        options,
        permissions: agent
            .permission
            .map(translate_public_profile_permissions)
            .transpose()?
            .or_else(|| {
                base.as_ref()
                    .and_then(|profile| profile.permissions.clone())
            }),
        max_iters: agent
            .max_iters
            .or_else(|| base.as_ref().and_then(|profile| profile.max_iters)),
        tool_failure_mode: if matches!(agent.tool_failure_mode, ToolFailureMode::FailTurn) {
            base.as_ref()
                .map(|profile| profile.tool_failure_mode)
                .unwrap_or(agent.tool_failure_mode)
        } else {
            agent.tool_failure_mode
        },
        tools: if configured_tools.is_empty() {
            base.as_ref()
                .map(|profile| profile.tools.clone())
                .unwrap_or_default()
        } else {
            configured_tools
        },
        enabled: None,
    })
}

fn translate_public_profile_permissions(
    permissions: PublicProfilePermissions,
) -> Result<ProfilePermissions, ConfigError> {
    let edit = public_rule_mode(&permissions.edit);
    let shell = public_rule_mode(&permissions.bash);
    let task = public_rule_mode(&permissions.task);
    let edit_rules = public_selector_rules("edit", permissions.edit)?;
    let shell_rules = public_selector_rules("bash", permissions.bash)?;
    let task_rules = public_selector_rules("task", permissions.task)?;

    Ok(ProfilePermissions {
        fallback: permissions.fallback,
        edit,
        shell,
        network: permissions.network,
        question: permissions.question,
        task,
        webfetch: permissions.webfetch,
        websearch: permissions.websearch,
        codesearch: permissions.codesearch,
        lsp: permissions.lsp,
        rules: PermissionRuleSet {
            shell: shell_rules,
            edit: edit_rules,
            task: task_rules,
        },
    })
}
