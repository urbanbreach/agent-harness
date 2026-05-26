use schemars::schema_for;

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicConfigSurface {
    Runtime,
    Tui,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicConfigKeyStatus {
    Canonical,
    Compatibility,
    InertCompatibility,
    UnsupportedActive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicUnsupportedInactiveValue {
    BoolFalse,
    DisabledString,
    EmptyObject,
    EmptyArray,
}

impl PublicUnsupportedInactiveValue {
    fn matches(self, value: &serde_json::Value) -> bool {
        match self {
            Self::BoolFalse => matches!(value, serde_json::Value::Bool(false)),
            Self::DisabledString => {
                matches!(value, serde_json::Value::String(mode) if mode == "disabled")
            }
            Self::EmptyObject => value.as_object().is_some_and(|object| object.is_empty()),
            Self::EmptyArray => value.as_array().is_some_and(|items| items.is_empty()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicConfigTopLevelKey {
    pub name: &'static str,
    pub surface: PublicConfigSurface,
    pub status: PublicConfigKeyStatus,
    pub schema_property: bool,
    pub docs_table_row: bool,
    pub canonical_name: Option<&'static str>,
    pub inactive_value: Option<PublicUnsupportedInactiveValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicConfigAliasScope {
    RuntimeRoot,
    RuntimeBackgroundTasks,
    RuntimePermissions,
    RuntimePrompt,
    RuntimeCompaction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicConfigAlias {
    pub scope: PublicConfigAliasScope,
    pub alias: &'static str,
    pub canonical: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicConfigPermissionName {
    pub name: &'static str,
    pub canonical_name: &'static str,
    pub canonical: bool,
    pub schema_property: bool,
    pub supports_selectors: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicConfigCompactionKnob {
    pub canonical_name: &'static str,
    pub aliases: &'static [&'static str],
    pub default_value: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct PublicConfigContract {
    pub runtime_top_level_keys: &'static [PublicConfigTopLevelKey],
    pub tui_top_level_keys: &'static [PublicConfigTopLevelKey],
    pub runtime_aliases: &'static [PublicConfigAlias],
    pub permission_names: &'static [PublicConfigPermissionName],
    pub compaction_knobs: &'static [PublicConfigCompactionKnob],
}

impl PublicConfigContract {
    fn runtime_top_level_key(&self, name: &str) -> Option<&PublicConfigTopLevelKey> {
        self.runtime_top_level_keys
            .iter()
            .find(|entry| entry.name == name)
    }

    pub fn runtime_schema_top_level_keys(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.runtime_top_level_keys
            .iter()
            .filter(|entry| entry.schema_property)
            .map(|entry| entry.name)
    }

    pub fn runtime_documented_top_level_keys(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.runtime_top_level_keys
            .iter()
            .filter(|entry| entry.docs_table_row)
            .map(|entry| entry.name)
    }

    pub fn tui_schema_top_level_keys(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.tui_top_level_keys
            .iter()
            .filter(|entry| entry.schema_property)
            .map(|entry| entry.name)
    }

    pub fn tui_documented_top_level_keys(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.tui_top_level_keys
            .iter()
            .filter(|entry| entry.docs_table_row)
            .map(|entry| entry.name)
    }

    fn runtime_aliases_for_scope(
        &self,
        scope: PublicConfigAliasScope,
    ) -> impl Iterator<Item = &PublicConfigAlias> {
        self.runtime_aliases
            .iter()
            .filter(move |alias| alias.scope == scope)
    }
}

macro_rules! runtime_key {
    ($name:literal, $status:ident, schema, docs) => {
        PublicConfigTopLevelKey {
            name: $name,
            surface: PublicConfigSurface::Runtime,
            status: PublicConfigKeyStatus::$status,
            schema_property: true,
            docs_table_row: true,
            canonical_name: None,
            inactive_value: None,
        }
    };
    ($name:literal, $status:ident, schema, docs, canonical = $canonical:literal) => {
        PublicConfigTopLevelKey {
            name: $name,
            surface: PublicConfigSurface::Runtime,
            status: PublicConfigKeyStatus::$status,
            schema_property: true,
            docs_table_row: true,
            canonical_name: Some($canonical),
            inactive_value: None,
        }
    };
    ($name:literal, UnsupportedActive, schema, docs, inactive = $inactive:ident) => {
        PublicConfigTopLevelKey {
            name: $name,
            surface: PublicConfigSurface::Runtime,
            status: PublicConfigKeyStatus::UnsupportedActive,
            schema_property: true,
            docs_table_row: true,
            canonical_name: None,
            inactive_value: Some(PublicUnsupportedInactiveValue::$inactive),
        }
    };
    ($name:literal, $status:ident, no_schema, no_docs, canonical = $canonical:literal) => {
        PublicConfigTopLevelKey {
            name: $name,
            surface: PublicConfigSurface::Runtime,
            status: PublicConfigKeyStatus::$status,
            schema_property: false,
            docs_table_row: false,
            canonical_name: Some($canonical),
            inactive_value: None,
        }
    };
    ($name:literal, $status:ident, no_schema, no_docs) => {
        PublicConfigTopLevelKey {
            name: $name,
            surface: PublicConfigSurface::Runtime,
            status: PublicConfigKeyStatus::$status,
            schema_property: false,
            docs_table_row: false,
            canonical_name: None,
            inactive_value: None,
        }
    };
}

const PUBLIC_RUNTIME_TOP_LEVEL_CONFIG_KEYS: &[PublicConfigTopLevelKey] = &[
    runtime_key!("$schema", Canonical, schema, docs),
    runtime_key!("agent", Canonical, schema, docs),
    runtime_key!(
        "autoshare",
        UnsupportedActive,
        schema,
        docs,
        inactive = BoolFalse
    ),
    runtime_key!(
        "autoupdate",
        UnsupportedActive,
        schema,
        docs,
        inactive = BoolFalse
    ),
    runtime_key!(
        "command",
        UnsupportedActive,
        schema,
        docs,
        inactive = EmptyObject
    ),
    runtime_key!("compaction", InertCompatibility, schema, docs),
    runtime_key!("default_agent", Canonical, schema, docs),
    runtime_key!("disabled_providers", InertCompatibility, schema, docs),
    runtime_key!("enabled_providers", InertCompatibility, schema, docs),
    runtime_key!(
        "enterprise",
        UnsupportedActive,
        schema,
        docs,
        inactive = EmptyObject
    ),
    runtime_key!("experimental", InertCompatibility, schema, docs),
    runtime_key!("formatter", InertCompatibility, schema, docs),
    runtime_key!("instructions", Canonical, schema, docs),
    runtime_key!("layout", InertCompatibility, schema, docs),
    runtime_key!("logLevel", InertCompatibility, schema, docs),
    runtime_key!("lsp", Compatibility, schema, docs),
    runtime_key!("mcp", Canonical, schema, docs),
    runtime_key!("mode", Compatibility, schema, docs, canonical = "agent"),
    runtime_key!("model", Canonical, schema, docs),
    runtime_key!("model_profile", Canonical, schema, docs),
    runtime_key!("permission", Canonical, schema, docs),
    runtime_key!(
        "plugin",
        UnsupportedActive,
        schema,
        docs,
        inactive = EmptyArray
    ),
    runtime_key!("provider", Canonical, schema, docs),
    runtime_key!("runtime", Canonical, schema, docs),
    runtime_key!(
        "server",
        UnsupportedActive,
        schema,
        docs,
        inactive = EmptyObject
    ),
    runtime_key!(
        "share",
        UnsupportedActive,
        schema,
        docs,
        inactive = DisabledString
    ),
    runtime_key!("shell", InertCompatibility, schema, docs),
    runtime_key!("skills", Canonical, schema, docs),
    runtime_key!("small_model", Canonical, schema, docs),
    runtime_key!("snapshot", InertCompatibility, schema, docs),
    runtime_key!("tool_output", InertCompatibility, schema, docs),
    runtime_key!("tools", InertCompatibility, schema, docs),
    runtime_key!("username", InertCompatibility, schema, docs),
    runtime_key!("watcher", InertCompatibility, schema, docs),
    runtime_key!(
        "providers",
        Compatibility,
        no_schema,
        no_docs,
        canonical = "provider"
    ),
    runtime_key!(
        "smallModel",
        Compatibility,
        no_schema,
        no_docs,
        canonical = "small_model"
    ),
    runtime_key!(
        "modelProfile",
        Compatibility,
        no_schema,
        no_docs,
        canonical = "model_profile"
    ),
    runtime_key!(
        "model_profiles",
        Compatibility,
        no_schema,
        no_docs,
        canonical = "model_profile"
    ),
    runtime_key!(
        "agents",
        Compatibility,
        no_schema,
        no_docs,
        canonical = "agent"
    ),
    runtime_key!(
        "categories",
        Compatibility,
        no_schema,
        no_docs,
        canonical = "agent"
    ),
    runtime_key!(
        "profiles",
        Compatibility,
        no_schema,
        no_docs,
        canonical = "agent"
    ),
    runtime_key!(
        "defaultAgent",
        Compatibility,
        no_schema,
        no_docs,
        canonical = "default_agent"
    ),
    runtime_key!(
        "permissions",
        Compatibility,
        no_schema,
        no_docs,
        canonical = "permission"
    ),
    runtime_key!(
        "backgroundTask",
        Compatibility,
        no_schema,
        no_docs,
        canonical = "runtime.background_tasks"
    ),
    runtime_key!(
        "paths",
        Compatibility,
        no_schema,
        no_docs,
        canonical = "runtime.session_dir"
    ),
    runtime_key!(
        "deterministic",
        Compatibility,
        no_schema,
        no_docs,
        canonical = "runtime.deterministic"
    ),
    runtime_key!("integrations", Compatibility, no_schema, no_docs),
    runtime_key!("hooks", Compatibility, no_schema, no_docs),
    runtime_key!("logging", Compatibility, no_schema, no_docs),
    runtime_key!("ui", Compatibility, no_schema, no_docs),
    runtime_key!("hashline_edit", Compatibility, no_schema, no_docs),
    runtime_key!(
        "hashlineEdit",
        Compatibility,
        no_schema,
        no_docs,
        canonical = "hashline_edit"
    ),
];

const PUBLIC_TUI_TOP_LEVEL_CONFIG_KEYS: &[PublicConfigTopLevelKey] = &[
    PublicConfigTopLevelKey {
        name: "$schema",
        surface: PublicConfigSurface::Tui,
        status: PublicConfigKeyStatus::Canonical,
        schema_property: true,
        docs_table_row: true,
        canonical_name: None,
        inactive_value: None,
    },
    PublicConfigTopLevelKey {
        name: "keybinds",
        surface: PublicConfigSurface::Tui,
        status: PublicConfigKeyStatus::Canonical,
        schema_property: true,
        docs_table_row: true,
        canonical_name: None,
        inactive_value: None,
    },
    PublicConfigTopLevelKey {
        name: "keybindings",
        surface: PublicConfigSurface::Tui,
        status: PublicConfigKeyStatus::Compatibility,
        schema_property: false,
        docs_table_row: false,
        canonical_name: Some("keybinds"),
        inactive_value: None,
    },
];

const PUBLIC_RUNTIME_ALIASES: &[PublicConfigAlias] = &[
    PublicConfigAlias {
        scope: PublicConfigAliasScope::RuntimeRoot,
        alias: "backgroundTasks",
        canonical: "background_tasks",
    },
    PublicConfigAlias {
        scope: PublicConfigAliasScope::RuntimeRoot,
        alias: "sessionDir",
        canonical: "session_dir",
    },
    PublicConfigAlias {
        scope: PublicConfigAliasScope::RuntimeBackgroundTasks,
        alias: "defaultConcurrency",
        canonical: "default_concurrency",
    },
    PublicConfigAlias {
        scope: PublicConfigAliasScope::RuntimeBackgroundTasks,
        alias: "providerConcurrency",
        canonical: "provider_concurrency",
    },
    PublicConfigAlias {
        scope: PublicConfigAliasScope::RuntimeBackgroundTasks,
        alias: "modelConcurrency",
        canonical: "model_concurrency",
    },
    PublicConfigAlias {
        scope: PublicConfigAliasScope::RuntimeBackgroundTasks,
        alias: "staleTimeoutMs",
        canonical: "stale_timeout_ms",
    },
    PublicConfigAlias {
        scope: PublicConfigAliasScope::RuntimeBackgroundTasks,
        alias: "messageStalenessTimeoutMs",
        canonical: "message_staleness_timeout_ms",
    },
    PublicConfigAlias {
        scope: PublicConfigAliasScope::RuntimePermissions,
        alias: "askTimeoutMs",
        canonical: "ask_timeout_ms",
    },
    PublicConfigAlias {
        scope: PublicConfigAliasScope::RuntimePrompt,
        alias: "waitTimeoutMs",
        canonical: "wait_timeout_ms",
    },
    PublicConfigAlias {
        scope: PublicConfigAliasScope::RuntimeCompaction,
        alias: "modelBacked",
        canonical: "model_backed",
    },
    PublicConfigAlias {
        scope: PublicConfigAliasScope::RuntimeCompaction,
        alias: "modelRef",
        canonical: "model_ref",
    },
    PublicConfigAlias {
        scope: PublicConfigAliasScope::RuntimeCompaction,
        alias: "model",
        canonical: "model_ref",
    },
    PublicConfigAlias {
        scope: PublicConfigAliasScope::RuntimeCompaction,
        alias: "splitOversizedTurns",
        canonical: "split_oversized_turns",
    },
    PublicConfigAlias {
        scope: PublicConfigAliasScope::RuntimeCompaction,
        alias: "autoRetryOverflow",
        canonical: "auto_retry_overflow",
    },
    PublicConfigAlias {
        scope: PublicConfigAliasScope::RuntimeCompaction,
        alias: "structuredSummaryContract",
        canonical: "structured_summary_contract",
    },
    PublicConfigAlias {
        scope: PublicConfigAliasScope::RuntimeCompaction,
        alias: "estimatedTokenTriggers",
        canonical: "estimated_token_triggers",
    },
    PublicConfigAlias {
        scope: PublicConfigAliasScope::RuntimeCompaction,
        alias: "fallbackInputTokens",
        canonical: "fallback_input_tokens",
    },
];

const PUBLIC_PERMISSION_NAMES: &[PublicConfigPermissionName] = &[
    PublicConfigPermissionName {
        name: "bash",
        canonical_name: "bash",
        canonical: true,
        schema_property: true,
        supports_selectors: true,
    },
    PublicConfigPermissionName {
        name: "edit",
        canonical_name: "edit",
        canonical: true,
        schema_property: true,
        supports_selectors: true,
    },
    PublicConfigPermissionName {
        name: "question",
        canonical_name: "question",
        canonical: true,
        schema_property: true,
        supports_selectors: false,
    },
    PublicConfigPermissionName {
        name: "task",
        canonical_name: "task",
        canonical: true,
        schema_property: true,
        supports_selectors: true,
    },
    PublicConfigPermissionName {
        name: "webfetch",
        canonical_name: "webfetch",
        canonical: true,
        schema_property: true,
        supports_selectors: false,
    },
    PublicConfigPermissionName {
        name: "websearch",
        canonical_name: "websearch",
        canonical: true,
        schema_property: true,
        supports_selectors: false,
    },
    PublicConfigPermissionName {
        name: "codesearch",
        canonical_name: "codesearch",
        canonical: true,
        schema_property: true,
        supports_selectors: false,
    },
    PublicConfigPermissionName {
        name: "lsp",
        canonical_name: "lsp",
        canonical: true,
        schema_property: true,
        supports_selectors: false,
    },
    PublicConfigPermissionName {
        name: "shell",
        canonical_name: "bash",
        canonical: false,
        schema_property: false,
        supports_selectors: true,
    },
    PublicConfigPermissionName {
        name: "network",
        canonical_name: "network",
        canonical: false,
        schema_property: false,
        supports_selectors: false,
    },
];

const PUBLIC_COMPACTION_KNOBS: &[PublicConfigCompactionKnob] = &[
    PublicConfigCompactionKnob {
        canonical_name: "model_backed",
        aliases: &["modelBacked"],
        default_value: "false",
    },
    PublicConfigCompactionKnob {
        canonical_name: "model_ref",
        aliases: &["model", "modelRef"],
        default_value: "unset",
    },
    PublicConfigCompactionKnob {
        canonical_name: "split_oversized_turns",
        aliases: &["splitOversizedTurns"],
        default_value: "false",
    },
    PublicConfigCompactionKnob {
        canonical_name: "auto_retry_overflow",
        aliases: &["autoRetryOverflow"],
        default_value: "true",
    },
    PublicConfigCompactionKnob {
        canonical_name: "structured_summary_contract",
        aliases: &["structuredSummaryContract"],
        default_value: "true",
    },
    PublicConfigCompactionKnob {
        canonical_name: "estimated_token_triggers",
        aliases: &["estimatedTokenTriggers"],
        default_value: "true",
    },
    PublicConfigCompactionKnob {
        canonical_name: "fallback_input_tokens",
        aliases: &["fallbackInputTokens"],
        default_value: "32768",
    },
];

pub fn public_config_contract() -> PublicConfigContract {
    PublicConfigContract {
        runtime_top_level_keys: PUBLIC_RUNTIME_TOP_LEVEL_CONFIG_KEYS,
        tui_top_level_keys: PUBLIC_TUI_TOP_LEVEL_CONFIG_KEYS,
        runtime_aliases: PUBLIC_RUNTIME_ALIASES,
        permission_names: PUBLIC_PERMISSION_NAMES,
        compaction_knobs: PUBLIC_COMPACTION_KNOBS,
    }
}

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

const CATEGORY_ROUTING_TOOLS: [&str; 13] = [
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
    pub shell: Option<String>,
    #[serde(default, rename = "logLevel")]
    pub log_level: Option<String>,
    #[serde(default)]
    pub server: Option<serde_json::Value>,
    #[serde(default)]
    pub command: Option<serde_json::Value>,
    #[serde(default)]
    pub watcher: Option<serde_json::Value>,
    #[serde(default)]
    pub snapshot: Option<bool>,
    #[serde(default)]
    pub plugin: Option<serde_json::Value>,
    #[serde(default)]
    pub share: Option<serde_json::Value>,
    #[serde(default)]
    pub autoshare: Option<bool>,
    #[serde(default)]
    pub autoupdate: Option<serde_json::Value>,
    #[serde(default)]
    pub disabled_providers: Option<Vec<String>>,
    #[serde(default)]
    pub enabled_providers: Option<Vec<String>>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub formatter: Option<serde_json::Value>,
    #[serde(default)]
    pub lsp: Option<serde_json::Value>,
    #[serde(default)]
    pub layout: Option<String>,
    #[serde(default)]
    pub tools: Option<BTreeMap<String, bool>>,
    #[serde(default)]
    pub enterprise: Option<serde_json::Value>,
    #[serde(default)]
    pub tool_output: Option<serde_json::Value>,
    #[serde(default)]
    pub compaction: Option<serde_json::Value>,
    #[serde(default)]
    pub experimental: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicRuntimeSettingsConfig {
    #[serde(default)]
    pub compaction: CompactionRuntimeConfig,
}

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
    pub discipline: Option<PublicAgentConfig>,
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
            && self.discipline.is_none()
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

    fn into_entries(self) -> BTreeMap<String, PublicAgentConfig> {
        let mut agents = self.custom;
        for (name, agent) in [
            ("build", self.build),
            ("plan", self.plan),
            ("discipline", self.discipline),
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
    fn tool_ids(self) -> Vec<String> {
        match self {
            Self::List(tools) => tools,
            Self::Map(tools) => tools
                .into_iter()
                .filter_map(|(tool, enabled)| enabled.then_some(tool))
                .collect(),
        }
    }
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
    object: &serde_json::Map<String, serde_json::Value>,
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
                "Open",
                "Code",
                " config keys: {}; this harness accepts the ",
                "Open",
                "Code",
                " config shape, but does not execute server, command, plugin, sharing, update, or enterprise product features"
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
    let contract = public_config_contract();

    canonicalize_object_aliases(
        runtime_object,
        &contract
            .runtime_aliases_for_scope(PublicConfigAliasScope::RuntimeRoot)
            .map(|alias| (alias.alias, alias.canonical))
            .collect::<Vec<_>>(),
    );

    if let Some(background_tasks) = runtime_object
        .get_mut("background_tasks")
        .and_then(serde_json::Value::as_object_mut)
    {
        canonicalize_object_aliases(
            background_tasks,
            &contract
                .runtime_aliases_for_scope(PublicConfigAliasScope::RuntimeBackgroundTasks)
                .map(|alias| (alias.alias, alias.canonical))
                .collect::<Vec<_>>(),
        );
    }

    if let Some(permissions) = runtime_object
        .get_mut("permissions")
        .and_then(serde_json::Value::as_object_mut)
    {
        canonicalize_object_aliases(
            permissions,
            &contract
                .runtime_aliases_for_scope(PublicConfigAliasScope::RuntimePermissions)
                .map(|alias| (alias.alias, alias.canonical))
                .collect::<Vec<_>>(),
        );
    }

    if let Some(prompt) = runtime_object
        .get_mut("prompt")
        .and_then(serde_json::Value::as_object_mut)
    {
        canonicalize_object_aliases(
            prompt,
            &contract
                .runtime_aliases_for_scope(PublicConfigAliasScope::RuntimePrompt)
                .map(|alias| (alias.alias, alias.canonical))
                .collect::<Vec<_>>(),
        );
    }

    if let Some(compaction) = runtime_object
        .get_mut("compaction")
        .and_then(serde_json::Value::as_object_mut)
    {
        canonicalize_object_aliases(
            compaction,
            &contract
                .runtime_aliases_for_scope(PublicConfigAliasScope::RuntimeCompaction)
                .map(|alias| (alias.alias, alias.canonical))
                .collect::<Vec<_>>(),
        );
    }
}

fn default_shipped_agents(
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
                    "team_create",
                    "team_status",
                    "team_send_message",
                    "team_task_create",
                    "team_task_list",
                    "team_task_get",
                    "team_task_update",
                    "team_shutdown_request",
                    "team_shutdown_approve",
                    "team_shutdown_reject",
                    "team_delete",
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
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
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
                    crate::plan::PLAN_EXIT_TOOL_ID,
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
            },
        ),
        (
            "discipline".to_string(),
            ProfileConfig {
                name: None,
                description:
                    "Disciplined autonomous delivery lane with strict todo, delegation, and verification behavior."
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
                    "team_create",
                    "team_status",
                    "team_send_message",
                    "team_task_create",
                    "team_task_list",
                    "team_task_get",
                    "team_task_update",
                    "team_shutdown_request",
                    "team_shutdown_approve",
                    "team_shutdown_reject",
                    "team_delete",
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
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
            },
        ),
        (
            "explore".to_string(),
            ProfileConfig {
                name: None,
                description:
                    "Read-only contextual codebase search agent for finding files, patterns, and conventions."
                        .to_string(),
                system_prompt: Some(
                    "You are a read-only exploration subagent. Search the local codebase, inspect relevant files, and return concise findings with file paths and rationale. Do not edit files, run shell commands, or delegate to other agents."
                        .to_string(),
                ),
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
                tools: vec!["question", "lsp", "read", "glob", "grep", "list", "batch"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            },
        ),
        (
            "general".to_string(),
            ProfileConfig {
                name: None,
                description:
                    "General-purpose implementation and research subagent for focused multi-step work."
                        .to_string(),
                system_prompt: Some(
                    "You are a focused general-purpose subagent. Complete the delegated task using the tools available to this profile, report what you changed or learned, and include verification evidence when applicable. Do not spawn further subagents unless this profile is explicitly configured with the task tool."
                        .to_string(),
                ),
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
                    "lsp",
                    "read",
                    "glob",
                    "grep",
                    "list",
                    "edit",
                    "bash",
                    "batch",
                ]
                .into_iter()
                .map(str::to_string)
                .collect(),
            },
        ),
        category_routing_profile(
            "visual-engineering",
            "Frontend, UI/UX, layout, styling, animation, and visual design subagent.",
            "You are the visual-engineering category subagent. Focus on frontend, UI/UX, layout, styling, animation, and design work. Preserve existing product semantics, verify through the rendered or CLI-visible surface when applicable, and report concise evidence.",
            model_ref,
        ),
        category_routing_profile(
            "artistry",
            "Complex creative problem-solving subagent for ambiguous product or implementation work.",
            "You are the artistry category subagent. Solve complex creative implementation problems with clear tradeoffs, minimal abstractions, and observable verification evidence.",
            model_ref,
        ),
        category_routing_profile(
            "ultrabrain",
            "Hard logic, architecture, algorithms, and deep debugging subagent.",
            "You are the ultrabrain category subagent. Handle genuinely hard logic, architecture, algorithmic, or debugging tasks. Prefer root-cause fixes, state assumptions explicitly, and verify the behavioral boundary.",
            model_ref,
        ),
        category_routing_profile(
            "deep",
            "Autonomous research and end-to-end implementation subagent.",
            "You are the deep category subagent. Work autonomously on multi-step implementation or research tasks, keep scope focused, and return the completed outcome with verification evidence.",
            model_ref,
        ),
        category_routing_profile(
            "quick",
            "Small, low-risk implementation or cleanup subagent.",
            "You are the quick category subagent. Complete small, low-risk tasks with the smallest correct change and only the verification needed for confidence.",
            model_ref,
        ),
        category_routing_profile(
            "unspecified-low",
            "Low-effort fallback subagent for uncategorized small tasks.",
            "You are the unspecified-low category subagent. Handle uncategorized low-effort tasks directly, avoid broad refactors, and report the concise result.",
            model_ref,
        ),
        category_routing_profile(
            "unspecified-high",
            "High-effort fallback subagent for uncategorized complex tasks.",
            "You are the unspecified-high category subagent. Handle uncategorized high-effort tasks thoroughly, inspect enough context before acting, and provide verification evidence.",
            model_ref,
        ),
        category_routing_profile(
            "writing",
            "Documentation, prose, technical writing, and editing subagent.",
            "You are the writing category subagent. Produce clear documentation, prose, or technical writing that matches the repository voice and keeps examples aligned with behavior.",
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
            },
        ),
    ])
}

fn category_routing_profile(
    name: &str,
    description: &str,
    system_prompt: &str,
    model_ref: &str,
) -> (String, ProfileConfig) {
    (
        name.to_string(),
        ProfileConfig {
            name: None,
            description: description.to_string(),
            system_prompt: Some(system_prompt.to_string()),
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

fn public_agent_to_profile(
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
    let task = public_rule_mode(&parsed.task)
        .or_else(|| global.clone())
        .or(fallback.defaults.task);
    let edit_rules = public_selector_rules("edit", parsed.edit)?;
    let shell_rules = public_selector_rules("bash", parsed.bash)?;
    let task_rules = public_selector_rules("task", parsed.task)?;

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
            task,
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
            task: task_rules,
        },
        shell_allowlist: parsed.shell_allowlist.unwrap_or(fallback.shell_allowlist),
    })
    .map_err(|err| ConfigError::ParseJson5(err.to_string()))
}

fn normalize_public_mcp_servers(value: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Object(servers) = value else {
        return value;
    };

    let mut normalized_servers = serde_json::Map::new();
    for (name, server) in servers {
        let mut normalized = server;
        let Some(server_object) = normalized.as_object_mut() else {
            normalized_servers.insert(name, normalized);
            continue;
        };

        if server_object.len() == 1
            && matches!(
                server_object.get("enabled"),
                Some(serde_json::Value::Bool(false))
            )
        {
            continue;
        }

        if !server_object.contains_key("transport") {
            if let Some(kind) = server_object.remove("type") {
                let transport = match kind.as_str() {
                    Some("local") => "stdio",
                    Some("remote") => "http",
                    Some(other) => other,
                    None => "",
                };
                if !transport.is_empty() {
                    server_object.insert(
                        "transport".to_string(),
                        serde_json::Value::String(transport.to_string()),
                    );
                }
            }
        }

        normalized_servers.insert(name, normalized);
    }

    serde_json::Value::Object(normalized_servers)
}

fn normalize_public_lsp_config(value: &serde_json::Value) -> Option<serde_json::Value> {
    match value {
        serde_json::Value::Bool(false) => Some(serde_json::json!({ "disabled": true })),
        serde_json::Value::Bool(true) | serde_json::Value::Null => None,
        serde_json::Value::Object(object) if object.contains_key("servers") => Some(value.clone()),
        serde_json::Value::Object(object) => {
            Some(serde_json::json!({ "servers": serde_json::Value::Object(object.clone()) }))
        }
        _ => None,
    }
}

fn normalize_public_skills_config(value: &serde_json::Value) -> serde_json::Value {
    let mut normalized = value.clone();
    let Some(object) = normalized.as_object_mut() else {
        return normalized;
    };
    object.remove("urls");
    normalized
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
        let mcp_value =
            serde_json::json!({ "servers": normalize_public_mcp_servers(value.clone()) });
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
        translated.insert("skills".to_string(), normalize_public_skills_config(value));
    }

    if let Some(value) = object.get("lsp").and_then(normalize_public_lsp_config) {
        translated.insert("lsp".to_string(), value);
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
    let mut schema = serde_json::to_value(schema_for!(PublicRuntimeConfig))
        .map_err(|err| ConfigError::SerializeSchema(err.to_string()))?;
    let definitions = schema
        .get_mut("definitions")
        .and_then(serde_json::Value::as_object_mut);
    if let Some(definitions) = definitions {
        if let Some(agent_map) = definitions
            .get_mut("PublicAgentMap")
            .and_then(serde_json::Value::as_object_mut)
        {
            agent_map.insert(
                "additionalProperties".to_string(),
                serde_json::json!({ "$ref": "#/definitions/PublicAgentConfig" }),
            );
            agent_map.insert(
                "description".to_string(),
                serde_json::Value::String(concat!(
                    "Named agent definitions. Built-in ",
                    "Open",
                    "Code",
                    "-compatible agents are explicit so editors can complete them, and custom names are accepted through the same shape."
                ).to_string()),
            );
        }
        if let Some(disable_property) = definitions
            .get_mut("PublicAgentConfig")
            .and_then(|agent_config| agent_config.get_mut("properties"))
            .and_then(serde_json::Value::as_object_mut)
            .and_then(|properties| properties.get_mut("disable"))
            .and_then(serde_json::Value::as_object_mut)
        {
            disable_property.insert(
                "description".to_string(),
                serde_json::Value::String(
                    concat!(
                        "Open",
                        "Code",
                        "-compatible negative toggle. Equivalent to `enable: false`."
                    )
                    .to_string(),
                ),
            );
        }
    }
    serde_json::to_string_pretty(&schema)
        .map_err(|err| ConfigError::SerializeSchema(err.to_string()))
}

pub fn harness_tui_schema_pretty_json() -> Result<String, ConfigError> {
    let schema = schema_for!(PublicTuiConfig);
    serde_json::to_string_pretty(&schema)
        .map_err(|err| ConfigError::SerializeSchema(err.to_string()))
}
