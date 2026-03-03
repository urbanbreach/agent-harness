use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::event::{ActorKind, EventActor};

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
    pub tool_call_id: String,
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

    fn capability(&self) -> ToolCapability;

    async fn call(&self, ctx: ToolContext, args_json: Value) -> Result<ToolResult, ToolError>;
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
}

pub fn actor_capabilities(actor_kind: ActorKind) -> BTreeSet<ToolCapability> {
    match actor_kind {
        ActorKind::Worker => BTreeSet::from([
            ToolCapability::ReadFs,
            ToolCapability::EditFs,
            ToolCapability::Shell,
            ToolCapability::Network,
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
    use super::{ArtifactStore, ArtifactStoreError, ToolCapability, ToolRegistry};
    use crate::event::ActorKind;
    use std::fs;

    #[test]
    fn worker_cannot_use_spawn_agent_capability() {
        let registry = ToolRegistry::new();
        assert!(!registry.capability_allowed(ActorKind::Worker, ToolCapability::SpawnAgent));
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
}
