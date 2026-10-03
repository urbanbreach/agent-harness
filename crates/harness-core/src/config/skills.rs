use super::SkillsConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Body-free discovery records. Locations are executable, ephemeral runtime
/// state, not provider-visible metadata or a durable registration payload.
///
/// Finalized catalogs live only in the owning private, verified state artifact.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SkillCatalog {
    pub entries: Vec<SkillCatalogEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillCatalogEntry {
    pub stable_id: String,
    pub name: String,
    pub description: String,
    pub source_scope: String,
    pub root_path: PathBuf,
    pub location: PathBuf,
    pub loadable: bool,
    pub permission_mode: String,
    pub status: SkillCatalogStatus,
    pub reason: Option<String>,
    pub argument_hint: Option<String>,
    pub allowed_tools: Vec<String>,
    pub deferred_mcp: Option<String>,
    pub deferred_resources: Option<String>,
    pub body_loaded: bool,
    pub body_digest: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillCatalogStatus {
    Loadable,
    Denied,
    Disabled,
    Malformed,
    Shadowed,
}

impl SkillCatalogStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Loadable => "loadable",
            Self::Denied => "denied",
            Self::Disabled => "disabled",
            Self::Malformed => "malformed",
            Self::Shadowed => "shadowed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillStartupSnapshot {
    pub config: SkillsConfig,
    pub catalog: SkillCatalog,
}

/// Injected by the native tools/host layer. Called only during live startup,
/// never by event restoration. None preserves the host's unknown trust verdict.
pub trait SkillCatalogDiscovery: Send + Sync {
    fn discover(
        &self,
        cwd: &Path,
        config: &SkillsConfig,
        project_trusted: Option<bool>,
    ) -> Result<SkillCatalog, crate::tool::ToolError>;
}
