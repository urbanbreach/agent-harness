use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

use crate::coord::CoordinatorHandle;
use crate::event::{ActorKind, EventActor};

pub type ToolId = String;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum ToolSurface {
    #[default]
    Native,
    Compat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCapability {
    ReadFs,
    EditFs,
    Shell,
    Network,
    SpawnAgent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool_call_id: String,
    pub actor: EventActor,
    pub tool_id: ToolId,
    pub args_digest: String,
    pub args_redacted_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub display_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_json: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactRef>,
}

impl ToolResult {
    pub fn text(display_text: impl Into<String>) -> Self {
        Self {
            display_text: display_text.into(),
            structured_json: None,
            artifacts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolContext {
    pub run_id: String,
    pub workspace_root: PathBuf,
    pub artifacts_dir: PathBuf,
    pub actor: EventActor,
    pub category: Option<String>,
    pub plan_mode: bool,
    pub plan_exit_target_profile: Option<String>,
    pub tool_call_id: String,
    pub coordinator: CoordinatorHandle,
}

impl ToolContext {
    pub fn resolve_workspace_path(&self, relative: &Path) -> Result<PathBuf, ToolError> {
        let candidate = self.workspace_root.join(relative);
        let canonical_workspace = self.workspace_root.canonicalize().map_err(|source| {
            ToolError::WorkspaceRootUnavailable {
                path: self.workspace_root.display().to_string(),
                source,
            }
        })?;

        let canonical_candidate =
            candidate
                .canonicalize()
                .map_err(|source| ToolError::PathResolution {
                    path: candidate.display().to_string(),
                    source,
                })?;

        if !canonical_candidate.starts_with(&canonical_workspace) {
            return Err(ToolError::PathEscapesWorkspace {
                workspace_root: canonical_workspace.display().to_string(),
                path: canonical_candidate.display().to_string(),
            });
        }

        Ok(canonical_candidate)
    }

    pub fn artifact_store(&self) -> Result<ArtifactStore, ArtifactStoreError> {
        ArtifactStore::new(self.artifacts_dir.clone())
    }
}

#[derive(Debug, Clone)]
pub struct ArtifactStore {
    artifacts_dir: PathBuf,
}

impl ArtifactStore {
    pub fn new(artifacts_dir: impl Into<PathBuf>) -> Result<Self, ArtifactStoreError> {
        let artifacts_dir = artifacts_dir.into();
        fs::create_dir_all(&artifacts_dir).map_err(|source| {
            ArtifactStoreError::CreateDirectory {
                path: artifacts_dir.display().to_string(),
                source,
            }
        })?;
        Ok(Self { artifacts_dir })
    }

    pub fn write_text(
        &self,
        name: &str,
        contents: &str,
    ) -> Result<ArtifactRef, ArtifactStoreError> {
        let relative = validate_artifact_name(name)?;
        let target = self.artifacts_dir.join(&relative);

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|source| ArtifactStoreError::CreateDirectory {
                path: parent.display().to_string(),
                source,
            })?;
        }

        fs::write(&target, contents.as_bytes()).map_err(|source| {
            ArtifactStoreError::WriteFile {
                path: target.display().to_string(),
                source,
            }
        })?;

        let digest = blake3::hash(contents.as_bytes()).to_hex().to_string();
        let rel_path = Path::new("artifacts")
            .join(relative)
            .to_string_lossy()
            .to_string();

        Ok(ArtifactRef {
            path: rel_path,
            digest: Some(digest),
        })
    }
}

#[derive(Debug, Error)]
pub enum ArtifactStoreError {
    #[error("artifact name cannot be empty")]
    EmptyName,
    #[error("artifact path must be relative: {0}")]
    AbsolutePath(String),
    #[error("artifact path cannot contain parent traversals: {0}")]
    ParentTraversal(String),
    #[error("failed to create artifact directory {path}: {source}")]
    CreateDirectory {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write artifact file {path}: {source}")]
    WriteFile {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

fn validate_artifact_name(name: &str) -> Result<PathBuf, ArtifactStoreError> {
    if name.trim().is_empty() {
        return Err(ArtifactStoreError::EmptyName);
    }

    let relative = Path::new(name);
    if relative.is_absolute() {
        return Err(ArtifactStoreError::AbsolutePath(name.to_string()));
    }

    if relative
        .components()
        .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(ArtifactStoreError::ParentTraversal(name.to_string()));
    }

    Ok(relative.to_path_buf())
}

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("tool argument error: {0}")]
    InvalidArguments(String),
    #[error("workspace root unavailable: {path}: {source}")]
    WorkspaceRootUnavailable {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to resolve path {path}: {source}")]
    PathResolution {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("path escapes workspace root {workspace_root}: {path}")]
    PathEscapesWorkspace {
        workspace_root: String,
        path: String,
    },
    #[error("command blocked by shell allowlist: {0}")]
    CommandBlocked(String),
    #[error("tool execution failed: {0}")]
    Execution(String),
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn id(&self) -> &str;

    fn description(&self) -> &str {
        "No description provided."
    }

    fn parameters_json_schema(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": true,
        })
    }

    fn capability(&self) -> ToolCapability;

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NativeToolPermissionClass {
    ReadOnly,
    WorkspaceWrite,
    Shell,
    Network,
    AgentSpawn,
    ControlPlane,
    UserInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NativeToolProviderExposure {
    AliasOnly,
    ExplicitOptIn,
    Reserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NativeToolMigrationStatus {
    CompatOnly,
    NativeStable,
    NativeUpgradeInProgress,
    Unimplemented,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeToolParityEntry {
    pub canonical_id: &'static str,
    pub aliases: &'static [&'static str],
    pub permission_class: NativeToolPermissionClass,
    pub provider_exposure: NativeToolProviderExposure,
    pub migration_status: NativeToolMigrationStatus,
}

impl NativeToolParityEntry {
    pub fn all_ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        std::iter::once(self.canonical_id).chain(self.aliases.iter().copied())
    }

    pub const fn registers_canonical_id(&self) -> bool {
        matches!(
            self.migration_status,
            NativeToolMigrationStatus::NativeStable
                | NativeToolMigrationStatus::NativeUpgradeInProgress
        )
    }

    pub const fn exposes_aliases(&self) -> bool {
        matches!(
            self.provider_exposure,
            NativeToolProviderExposure::AliasOnly | NativeToolProviderExposure::ExplicitOptIn
        )
    }

    pub fn primary_alias(&self) -> Option<&'static str> {
        self.aliases.first().copied()
    }

    pub fn tool_id_for_surface(&self, surface: ToolSurface) -> &'static str {
        match surface {
            ToolSurface::Native => self.canonical_id,
            ToolSurface::Compat => {
                if self.exposes_aliases() {
                    self.primary_alias().unwrap_or(self.canonical_id)
                } else {
                    self.canonical_id
                }
            }
        }
    }
}

pub const NATIVE_TOOL_PARITY_MATRIX: &[NativeToolParityEntry] = &[
    NativeToolParityEntry {
        canonical_id: "user.question",
        aliases: &["question"],
        permission_class: NativeToolPermissionClass::UserInput,
        provider_exposure: NativeToolProviderExposure::AliasOnly,
        migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
    },
    NativeToolParityEntry {
        canonical_id: "tool.invalid",
        aliases: &["invalid"],
        permission_class: NativeToolPermissionClass::ControlPlane,
        provider_exposure: NativeToolProviderExposure::AliasOnly,
        migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
    },
    NativeToolParityEntry {
        canonical_id: "fs.write",
        aliases: &["write"],
        permission_class: NativeToolPermissionClass::WorkspaceWrite,
        provider_exposure: NativeToolProviderExposure::AliasOnly,
        migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
    },
    NativeToolParityEntry {
        canonical_id: "web.fetch",
        aliases: &["webfetch"],
        permission_class: NativeToolPermissionClass::Network,
        provider_exposure: NativeToolProviderExposure::AliasOnly,
        migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
    },
    NativeToolParityEntry {
        canonical_id: "todo.write",
        aliases: &["todowrite"],
        permission_class: NativeToolPermissionClass::ControlPlane,
        provider_exposure: NativeToolProviderExposure::AliasOnly,
        migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
    },
    NativeToolParityEntry {
        canonical_id: "todo.read",
        aliases: &["todoread"],
        permission_class: NativeToolPermissionClass::ControlPlane,
        provider_exposure: NativeToolProviderExposure::AliasOnly,
        migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
    },
    NativeToolParityEntry {
        canonical_id: "skill.load",
        aliases: &["skill"],
        permission_class: NativeToolPermissionClass::ControlPlane,
        provider_exposure: NativeToolProviderExposure::AliasOnly,
        migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
    },
    NativeToolParityEntry {
        canonical_id: "search.web",
        aliases: &["websearch"],
        permission_class: NativeToolPermissionClass::Network,
        provider_exposure: NativeToolProviderExposure::AliasOnly,
        migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
    },
    NativeToolParityEntry {
        canonical_id: "search.code",
        aliases: &["codesearch"],
        permission_class: NativeToolPermissionClass::Network,
        provider_exposure: NativeToolProviderExposure::AliasOnly,
        migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
    },
    NativeToolParityEntry {
        canonical_id: "github.issue",
        aliases: &[],
        permission_class: NativeToolPermissionClass::Network,
        provider_exposure: NativeToolProviderExposure::ExplicitOptIn,
        migration_status: NativeToolMigrationStatus::NativeStable,
    },
    NativeToolParityEntry {
        canonical_id: "github.pull_request",
        aliases: &[],
        permission_class: NativeToolPermissionClass::Network,
        provider_exposure: NativeToolProviderExposure::ExplicitOptIn,
        migration_status: NativeToolMigrationStatus::NativeStable,
    },
    NativeToolParityEntry {
        canonical_id: "code.lsp",
        aliases: &["lsp"],
        permission_class: NativeToolPermissionClass::ReadOnly,
        provider_exposure: NativeToolProviderExposure::AliasOnly,
        migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
    },
    NativeToolParityEntry {
        canonical_id: "code.lsp.rename",
        aliases: &[],
        permission_class: NativeToolPermissionClass::WorkspaceWrite,
        provider_exposure: NativeToolProviderExposure::ExplicitOptIn,
        migration_status: NativeToolMigrationStatus::NativeStable,
    },
    NativeToolParityEntry {
        canonical_id: "tool.batch",
        aliases: &["batch"],
        permission_class: NativeToolPermissionClass::ControlPlane,
        provider_exposure: NativeToolProviderExposure::AliasOnly,
        migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
    },
    NativeToolParityEntry {
        canonical_id: "plan.exit",
        aliases: &["plan_exit"],
        permission_class: NativeToolPermissionClass::ControlPlane,
        provider_exposure: NativeToolProviderExposure::AliasOnly,
        migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
    },
    NativeToolParityEntry {
        canonical_id: "agent.spawn",
        aliases: &["task"],
        permission_class: NativeToolPermissionClass::AgentSpawn,
        provider_exposure: NativeToolProviderExposure::ExplicitOptIn,
        migration_status: NativeToolMigrationStatus::NativeUpgradeInProgress,
    },
    NativeToolParityEntry {
        canonical_id: "shell.run",
        aliases: &["bash"],
        permission_class: NativeToolPermissionClass::Shell,
        provider_exposure: NativeToolProviderExposure::ExplicitOptIn,
        migration_status: NativeToolMigrationStatus::NativeStable,
    },
    NativeToolParityEntry {
        canonical_id: "fs.read",
        aliases: &["read"],
        permission_class: NativeToolPermissionClass::ReadOnly,
        provider_exposure: NativeToolProviderExposure::ExplicitOptIn,
        migration_status: NativeToolMigrationStatus::NativeStable,
    },
    NativeToolParityEntry {
        canonical_id: "fs.glob",
        aliases: &["glob"],
        permission_class: NativeToolPermissionClass::ReadOnly,
        provider_exposure: NativeToolProviderExposure::ExplicitOptIn,
        migration_status: NativeToolMigrationStatus::NativeStable,
    },
    NativeToolParityEntry {
        canonical_id: "fs.grep",
        aliases: &["grep"],
        permission_class: NativeToolPermissionClass::ReadOnly,
        provider_exposure: NativeToolProviderExposure::ExplicitOptIn,
        migration_status: NativeToolMigrationStatus::NativeStable,
    },
];

pub const fn native_tool_parity_matrix() -> &'static [NativeToolParityEntry] {
    NATIVE_TOOL_PARITY_MATRIX
}

pub fn native_tool_parity_entry_for(tool_id: &str) -> Option<&'static NativeToolParityEntry> {
    native_tool_parity_matrix()
        .iter()
        .find(|entry| entry.all_ids().any(|candidate| candidate == tool_id))
}

pub fn canonical_tool_id_for(tool_id: &str) -> Option<&'static str> {
    native_tool_parity_entry_for(tool_id).map(|entry| entry.canonical_id)
}

pub fn native_and_alias_tool_ids() -> Vec<&'static str> {
    let mut tool_ids = Vec::new();
    for entry in native_tool_parity_matrix() {
        tool_ids.extend(entry.all_ids());
    }
    tool_ids
}

pub fn tool_id_for_surface<'a>(tool_id: &'a str, surface: ToolSurface) -> Cow<'a, str> {
    native_tool_parity_entry_for(tool_id)
        .map(|entry| Cow::Borrowed(entry.tool_id_for_surface(surface)))
        .unwrap_or_else(|| Cow::Borrowed(tool_id))
}

pub fn resolve_tool_ids_for_surface<I, S>(tool_ids: I, surface: ToolSurface) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut resolved = tool_ids
        .into_iter()
        .map(|tool_id| tool_id_for_surface(tool_id.as_ref(), surface).into_owned())
        .collect::<Vec<_>>();
    resolved.sort();
    resolved.dedup();
    resolved
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolFunctionNameMapping {
    function_to_tool_id: BTreeMap<String, ToolId>,
    tool_id_to_function_name: BTreeMap<ToolId, String>,
}

impl ToolFunctionNameMapping {
    pub fn function_to_tool_id(&self) -> &BTreeMap<String, ToolId> {
        &self.function_to_tool_id
    }

    pub fn tool_id_to_function_name(&self) -> &BTreeMap<ToolId, String> {
        &self.tool_id_to_function_name
    }

    pub fn tool_id_for_function_name(&self, function_name: &str) -> Option<&str> {
        self.function_to_tool_id
            .get(function_name)
            .map(String::as_str)
    }

    pub fn function_name_for_tool_id(&self, tool_id: &str) -> Option<&str> {
        self.tool_id_to_function_name
            .get(tool_id)
            .map(String::as_str)
    }
}

pub fn sanitize_tool_function_name(tool_id: &str) -> String {
    let mut function_name = tool_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();

    let starts_with_valid_char = function_name
        .as_bytes()
        .first()
        .is_some_and(|first| first.is_ascii_alphabetic() || *first == b'_');
    if !starts_with_valid_char {
        function_name.insert_str(0, "t_");
    }

    function_name
}

pub fn build_tool_function_name_mapping<I, S>(tool_ids: I) -> ToolFunctionNameMapping
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut sorted_tool_ids = tool_ids
        .into_iter()
        .map(|tool_id| tool_id.as_ref().to_string())
        .collect::<Vec<_>>();
    sorted_tool_ids.sort();
    sorted_tool_ids.dedup();

    let mut function_to_tool_id = BTreeMap::new();
    let mut tool_id_to_function_name = BTreeMap::new();
    let mut used_function_names = BTreeSet::new();

    for tool_id in sorted_tool_ids {
        let base_name = sanitize_tool_function_name(&tool_id);
        let mut function_name = base_name.clone();
        let mut suffix = 2usize;
        while used_function_names.contains(&function_name) {
            function_name = format!("{base_name}_{suffix}");
            suffix += 1;
        }

        used_function_names.insert(function_name.clone());
        function_to_tool_id.insert(function_name.clone(), tool_id.clone());
        tool_id_to_function_name.insert(tool_id, function_name);
    }

    ToolFunctionNameMapping {
        function_to_tool_id,
        tool_id_to_function_name,
    }
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: BTreeMap<ToolId, Arc<dyn Tool>>,
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids = self.tools.keys().collect::<Vec<_>>();
        f.debug_struct("ToolRegistry")
            .field("tool_ids", &ids)
            .finish()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.id().to_string(), tool);
    }

    pub fn get(&self, tool_id: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(tool_id).cloned()
    }

    pub fn get_for_actor(&self, actor_kind: ActorKind, tool_id: &str) -> Option<Arc<dyn Tool>> {
        let tool = self.get(tool_id)?;
        if self.capability_allowed(actor_kind, tool.capability()) {
            Some(tool)
        } else {
            None
        }
    }

    pub fn capability_allowed(&self, actor_kind: ActorKind, capability: ToolCapability) -> bool {
        actor_capabilities(actor_kind).contains(&capability)
    }

    pub fn filter_for_actor(&self, actor_kind: ActorKind) -> Vec<Arc<dyn Tool>> {
        self.tools
            .values()
            .filter(|tool| self.capability_allowed(actor_kind, tool.capability()))
            .cloned()
            .collect()
    }

    pub fn function_name_mapping(&self) -> ToolFunctionNameMapping {
        build_tool_function_name_mapping(self.tools.keys().map(String::as_str))
    }

    pub fn tool_ids(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }
}

pub fn actor_capabilities(actor_kind: ActorKind) -> BTreeSet<ToolCapability> {
    match actor_kind {
        ActorKind::Worker => BTreeSet::from([
            ToolCapability::ReadFs,
            ToolCapability::EditFs,
            ToolCapability::Shell,
            ToolCapability::Network,
            ToolCapability::SpawnAgent,
        ]),
        ActorKind::Supervisor | ActorKind::System => BTreeSet::from([
            ToolCapability::ReadFs,
            ToolCapability::EditFs,
            ToolCapability::Shell,
            ToolCapability::Network,
            ToolCapability::SpawnAgent,
        ]),
        ActorKind::User => BTreeSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_tool_function_name_mapping, canonical_tool_id_for, native_and_alias_tool_ids,
        native_tool_parity_matrix, sanitize_tool_function_name, ArtifactStore, ArtifactStoreError,
        ToolCapability, ToolRegistry,
    };
    use crate::event::ActorKind;
    use std::collections::BTreeSet;
    use std::fs;

    #[test]
    fn worker_can_use_spawn_agent_capability() {
        let registry = ToolRegistry::new();
        assert!(registry.capability_allowed(ActorKind::Worker, ToolCapability::SpawnAgent));
        assert!(registry.capability_allowed(ActorKind::Worker, ToolCapability::Shell));
    }

    #[test]
    fn artifact_store_write_text_returns_artifact_ref_with_digest() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let store = ArtifactStore::new(temp_dir.path().join("artifacts")).expect("artifact store");

        let artifact = store
            .write_text("notes/output.txt", "hello artifact")
            .expect("write artifact");

        assert_eq!(artifact.path, "artifacts/notes/output.txt");
        assert_eq!(
            artifact.digest.as_deref(),
            Some(blake3::hash(b"hello artifact").to_hex().as_str())
        );
        let written = fs::read_to_string(temp_dir.path().join("artifacts/notes/output.txt"))
            .expect("read artifact file");
        assert_eq!(written, "hello artifact");
    }

    #[test]
    fn artifact_store_rejects_parent_traversal() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let store = ArtifactStore::new(temp_dir.path().join("artifacts")).expect("artifact store");

        let err = store
            .write_text("../escape.txt", "blocked")
            .expect_err("traversal must fail");
        assert!(matches!(err, ArtifactStoreError::ParentTraversal(_)));
    }

    #[test]
    fn sanitize_tool_function_name_applies_milestone_policy() {
        assert_eq!(sanitize_tool_function_name("fs.read"), "fs_read");
        assert_eq!(
            sanitize_tool_function_name("edit/hashline_apply"),
            "edit_hashline_apply"
        );
        assert_eq!(sanitize_tool_function_name("1shell.run"), "t_1shell_run");
        assert_eq!(sanitize_tool_function_name(""), "t_");
    }

    #[test]
    fn function_name_mapping_is_deterministic_unique_and_reversible() {
        let tool_ids = vec!["fs.read", "fs/read", "fs_read", "1tool"];
        let mapping_a = build_tool_function_name_mapping(tool_ids.iter().copied());

        let mut reversed_tool_ids = tool_ids.clone();
        reversed_tool_ids.reverse();
        let mapping_b = build_tool_function_name_mapping(reversed_tool_ids.iter().copied());

        assert_eq!(mapping_a, mapping_b);
        assert_eq!(mapping_a.function_to_tool_id().len(), tool_ids.len());
        assert_eq!(mapping_a.tool_id_to_function_name().len(), tool_ids.len());

        let unique_function_names = mapping_a
            .function_to_tool_id()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        assert_eq!(unique_function_names.len(), tool_ids.len());

        for (function_name, tool_id) in mapping_a.function_to_tool_id() {
            assert_eq!(
                mapping_a.tool_id_for_function_name(function_name),
                Some(tool_id.as_str())
            );
            assert_eq!(
                mapping_a.function_name_for_tool_id(tool_id),
                Some(function_name.as_str())
            );
        }
    }

    #[test]
    fn function_name_mapping_is_stable_for_native_and_alias_tool_ids() {
        let tool_ids = native_and_alias_tool_ids();
        let mapping_a = build_tool_function_name_mapping(tool_ids.iter().copied());

        let mut reversed_tool_ids = tool_ids.clone();
        reversed_tool_ids.reverse();
        let mapping_b = build_tool_function_name_mapping(reversed_tool_ids.iter().copied());

        assert_eq!(mapping_a, mapping_b);
        assert_eq!(mapping_a.function_to_tool_id().len(), tool_ids.len());
        assert_eq!(mapping_a.tool_id_to_function_name().len(), tool_ids.len());

        let unique_tool_ids = tool_ids.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(unique_tool_ids.len(), tool_ids.len());

        let unique_function_names = mapping_a
            .function_to_tool_id()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        assert_eq!(unique_function_names.len(), tool_ids.len());

        for entry in native_tool_parity_matrix() {
            assert_eq!(
                canonical_tool_id_for(entry.canonical_id),
                Some(entry.canonical_id)
            );
            for alias in entry.aliases {
                assert_eq!(canonical_tool_id_for(alias), Some(entry.canonical_id));
            }
        }

        for (function_name, tool_id) in mapping_a.function_to_tool_id() {
            assert_eq!(
                mapping_a.tool_id_for_function_name(function_name),
                Some(tool_id.as_str())
            );
            assert_eq!(
                mapping_a.function_name_for_tool_id(tool_id),
                Some(function_name.as_str())
            );
        }
    }
}
