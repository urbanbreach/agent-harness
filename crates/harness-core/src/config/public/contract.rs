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
    pub(super) fn matches(self, value: &serde_json::Value) -> bool {
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
    pub(super) fn runtime_top_level_key(&self, name: &str) -> Option<&PublicConfigTopLevelKey> {
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

    pub(super) fn runtime_aliases_for_scope(
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
    runtime_key!("disabled_providers", Compatibility, schema, docs),
    runtime_key!("enabled_providers", Compatibility, schema, docs),
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
