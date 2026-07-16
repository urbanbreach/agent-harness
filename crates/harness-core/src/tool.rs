// allow: SIZE_OK — tool trait and registry (schema + execution + artifacts)
use crate::UnwrapOrAbort;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use thiserror::Error;

use crate::agent::AgentModelSettings;
use crate::coord::CoordinatorHandle;
use crate::edit::hashline::LineAnchor;
use crate::event::{ActorKind, EventActor};
use crate::session_paths::ARTIFACTS_DIR_NAME;

pub type ToolId = String;

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
    pub tool_call_id: crate::ids::ToolCallId,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolResultContent {
    Text {
        text: String,
    },
    File {
        uri: String,
        mime: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
}

impl ToolResultContent {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn file(uri: impl Into<String>, mime: impl Into<String>, name: Option<String>) -> Self {
        Self::File {
            uri: uri.into(),
            mime: mime.into(),
            name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    pub display_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_json: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactRef>,
    #[serde(skip)]
    pub provider_content: Vec<ToolResultContent>,
}

impl ToolResult {
    pub fn text(display_text: impl Into<String>) -> Self {
        Self {
            display_text: display_text.into(),
            structured_json: None,
            artifacts: Vec::new(),
            provider_content: Vec::new(),
        }
    }

    pub fn structured(display_text: impl Into<String>, structured_json: Value) -> Self {
        Self::structured_with_artifacts(display_text, structured_json, Vec::new())
    }

    pub fn artifacts(display_text: impl Into<String>, artifacts: Vec<ArtifactRef>) -> Self {
        Self {
            display_text: display_text.into(),
            structured_json: None,
            artifacts,
            provider_content: Vec::new(),
        }
    }

    pub fn structured_with_artifacts(
        display_text: impl Into<String>,
        structured_json: Value,
        artifacts: Vec<ArtifactRef>,
    ) -> Self {
        Self {
            display_text: display_text.into(),
            structured_json: Some(structured_json),
            artifacts,
            provider_content: Vec::new(),
        }
    }

    pub fn with_provider_content(mut self, provider_content: Vec<ToolResultContent>) -> Self {
        self.provider_content = provider_content;
        self
    }
}

/// Ephemeral state shared by tool calls in the same coordinator run.
///
/// Cloning this value intentionally shares the underlying registry, so read
/// anchors observed by one tool call can disambiguate a later edit in the same
/// run without leaking across independently-created run states.
#[derive(Debug, Clone, Default)]
pub struct ToolRunState {
    edit_session: EditSession,
}

impl ToolRunState {
    pub fn edit_session(&self) -> &EditSession {
        &self.edit_session
    }
}

#[derive(Debug, Clone, Default)]
pub struct EditSession {
    file_reads: Arc<Mutex<FileReadRegistry>>,
}

impl EditSession {
    pub fn record_file_read(&self, resolved_path: &Path) -> Result<(), ToolError> {
        self.record_file_read_with_anchors(resolved_path, None)
    }

    pub fn record_file_hashline_read(
        &self,
        resolved_path: &Path,
        anchors: Vec<LineAnchor>,
    ) -> Result<(), ToolError> {
        self.record_file_read_with_anchors(resolved_path, Some(anchors))
    }

    pub fn recent_hashline_anchors(&self, resolved_path: &Path) -> Option<Vec<LineAnchor>> {
        let prior = {
            let state = lock_file_reads(&self.file_reads);
            state.reads.get(resolved_path).cloned()
        }?;

        let anchors = prior.hashline_anchors?;
        let current = file_read_stamp_from_metadata(resolved_path).ok()?;
        (current.fingerprint == prior.fingerprint).then_some(anchors)
    }

    fn record_file_read_with_anchors(
        &self,
        resolved_path: &Path,
        anchors: Option<Vec<LineAnchor>>,
    ) -> Result<(), ToolError> {
        let stamp = file_read_stamp_from_metadata(resolved_path)?;
        let mut state = lock_file_reads(&self.file_reads);
        state.reads.insert(
            resolved_path.to_path_buf(),
            FileReadStamp {
                hashline_anchors: anchors,
                ..stamp
            },
        );
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    content_hash: u64,
}

#[derive(Debug, Clone)]
struct FileReadStamp {
    fingerprint: FileFingerprint,
    hashline_anchors: Option<Vec<LineAnchor>>,
}

#[derive(Debug, Default)]
struct FileReadRegistry {
    reads: BTreeMap<PathBuf, FileReadStamp>,
}

fn lock_file_reads(mutex: &Mutex<FileReadRegistry>) -> std::sync::MutexGuard<'_, FileReadRegistry> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn file_read_stamp_from_metadata(resolved_path: &Path) -> Result<FileReadStamp, ToolError> {
    fs::metadata(resolved_path).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => {
            ToolError::Execution(format!("File {} not found", resolved_path.display()))
        }
        _ => ToolError::Execution(format!(
            "failed to inspect file freshness for {}: {err}",
            resolved_path.display()
        )),
    })?;
    let content = fs::read(resolved_path).map_err(|err| {
        ToolError::Execution(format!(
            "failed to read file freshness digest for {}: {err}",
            resolved_path.display()
        ))
    })?;

    Ok(FileReadStamp {
        fingerprint: FileFingerprint {
            content_hash: content_fingerprint(&content),
        },
        hashline_anchors: None,
    })
}

fn content_fingerprint(content: &[u8]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

#[derive(Debug, Clone)]
pub struct ToolContext {
    pub run_id: String,
    pub workspace_root: PathBuf,
    pub artifacts_dir: PathBuf,
    pub actor: EventActor,
    pub category: Option<String>,
    pub tool_call_id: crate::ids::ToolCallId,
    pub current_model_ref: Option<String>,
    pub current_model_settings: Option<AgentModelSettings>,
    pub tool_state: ToolRunState,
    pub coordinator: CoordinatorHandle,
    /// Call-scoped absolute path prefixes authorized for outside-workspace I/O.
    /// Empty means fail-closed for any path outside the workspace.
    pub external_directory_allow_prefixes: Vec<PathBuf>,
}

impl ToolContext {
    pub fn external_path_authorized(&self, path: &Path) -> bool {
        if self.external_directory_allow_prefixes.is_empty() {
            return false;
        }
        let candidate = canonicalize_best_effort(path);
        self.external_directory_allow_prefixes.iter().any(|prefix| {
            let prefix = canonicalize_best_effort(prefix);
            candidate == prefix || candidate.starts_with(&prefix)
        })
    }

    pub fn resolve_workspace_path(&self, relative: &Path) -> Result<PathBuf, ToolError> {
        let canonical_workspace = self.workspace_root.canonicalize().map_err(|source| {
            ToolError::WorkspaceRootUnavailable {
                path: self.workspace_root.display().to_string(),
                source,
            }
        })?;

        match normalize_workspace_target_path(&canonical_workspace, relative) {
            Ok(candidate) => {
                if candidate == canonical_workspace {
                    return Ok(canonical_workspace);
                }

                let canonical_candidate =
                    candidate
                        .canonicalize()
                        .map_err(|source| ToolError::PathResolution {
                            path: candidate.display().to_string(),
                            source,
                        })?;

                if canonical_candidate.starts_with(&canonical_workspace) {
                    return Ok(canonical_candidate);
                }

                if self.external_path_authorized(&canonical_candidate) {
                    return Ok(canonical_candidate);
                }

                Err(ToolError::PathEscapesWorkspace {
                    workspace_root: canonical_workspace.display().to_string(),
                    path: canonical_candidate.display().to_string(),
                })
            }
            Err(ToolError::PathEscapesWorkspace { path, .. }) => {
                let external = PathBuf::from(&path);
                let resolved = canonicalize_best_effort(&external);
                if self.external_path_authorized(&resolved) {
                    Ok(resolved)
                } else {
                    Err(ToolError::PathEscapesWorkspace {
                        workspace_root: canonical_workspace.display().to_string(),
                        path,
                    })
                }
            }
            Err(err) => Err(err),
        }
    }

    pub fn artifact_store(&self) -> Result<ArtifactStore, ArtifactStoreError> {
        ArtifactStore::new(self.artifacts_dir.clone())
    }
}

fn canonicalize_best_effort(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| {
        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    let _ = normalized.pop();
                }
                std::path::Component::CurDir => {}
                other => normalized.push(other.as_os_str()),
            }
        }
        if normalized.as_os_str().is_empty() {
            path.to_path_buf()
        } else {
            normalized
        }
    })
}

pub(crate) fn normalize_workspace_target_path(
    workspace: &Path,
    input: &Path,
) -> Result<PathBuf, ToolError> {
    let relative = if input.is_absolute() {
        match input.strip_prefix(workspace) {
            Ok(relative) => relative,
            Err(_) => {
                let canonical_input = input.canonicalize().unwrap_or_else(|_| input.to_path_buf());
                if canonical_input.starts_with(workspace) {
                    return Ok(canonical_input);
                }
                return Err(ToolError::PathEscapesWorkspace {
                    workspace_root: workspace.display().to_string(),
                    path: canonical_input.display().to_string(),
                });
            }
        }
    } else {
        input
    };

    let mut normalized = workspace.to_path_buf();
    for component in relative.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(segment) => normalized.push(segment),
            std::path::Component::ParentDir => {
                if normalized == workspace {
                    return Err(ToolError::PathEscapesWorkspace {
                        workspace_root: workspace.display().to_string(),
                        path: input.display().to_string(),
                    });
                }
                normalized.pop();
            }
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                return Err(ToolError::InvalidArguments(
                    "path must be workspace-relative or inside the workspace".to_string(),
                ));
            }
        }
    }

    Ok(normalized)
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
        let rel_path = Path::new(ARTIFACTS_DIR_NAME)
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

pub trait ToolResultExt<T> {
    fn tool_err(self, ctx: impl std::fmt::Display) -> Result<T, ToolError>;
}

impl<T, E: std::fmt::Display> ToolResultExt<T> for Result<T, E> {
    fn tool_err(self, ctx: impl std::fmt::Display) -> Result<T, ToolError> {
        self.map_err(|err| ToolError::Execution(format!("{ctx}: {err}")))
    }
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

/// Eliminates the 4-method boilerplate (`id`, `description`, `parameters_json_schema`,
/// `capability`) in `impl Tool for ...` blocks. Usage:
///
/// ```ignore
/// #[async_trait]
/// impl Tool for MyTool {
///     tool_metadata!("my_tool", "description", ToolCapability::EditFs, json!({...}));
///     async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError> { ... }
/// }
/// ```
#[macro_export]
macro_rules! tool_metadata {
    ($id:literal, $desc:literal, $cap:expr, $schema:expr) => {
        fn id(&self) -> &str {
            $id
        }
        fn description(&self) -> &str {
            $desc
        }
        fn parameters_json_schema(&self) -> serde_json::Value {
            $schema
        }
        fn capability(&self) -> $crate::tool::ToolCapability {
            $cap
        }
    };
}

pub fn canonical_tool_id_for(tool_id: &str) -> Option<&str> {
    if tool_id.trim().is_empty() {
        None
    } else {
        Some(tool_id)
    }
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

pub fn sanitize_mcp_tool_segment(name: &str) -> String {
    sanitize_tool_function_name(name).replace('-', "_")
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
    profile_tool_descriptions: BTreeMap<(ToolId, String), String>,
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

    pub fn set_profile_tool_description(
        &mut self,
        tool_id: impl Into<String>,
        profile_name: impl Into<String>,
        description: impl Into<String>,
    ) {
        self.profile_tool_descriptions
            .insert((tool_id.into(), profile_name.into()), description.into());
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

    pub fn description_for_profile(&self, tool_id: &str, profile_name: &str) -> Option<&str> {
        self.profile_tool_descriptions
            .get(&(tool_id.to_string(), profile_name.to_string()))
            .map(String::as_str)
    }

    pub fn tool_ids(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }
}

pub fn actor_capabilities(actor_kind: ActorKind) -> BTreeSet<ToolCapability> {
    match actor_kind {
        ActorKind::Worker | ActorKind::Supervisor | ActorKind::System => BTreeSet::from([
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
        build_tool_function_name_mapping, canonical_tool_id_for, sanitize_mcp_tool_segment,
        sanitize_tool_function_name, ArtifactStore, ArtifactStoreError, ToolCapability,
        ToolContext, ToolError, ToolRegistry, ToolRunState,
    };
    use crate::clock::RealClock;
    use crate::coord::{spawn_coordinator, CoordinatorConfig};
    use crate::event::ActorKind;
    use crate::event::EventActor;
    use crate::redact::DefaultRedactor;
    use crate::UnwrapOrAbort;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;

    fn tool_context(workspace_root: &Path, tool_call_id: &str) -> ToolContext {
        let coordinator = spawn_coordinator(
            CoordinatorConfig::default(),
            Arc::new(RealClock::new()),
            Arc::new(DefaultRedactor::default()),
        );

        ToolContext {
            run_id: "run-tool-tests".into(),
            workspace_root: workspace_root.to_path_buf(),
            artifacts_dir: workspace_root.join("artifacts"),
            actor: EventActor::new(ActorKind::Supervisor, None),
            category: Some("deep".to_string()),
            tool_call_id: tool_call_id.into(),
            current_model_ref: None,
            current_model_settings: None,
            tool_state: ToolRunState::default(),
            external_directory_allow_prefixes: Vec::new(),
            coordinator,
        }
    }

    #[test]
    fn worker_can_use_spawn_agent_capability() {
        let registry = ToolRegistry::new();
        assert!(registry.capability_allowed(ActorKind::Worker, ToolCapability::SpawnAgent));
        assert!(registry.capability_allowed(ActorKind::Worker, ToolCapability::Shell));
    }

    #[test]
    fn artifact_store_write_text_returns_artifact_ref_with_digest() {
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let store = ArtifactStore::new(temp_dir.path().join("artifacts")).unwrap_or_abort();

        let artifact = store
            .write_text("notes/output.txt", "hello artifact")
            .unwrap_or_abort();

        assert_eq!(artifact.path, "artifacts/notes/output.txt");
        assert_eq!(
            artifact.digest.as_deref(),
            Some(blake3::hash(b"hello artifact").to_hex().as_str())
        );
        let written = fs::read_to_string(temp_dir.path().join("artifacts/notes/output.txt"))
            .unwrap_or_abort();
        assert_eq!(written, "hello artifact");
    }

    #[test]
    fn artifact_store_rejects_parent_traversal() {
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let store = ArtifactStore::new(temp_dir.path().join("artifacts")).unwrap_or_abort();

        let err = store
            .write_text("../escape.txt", "blocked")
            .expect_err("traversal must fail");
        assert!(matches!(err, ArtifactStoreError::ParentTraversal(_)));
    }

    #[tokio::test]
    async fn resolve_workspace_path_lexically_normalizes_self_references() {
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let workspace = temp_dir.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap_or_abort();
        let ctx = tool_context(&workspace, "tool-call-1");

        let resolved = ctx
            .resolve_workspace_path(Path::new("missing/.."))
            .unwrap_or_abort();

        assert_eq!(resolved, workspace.canonicalize().unwrap_or_abort());
    }

    #[tokio::test]
    async fn resolve_workspace_path_blocks_parent_traversal_above_workspace() {
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let workspace = temp_dir.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap_or_abort();
        let ctx = tool_context(&workspace, "tool-call-2");

        let err = ctx
            .resolve_workspace_path(Path::new("../escape.txt"))
            .expect_err("parent traversal must be blocked");

        assert!(matches!(err, ToolError::PathEscapesWorkspace { .. }));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn resolve_workspace_path_accepts_absolute_symlink_alias_inside_workspace() {
        let temp_dir = tempfile::tempdir().unwrap_or_abort();
        let workspace = temp_dir.path().join("workspace");
        let nested = workspace.join("nested");
        let alias = temp_dir.path().join("workspace-alias");
        fs::create_dir_all(&nested).unwrap_or_abort();
        std::os::unix::fs::symlink(workspace.canonicalize().unwrap_or_abort(), &alias)
            .unwrap_or_abort();
        let ctx = tool_context(&workspace, "tool-call-symlink");

        let resolved = ctx
            .resolve_workspace_path(&alias.join("nested"))
            .unwrap_or_abort();

        assert_eq!(resolved, nested.canonicalize().unwrap_or_abort());
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
    fn sanitize_mcp_tool_segment_matches_first_class_tool_id_policy() {
        assert_eq!(sanitize_mcp_tool_segment("tool.call"), "tool_call");
        assert_eq!(sanitize_mcp_tool_segment("tools-list"), "tools_list");
        assert_eq!(sanitize_mcp_tool_segment("1server/tool"), "t_1server_tool");
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
    fn canonical_tool_identity_helper_returns_input_for_public_ids() {
        let tool_ids = [
            "read",
            "list",
            "glob",
            "grep",
            "bash",
            "edit",
            "webfetch",
            "todowrite",
            "todoread",
            "skill",
            "question",
            "batch",
            "task",
            "lsp",
            "invalid",
            "websearch",
            "codesearch",
            "lsp.rename",
            "github.issue",
            "github.pull_request",
        ];
        let mapping_a = build_tool_function_name_mapping(tool_ids.iter().copied());

        let mut reversed_tool_ids = tool_ids;
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

        for tool_id in tool_ids {
            assert_eq!(canonical_tool_id_for(tool_id), Some(tool_id));
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
