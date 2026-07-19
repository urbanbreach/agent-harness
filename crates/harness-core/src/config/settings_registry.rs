//! Typed settings registry foundation over existing public runtime/TUI keys.
//!
//! Complements [`super::public::public_config_contract`] with per-setting
//! metadata (scope, sensitivity, capability dependency, restart, merge, mutability).
//! Does not merge runtime and TUI public file contracts or implement migrations.

use serde::{Deserialize, Serialize};

use crate::worktree::{DEFAULT_WORKTREE_RELATIVE_BASE, WORKTREE_BRANCH_PREFIX};

/// Stable public setting identifier (dotted path over public config keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SettingId(pub &'static str);

impl SettingId {
    pub const fn new(value: &'static str) -> Self {
        Self(value)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl AsRef<str> for SettingId {
    fn as_ref(&self) -> &str {
        self.0
    }
}

impl std::fmt::Display for SettingId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// Stable schema identifier for generated/effective settings schema work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaId(pub &'static str);

impl SchemaId {
    pub const fn new(value: &'static str) -> Self {
        Self(value)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl AsRef<str> for SchemaId {
    fn as_ref(&self) -> &str {
        self.0
    }
}

impl std::fmt::Display for SchemaId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

/// Which public file contract owns the setting (runtime vs TUI stay separate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingSurface {
    Runtime,
    Tui,
}

/// Default discovery/merge scope for the setting (foundation; full layer engine is later work).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingScope {
    System,
    User,
    Profile,
    Project,
    Workspace,
    Worktree,
    Session,
    CommandLine,
    Environment,
}

/// Redaction/sensitivity class for effective-config and evidence surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingSensitivity {
    Public,
    Redacted,
    Secret,
}

/// Deterministic merge strategy when layers contribute the same setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingMergeStrategy {
    /// Later layer replaces the earlier value wholesale (scalars, modes).
    Replace,
    /// Nested maps deep-merge; arrays/primitives still replace.
    DeepMergeMap,
}

/// Whether the settings editor / write path may mutate the setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingMutability {
    /// Visible in registry/UI; edits are rejected.
    ReadOnly,
    /// Eligible for project-file edit when the write path supports the key.
    Editable,
}

/// One registered public setting with foundation metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingDefinition {
    pub setting_id: SettingId,
    pub schema_id: SchemaId,
    pub surface: SettingSurface,
    pub default_scope: SettingScope,
    pub sensitivity: SettingSensitivity,
    pub capability_dependency: Option<&'static str>,
    pub restart_required: bool,
    /// Stable string form of the built-in default when one exists.
    pub default_value: Option<&'static str>,
    pub merge_strategy: SettingMergeStrategy,
    pub mutability: SettingMutability,
}

impl SettingDefinition {
    pub const fn has_default(self) -> bool {
        self.default_value.is_some()
    }

    pub const fn is_secret(self) -> bool {
        matches!(self.sensitivity, SettingSensitivity::Secret)
    }

    pub const fn is_editable(self) -> bool {
        matches!(self.mutability, SettingMutability::Editable) && !self.is_secret()
    }
}

const fn def(
    setting_id: &'static str,
    schema_id: &'static str,
    surface: SettingSurface,
    default_scope: SettingScope,
    sensitivity: SettingSensitivity,
    capability_dependency: Option<&'static str>,
    restart_required: bool,
    default_value: Option<&'static str>,
    merge_strategy: SettingMergeStrategy,
    mutability: SettingMutability,
) -> SettingDefinition {
    SettingDefinition {
        setting_id: SettingId(setting_id),
        schema_id: SchemaId(schema_id),
        surface,
        default_scope,
        sensitivity,
        capability_dependency,
        restart_required,
        default_value,
        merge_strategy,
        mutability,
    }
}

const fn runtime_public(
    setting_id: &'static str,
    schema_id: &'static str,
    default_value: Option<&'static str>,
) -> SettingDefinition {
    def(
        setting_id,
        schema_id,
        SettingSurface::Runtime,
        SettingScope::Project,
        SettingSensitivity::Public,
        None,
        false,
        default_value,
        SettingMergeStrategy::Replace,
        SettingMutability::Editable,
    )
}

const fn runtime_public_map(
    setting_id: &'static str,
    schema_id: &'static str,
    default_value: Option<&'static str>,
) -> SettingDefinition {
    def(
        setting_id,
        schema_id,
        SettingSurface::Runtime,
        SettingScope::Project,
        SettingSensitivity::Public,
        None,
        false,
        default_value,
        SettingMergeStrategy::DeepMergeMap,
        SettingMutability::Editable,
    )
}

const fn runtime_permission(
    setting_id: &'static str,
    schema_id: &'static str,
    capability: &'static str,
) -> SettingDefinition {
    def(
        setting_id,
        schema_id,
        SettingSurface::Runtime,
        SettingScope::Project,
        SettingSensitivity::Public,
        Some(capability),
        false,
        None,
        SettingMergeStrategy::Replace,
        SettingMutability::Editable,
    )
}

/// Focused high-value public settings registry. Expand surgically; not full coverage.
// allow: SIZE_OK — static settings registry data table + metadata JSON API
const SETTINGS_REGISTRY: &[SettingDefinition] = &[
    runtime_public("model", "harness.runtime.model", None),
    runtime_public("default_agent", "harness.runtime.default_agent", None),
    runtime_public("small_model", "harness.runtime.small_model", None),
    runtime_public_map("agent", "harness.runtime.agent", None),
    runtime_public_map("mode", "harness.runtime.mode", None),
    runtime_public_map("provider", "harness.runtime.provider", None),
    runtime_public_map("skills", "harness.runtime.skills", None),
    runtime_public_map("mcp", "harness.runtime.mcp", None),
    runtime_public("formatter", "harness.runtime.formatter", None),
    runtime_public("instructions", "harness.runtime.instructions", None),
    runtime_public_map("model_profile", "harness.runtime.model_profile", None),
    runtime_public("lsp", "harness.runtime.lsp", None),
    runtime_public(
        "disabled_providers",
        "harness.runtime.disabled_providers",
        None,
    ),
    runtime_public(
        "enabled_providers",
        "harness.runtime.enabled_providers",
        None,
    ),
    runtime_public("shell", "harness.runtime.shell", None),
    runtime_public("logging", "harness.runtime.logging", None),
    runtime_public_map("ui", "harness.runtime.ui", None),
    runtime_permission("permission.bash", "harness.runtime.permission.bash", "bash"),
    runtime_permission("permission.edit", "harness.runtime.permission.edit", "edit"),
    runtime_permission(
        "permission.question",
        "harness.runtime.permission.question",
        "question",
    ),
    runtime_permission("permission.task", "harness.runtime.permission.task", "task"),
    runtime_permission(
        "permission.webfetch",
        "harness.runtime.permission.webfetch",
        "webfetch",
    ),
    runtime_permission(
        "permission.websearch",
        "harness.runtime.permission.websearch",
        "websearch",
    ),
    runtime_permission(
        "permission.codesearch",
        "harness.runtime.permission.codesearch",
        "codesearch",
    ),
    runtime_permission("permission.lsp", "harness.runtime.permission.lsp", "lsp"),
    runtime_permission("permission.read", "harness.runtime.permission.read", "read"),
    runtime_permission(
        "permission.external_directory",
        "harness.runtime.permission.external_directory",
        "external_directory",
    ),
    runtime_permission(
        "permission.doom_loop",
        "harness.runtime.permission.doom_loop",
        "doom_loop",
    ),
    runtime_public(
        "permission.shell_allowlist",
        "harness.runtime.permission.shell_allowlist",
        None,
    ),
    runtime_public(
        "runtime.compaction.enabled",
        "harness.runtime.compaction.enabled",
        Some("true"),
    ),
    runtime_public(
        "runtime.compaction.reserve_tokens",
        "harness.runtime.compaction.reserve_tokens",
        Some("16384"),
    ),
    runtime_public(
        "runtime.compaction.keep_recent_tokens",
        "harness.runtime.compaction.keep_recent_tokens",
        Some("20000"),
    ),
    runtime_public(
        "runtime.compaction.fallback_input_tokens",
        "harness.runtime.compaction.fallback_input_tokens",
        Some("32768"),
    ),
    runtime_public(
        "runtime.compaction.auto_retry_overflow",
        "harness.runtime.compaction.auto_retry_overflow",
        Some("true"),
    ),
    runtime_public(
        "runtime.compaction.structured_summary_contract",
        "harness.runtime.compaction.structured_summary_contract",
        Some("true"),
    ),
    runtime_public(
        "runtime.compaction.estimated_token_triggers",
        "harness.runtime.compaction.estimated_token_triggers",
        Some("true"),
    ),
    runtime_public(
        "runtime.deterministic.enabled",
        "harness.runtime.deterministic.enabled",
        Some("false"),
    ),
    def(
        "runtime.session_dir",
        "harness.runtime.session_dir",
        SettingSurface::Runtime,
        SettingScope::Project,
        SettingSensitivity::Public,
        None,
        true,
        Some(".agent-harness/sessions"),
        SettingMergeStrategy::Replace,
        SettingMutability::Editable,
    ),
    def(
        "provider.apiKey",
        "harness.runtime.provider.apiKey",
        SettingSurface::Runtime,
        SettingScope::Project,
        SettingSensitivity::Secret,
        None,
        false,
        None,
        SettingMergeStrategy::Replace,
        SettingMutability::ReadOnly,
    ),
    runtime_public(
        "hashline_edit",
        "harness.runtime.hashline_edit",
        Some("true"),
    ),
    def(
        "keybinds",
        "harness.tui.keybinds",
        SettingSurface::Tui,
        SettingScope::Project,
        SettingSensitivity::Public,
        None,
        false,
        Some("{}"),
        SettingMergeStrategy::DeepMergeMap,
        SettingMutability::Editable,
    ),
    def(
        "$schema",
        "harness.tui.$schema",
        SettingSurface::Tui,
        SettingScope::Project,
        SettingSensitivity::Public,
        None,
        false,
        None,
        SettingMergeStrategy::Replace,
        SettingMutability::ReadOnly,
    ),
    // Metadata-only: product defaults from worktree.rs, not public schema keys yet.
    def(
        "worktree.relative_base",
        "harness.runtime.worktree.relative_base",
        SettingSurface::Runtime,
        SettingScope::Worktree,
        SettingSensitivity::Public,
        None,
        true,
        Some(DEFAULT_WORKTREE_RELATIVE_BASE),
        SettingMergeStrategy::Replace,
        SettingMutability::ReadOnly,
    ),
    def(
        "worktree.branch_prefix",
        "harness.runtime.worktree.branch_prefix",
        SettingSurface::Runtime,
        SettingScope::Worktree,
        SettingSensitivity::Public,
        None,
        true,
        Some(WORKTREE_BRANCH_PREFIX),
        SettingMergeStrategy::Replace,
        SettingMutability::ReadOnly,
    ),
];

/// Setting ids that intentionally have no public harness.json / tui.json path yet.
const METADATA_ONLY_SETTING_IDS: &[&str] = &["worktree.relative_base", "worktree.branch_prefix"];

/// Returns the static typed settings registry (foundation; not a full migration engine).
pub fn settings_registry() -> &'static [SettingDefinition] {
    SETTINGS_REGISTRY
}

/// Operator-facing counts for the settings registry (diagnostics only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SettingsRegistrySummary {
    pub total: usize,
    pub runtime: usize,
    pub tui: usize,
    pub editable: usize,
    pub read_only: usize,
    pub secret: usize,
    pub metadata_only: usize,
    pub with_default: usize,
}

impl SettingsRegistrySummary {
    pub fn one_line(&self) -> String {
        format!(
            "settings registry: {} total (runtime={}, tui={}, editable={}, read_only={}, secret={}, metadata_only={}, with_default={})",
            self.total,
            self.runtime,
            self.tui,
            self.editable,
            self.read_only,
            self.secret,
            self.metadata_only,
            self.with_default
        )
    }

    pub const fn has_editable(&self) -> bool {
        self.editable > 0
    }
}

/// Summarize registry composition for operator/CLI surfaces.
pub fn summarize_settings_registry() -> SettingsRegistrySummary {
    let mut summary = SettingsRegistrySummary {
        total: settings_registry().len(),
        ..SettingsRegistrySummary::default()
    };
    for entry in settings_registry() {
        match entry.surface {
            SettingSurface::Runtime => {
                summary.runtime = summary.runtime.saturating_add(1);
            }
            SettingSurface::Tui => {
                summary.tui = summary.tui.saturating_add(1);
            }
        }
        match entry.mutability {
            SettingMutability::Editable => {
                summary.editable = summary.editable.saturating_add(1);
            }
            SettingMutability::ReadOnly => {
                summary.read_only = summary.read_only.saturating_add(1);
            }
        }
        if matches!(entry.sensitivity, SettingSensitivity::Secret) {
            summary.secret = summary.secret.saturating_add(1);
        }
        if is_metadata_only_setting(entry.setting_id.as_str()) {
            summary.metadata_only = summary.metadata_only.saturating_add(1);
        }
        if entry.has_default() {
            summary.with_default = summary.with_default.saturating_add(1);
        }
    }
    summary
}

/// Look up one setting by stable `setting_id`.
pub fn setting_definition(setting_id: &str) -> Option<&'static SettingDefinition> {
    let canonical = resolve_setting_id(setting_id).unwrap_or(setting_id);
    settings_registry()
        .iter()
        .find(|entry| entry.setting_id.as_str() == canonical)
}

/// True when the setting documents product metadata without a public config key.
pub fn is_metadata_only_setting(setting_id: &str) -> bool {
    let canonical = resolve_setting_id(setting_id).unwrap_or(setting_id);
    METADATA_ONLY_SETTING_IDS.iter().any(|id| *id == canonical)
}

/// One legacy → canonical settings-id rename for load/write migration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingCompatMigration {
    pub legacy_id: &'static str,
    pub canonical_id: &'static str,
}

const SETTINGS_COMPAT_MIGRATIONS: &[SettingCompatMigration] = &[
    SettingCompatMigration {
        legacy_id: "hashlineEdit",
        canonical_id: "hashline_edit",
    },
    SettingCompatMigration {
        legacy_id: "hashline-edit",
        canonical_id: "hashline_edit",
    },
];

/// Compatibility renames applied before registry lookup / project writes.
pub fn settings_compat_migrations() -> &'static [SettingCompatMigration] {
    SETTINGS_COMPAT_MIGRATIONS
}

/// Resolve a legacy or canonical setting id to the registry canonical id.
pub fn resolve_setting_id(setting_id: &str) -> Option<&'static str> {
    if let Some(entry) = settings_registry()
        .iter()
        .find(|entry| entry.setting_id.as_str() == setting_id)
    {
        return Some(entry.setting_id.as_str());
    }
    SETTINGS_COMPAT_MIGRATIONS
        .iter()
        .find(|migration| migration.legacy_id == setting_id)
        .map(|migration| migration.canonical_id)
}

/// Source-explanation record for one registry setting (no secret values).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingSourceExplanation {
    pub setting_id: String,
    pub schema_id: String,
    pub surface: String,
    pub default_scope: String,
    pub sensitivity: String,
    pub merge_strategy: String,
    pub mutability: String,
    pub metadata_only: bool,
    pub restart_required: bool,
    pub has_default: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_dependency: Option<String>,
    /// True when project-file bool write path supports this setting today.
    pub project_write_supported: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_from_legacy: Option<String>,
}

impl SettingSourceExplanation {
    pub fn one_line(&self) -> String {
        format!(
            "setting {}: surface={} scope={} merge={} mutability={} metadata_only={} write={}",
            self.setting_id,
            self.surface,
            self.default_scope,
            self.merge_strategy,
            self.mutability,
            self.metadata_only,
            self.project_write_supported
        )
    }
}

const PROJECT_BOOL_WRITE_SETTING_IDS: &[&str] = &[
    "hashline_edit",
    "runtime.compaction.enabled",
    "runtime.compaction.auto_retry_overflow",
    "runtime.compaction.structured_summary_contract",
    "runtime.compaction.estimated_token_triggers",
    "runtime.deterministic.enabled",
];

/// Explain one setting id for source-explanation / settings-editor journeys.
pub fn explain_setting(setting_id: &str) -> Option<SettingSourceExplanation> {
    let legacy = SETTINGS_COMPAT_MIGRATIONS
        .iter()
        .find(|migration| migration.legacy_id == setting_id)
        .map(|migration| migration.legacy_id.to_string());
    let def = setting_definition(setting_id)?;
    let id = def.setting_id.as_str();
    let surface = match def.surface {
        SettingSurface::Runtime => "runtime",
        SettingSurface::Tui => "tui",
    };
    let default_scope = match def.default_scope {
        SettingScope::System => "system",
        SettingScope::User => "user",
        SettingScope::Profile => "profile",
        SettingScope::Project => "project",
        SettingScope::Workspace => "workspace",
        SettingScope::Worktree => "worktree",
        SettingScope::Session => "session",
        SettingScope::CommandLine => "command_line",
        SettingScope::Environment => "environment",
    };
    let sensitivity = match def.sensitivity {
        SettingSensitivity::Public => "public",
        SettingSensitivity::Redacted => "redacted",
        SettingSensitivity::Secret => "secret",
    };
    let merge_strategy = match def.merge_strategy {
        SettingMergeStrategy::Replace => "replace",
        SettingMergeStrategy::DeepMergeMap => "deep_merge_map",
    };
    let mutability = match def.mutability {
        SettingMutability::ReadOnly => "read_only",
        SettingMutability::Editable => "editable",
    };
    let default_value = if matches!(def.sensitivity, SettingSensitivity::Secret) {
        None
    } else {
        def.default_value.map(str::to_string)
    };
    Some(SettingSourceExplanation {
        setting_id: id.to_string(),
        schema_id: def.schema_id.as_str().to_string(),
        surface: surface.to_string(),
        default_scope: default_scope.to_string(),
        sensitivity: sensitivity.to_string(),
        merge_strategy: merge_strategy.to_string(),
        mutability: mutability.to_string(),
        metadata_only: is_metadata_only_setting(id),
        restart_required: def.restart_required,
        has_default: def.has_default(),
        default_value,
        capability_dependency: def.capability_dependency.map(str::to_string),
        project_write_supported: PROJECT_BOOL_WRITE_SETTING_IDS.contains(&id),
        resolved_from_legacy: legacy,
    })
}

/// Redacted JSON envelope of registry metadata (no secret or default values).
///
/// Schema: `harness-settings-registry-v1` with `setting_count` and `settings[]`
/// entries carrying `setting_id`, `schema_id`, `surface`, `sensitivity`,
/// `merge_strategy`, `mutability`, `metadata_only`.
pub fn settings_registry_json() -> Result<String, serde_json::Error> {
    let settings: Vec<serde_json::Value> = settings_registry()
        .iter()
        .map(|entry| {
            let surface = match entry.surface {
                SettingSurface::Runtime => "runtime",
                SettingSurface::Tui => "tui",
            };
            let sensitivity = match entry.sensitivity {
                SettingSensitivity::Public => "public",
                SettingSensitivity::Redacted => "redacted",
                SettingSensitivity::Secret => "secret",
            };
            let merge_strategy = match entry.merge_strategy {
                SettingMergeStrategy::Replace => "replace",
                SettingMergeStrategy::DeepMergeMap => "deep_merge_map",
            };
            let mutability = match entry.mutability {
                SettingMutability::ReadOnly => "read_only",
                SettingMutability::Editable => "editable",
            };
            serde_json::json!({
                "setting_id": entry.setting_id.as_str(),
                "schema_id": entry.schema_id.as_str(),
                "surface": surface,
                "sensitivity": sensitivity,
                "merge_strategy": merge_strategy,
                "mutability": mutability,
                "metadata_only": is_metadata_only_setting(entry.setting_id.as_str()),
            })
        })
        .collect();
    let envelope = serde_json::json!({
        "schema_version": "harness-settings-registry-v1",
        "setting_count": settings.len(),
        "settings": settings,
    });
    serde_json::to_string_pretty(&envelope)
}
