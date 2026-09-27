use serde::Serialize;
use std::sync::LazyLock;

macro_rules! ids {
    ($($name:ident),*) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
        #[serde(transparent)]
        pub struct $name(pub &'static str);
        impl $name {
            pub const fn new(value: &'static str) -> Self { Self(value) }
            pub const fn as_str(self) -> &'static str { self.0 }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.0) }
        }
    )*};
}
ids!(SettingId, SchemaId);
macro_rules! labels {
    ($($name:ident { $($variant:ident => $label:literal),* }),*) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),* }
        impl $name { pub const fn as_str(self) -> &'static str { match self { $(Self::$variant => $label),* } } }
    )*};
}
labels! {
    SettingSurface { Runtime => "runtime", Tui => "tui" },
    SettingScope { System => "system", User => "user", Profile => "profile", Project => "project", Workspace => "workspace", Worktree => "worktree", Session => "session", CommandLine => "command_line", Environment => "environment" },
    SettingSensitivity { Public => "public", Redacted => "redacted", Secret => "secret" },
    SettingMergeStrategy { Replace => "replace", DeepMergeMap => "deep_merge_map" },
    SettingMutability { ReadOnly => "read_only", Editable => "editable" }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SettingDefinition {
    pub setting_id: SettingId,
    pub schema_id: SchemaId,
    pub surface: SettingSurface,
    pub default_scope: SettingScope,
    pub sensitivity: SettingSensitivity,
    pub capability_dependency: Option<&'static str>,
    pub restart_required: bool,
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
        matches!(self.mutability, SettingMutability::Editable)
    }
}
fn definition(
    id: &'static str,
    schema: &'static str,
    surface: SettingSurface,
    default: Option<&'static str>,
) -> SettingDefinition {
    let metadata = is_metadata_only_setting(id);
    let secret = id == "provider.apiKey";
    SettingDefinition {
        setting_id: SettingId(id),
        schema_id: SchemaId(schema),
        surface,
        default_scope: if metadata {
            SettingScope::Worktree
        } else {
            SettingScope::Project
        },
        sensitivity: if secret {
            SettingSensitivity::Secret
        } else {
            SettingSensitivity::Public
        },
        capability_dependency: id.strip_prefix("permission."),
        restart_required: metadata
            || matches!(id, "runtime.always_approve" | "runtime.session_dir"),
        default_value: default,
        merge_strategy: if matches!(
            id,
            "agent"
                | "provider"
                | "skills"
                | "mcp"
                | "model_profile"
                | "ui"
                | "runtime.compaction.model_thresholds"
                | "runtime.compaction.agent_thresholds"
                | "keybinds"
        ) {
            SettingMergeStrategy::DeepMergeMap
        } else {
            SettingMergeStrategy::Replace
        },
        mutability: if secret || metadata || id == "$schema" {
            SettingMutability::ReadOnly
        } else {
            SettingMutability::Editable
        },
    }
}
macro_rules! entries {
    ($prefix:literal, $surface:ident; $($id:literal $(=> $default:literal)?),* $(,)?) => {
        [$(definition($id, concat!($prefix, $id), SettingSurface::$surface, [$($default)?].first().copied())),*]
    };
}
static REGISTRY: LazyLock<Vec<SettingDefinition>> = LazyLock::new(|| {
    let mut entries = entries!("harness.runtime.", Runtime;
        "model", "small_model", "agent", "provider", "skills", "mcp", "formatter", "instructions", "model_profile", "lsp",
        "disabled_providers", "enabled_providers", "shell", "logging", "ui",
        "permission.bash", "permission.edit", "permission.question", "permission.task", "permission.webfetch", "permission.websearch",
        "permission.codesearch", "permission.lsp", "permission.read", "permission.external_directory", "permission.doom_loop", "permission.shell_allowlist",
        "provider.apiKey", "hashline_edit" => "true", "worktree.relative_base" => ".agent-harness/worktrees", "worktree.branch_prefix" => "harness/wt-",
    ).to_vec();
    entries.extend(entries!("harness.", Runtime;
        "runtime.always_approve" => "false", "runtime.compaction.enabled" => "true", "runtime.compaction.reserve_tokens" => "16384",
        "runtime.compaction.keep_recent_tokens" => "20000", "runtime.compaction.threshold_percent", "runtime.compaction.threshold_tokens",
        "runtime.compaction.model_thresholds" => "{}", "runtime.compaction.agent_thresholds" => "{}", "runtime.compaction.fallback_input_tokens" => "32768",
        "runtime.compaction.auto_retry_overflow" => "true", "runtime.compaction.structured_summary_contract" => "true", "runtime.compaction.estimated_token_triggers" => "true",
        "runtime.deterministic.enabled" => "false", "runtime.session_dir" => ".agent-harness/sessions",
    ));
    entries.extend(entries!("harness.tui.", Tui; "confirm_before_rewind" => "true", "keybinds" => "{}", "$schema"));
    entries
});
pub fn settings_registry() -> &'static [SettingDefinition] {
    &REGISTRY
}
pub fn setting_definition(id: &str) -> Option<&'static SettingDefinition> {
    let id = match id {
        "hashlineEdit" | "hashline-edit" => "hashline_edit",
        _ => id,
    };
    settings_registry()
        .iter()
        .find(|entry| entry.setting_id.0 == id)
}
pub fn resolve_setting_id(id: &str) -> Option<&'static str> {
    setting_definition(id).map(|entry| entry.setting_id.0)
}
pub fn is_metadata_only_setting(id: &str) -> bool {
    matches!(id, "worktree.relative_base" | "worktree.branch_prefix")
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
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
        format!("settings registry: {} total (runtime={}, tui={}, editable={}, read_only={}, secret={}, metadata_only={}, with_default={})", self.total, self.runtime, self.tui, self.editable, self.read_only, self.secret, self.metadata_only, self.with_default)
    }
    pub const fn has_editable(&self) -> bool {
        self.editable > 0
    }
}
pub fn summarize_settings_registry() -> SettingsRegistrySummary {
    let mut summary = SettingsRegistrySummary::default();
    for entry in settings_registry() {
        summary.total += 1;
        summary.runtime += usize::from(entry.surface == SettingSurface::Runtime);
        summary.tui += usize::from(entry.surface == SettingSurface::Tui);
        summary.editable += usize::from(entry.is_editable());
        summary.read_only += usize::from(!entry.is_editable());
        summary.secret += usize::from(entry.is_secret());
        summary.metadata_only += usize::from(is_metadata_only_setting(entry.setting_id.0));
        summary.with_default += usize::from(entry.has_default());
    }
    summary
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SettingCompatMigration {
    pub legacy_id: &'static str,
    pub canonical_id: &'static str,
}
pub fn settings_compat_migrations() -> &'static [SettingCompatMigration] {
    &[
        SettingCompatMigration {
            legacy_id: "hashlineEdit",
            canonical_id: "hashline_edit",
        },
        SettingCompatMigration {
            legacy_id: "hashline-edit",
            canonical_id: "hashline_edit",
        },
    ]
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    pub default_value: Option<String>,
    pub capability_dependency: Option<String>,
    pub project_write_supported: bool,
    pub resolved_from_legacy: Option<String>,
}
pub fn explain_setting(id: &str) -> Option<SettingSourceExplanation> {
    let entry = setting_definition(id)?;
    Some(SettingSourceExplanation {
        setting_id: entry.setting_id.to_string(),
        schema_id: entry.schema_id.to_string(),
        surface: entry.surface.as_str().into(),
        default_scope: entry.default_scope.as_str().into(),
        sensitivity: entry.sensitivity.as_str().into(),
        merge_strategy: entry.merge_strategy.as_str().into(),
        mutability: entry.mutability.as_str().into(),
        metadata_only: is_metadata_only_setting(entry.setting_id.0),
        restart_required: entry.restart_required,
        has_default: entry.has_default(),
        default_value: entry.default_value.map(str::to_owned),
        capability_dependency: entry.capability_dependency.map(str::to_owned),
        project_write_supported: super::settings_write::writable(entry.setting_id.0).is_ok(),
        resolved_from_legacy: (id != entry.setting_id.0).then(|| id.into()),
    })
}
pub fn settings_registry_json() -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(settings_registry())
}
