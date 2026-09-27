use crate::{extension_manifest::*, store};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
pub const EXTENSION_REGISTRY_REL: &str = ".agent-harness/extension-registry.json";
#[derive(Debug, thiserror::Error)]
pub enum ExtensionRegistryError {
    #[error("extension registry I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid extension registry: {0}")]
    Invalid(&'static str),
    #[error("unsupported extension registry version {version} at {path}")]
    UnsupportedVersion { path: String, version: u32 },
    #[error("invalid extension path `{path}` (empty or escapes workspace)")]
    InvalidPath { path: String },
    #[error("extension manifest load failed at `{path}`: {detail}")]
    ManifestLoad { path: String, detail: String },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionRegistryEntry {
    pub extension_id: String,
    pub manifest_path: String,
    pub capabilities: usize,
    pub enabled_capabilities: usize,
    pub tools: usize,
    pub hooks: usize,
    pub loads_external_code: bool,
    pub registered_at_unix_ms: u64,
}
impl ExtensionRegistryEntry {
    pub fn one_line(&self) -> String {
        format!(
            "extension registry: id=`{}` path=`{}` caps={}/{} tools={} hooks={} loads_code={}",
            self.extension_id,
            self.manifest_path,
            self.enabled_capabilities,
            self.capabilities,
            self.tools,
            self.hooks,
            self.loads_external_code
        )
    }
    pub fn to_summary(&self) -> ExtensionManifestSummary {
        ExtensionManifestSummary {
            extension_id: self.extension_id.clone(),
            display_name: None,
            version: None,
            capabilities: self.capabilities,
            enabled_capabilities: self.enabled_capabilities,
            tools: self.tools,
            hooks: self.hooks,
            commands: 0,
            prompts: 0,
            mcp_bundles: 0,
            diagnostics: 0,
            provider_decorators: 0,
            loads_external_code: false,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionRegistrySummary {
    pub registered: usize,
    pub loads_external_code: bool,
}
impl ExtensionRegistrySummary {
    pub fn one_line(&self) -> String {
        format!(
            "extension registry: {} descriptor(s) (loads_code={})",
            self.registered, self.loads_external_code
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    entries: BTreeMap<String, ExtensionRegistryEntry>,
}
#[derive(Clone, Debug)]
pub struct ExtensionDescriptorRegistry {
    workspace_root: PathBuf,
    registry_path: PathBuf,
    doc: Document,
}
impl ExtensionDescriptorRegistry {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, ExtensionRegistryError> {
        let workspace_root = root.into();
        let registry_path = workspace_root.join(EXTENSION_REGISTRY_REL);
        let doc = load(&workspace_root, &registry_path)?;
        Ok(Self {
            workspace_root,
            registry_path,
            doc,
        })
    }
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }
    pub fn registry_path(&self) -> &Path {
        &self.registry_path
    }
    pub fn summary(&self) -> ExtensionRegistrySummary {
        ExtensionRegistrySummary {
            registered: self.doc.entries.len(),
            loads_external_code: false,
        }
    }
    pub fn list(&self) -> Vec<&ExtensionRegistryEntry> {
        self.doc.entries.values().collect()
    }
    pub fn get(&self, id: &str) -> Option<&ExtensionRegistryEntry> {
        self.doc.entries.get(id)
    }
    pub fn register_manifest_path(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<ExtensionRegistryEntry, ExtensionRegistryError> {
        let entry = self.entry(path.as_ref())?;
        self.commit(std::iter::once(entry.clone()).collect())?;
        Ok(entry)
    }
    pub fn discover_and_register(
        &mut self,
        root: &Path,
    ) -> Result<ExtensionDiscoverSummary, ExtensionRegistryError> {
        let root = resolve(&self.workspace_root, root)?;
        let mut pending = BTreeMap::new();
        for path in manifest_paths(&root) {
            if !path.try_exists()? {
                continue;
            }
            let entry = self.entry(&path)?;
            if pending.insert(entry.extension_id.clone(), entry).is_some() {
                return Err(ExtensionRegistryError::Invalid(
                    "duplicate descriptor id in scan",
                ));
            }
            if pending.len() > 1024 {
                return Err(ExtensionRegistryError::Invalid(
                    "scan exceeds 1024 descriptors",
                ));
            }
        }
        let discovered = pending.len();
        self.commit(pending.into_values().collect())?;
        Ok(ExtensionDiscoverSummary {
            discovered,
            loads_external_code: false,
        })
    }
    fn entry(&self, path: &Path) -> Result<ExtensionRegistryEntry, ExtensionRegistryError> {
        let path = resolve(&self.workspace_root, path)?;
        let root = self.workspace_root.canonicalize()?;
        let relative = path
            .strip_prefix(root)
            .ok()
            .and_then(|p| p.to_str())
            .ok_or_else(|| ExtensionRegistryError::InvalidPath {
                path: path.display().to_string(),
            })?
            .replace('\\', "/");
        valid_relative(&relative)?;
        let manifest = load_extension_manifest_from_path(&path).map_err(|e| {
            ExtensionRegistryError::ManifestLoad {
                path: path.display().to_string(),
                detail: e.to_string(),
            }
        })?;
        let summary = manifest.summary();
        Ok(ExtensionRegistryEntry {
            extension_id: summary.extension_id,
            manifest_path: relative,
            capabilities: summary.capabilities,
            enabled_capabilities: summary.enabled_capabilities,
            tools: summary.tools,
            hooks: summary.hooks,
            loads_external_code: false,
            registered_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|d| u64::try_from(d.as_millis()).ok())
                .unwrap_or(0),
        })
    }
    fn commit(
        &mut self,
        pending: Vec<ExtensionRegistryEntry>,
    ) -> Result<(), ExtensionRegistryError> {
        if pending.is_empty() {
            return Ok(());
        }
        let _lock = store::lock_private_parent(&self.registry_path)?;
        let mut next = load(&self.workspace_root, &self.registry_path)?;
        for entry in pending {
            next.entries.insert(entry.extension_id.clone(), entry);
        }
        if next.entries.len() > 1024 {
            return Err(ExtensionRegistryError::Invalid(
                "registry exceeds 1024 descriptors",
            ));
        }
        let bytes = serde_json::to_vec(&next)
            .map_err(|_| ExtensionRegistryError::Invalid("cannot encode registry"))?;
        if bytes.len() > 2 * 1024 * 1024 {
            return Err(ExtensionRegistryError::Invalid("registry exceeds 2 MiB"));
        }
        store::write_private_atomic(&self.registry_path, &bytes)?;
        self.doc = next;
        Ok(())
    }
}
fn load(root: &Path, path: &Path) -> Result<Document, ExtensionRegistryError> {
    let Some(bytes) = store::read_private_bytes(path, 2 * 1024 * 1024)? else {
        return Ok(Document {
            version: 1,
            entries: BTreeMap::new(),
        });
    };
    let doc: Document = serde_json::from_slice(&bytes)
        .map_err(|_| ExtensionRegistryError::Invalid("invalid registry JSON"))?;
    if doc.version != 1 {
        return Err(ExtensionRegistryError::UnsupportedVersion {
            path: path.display().to_string(),
            version: doc.version,
        });
    }
    if doc.entries.len() > 1024 {
        return Err(ExtensionRegistryError::Invalid(
            "registry exceeds 1024 descriptors",
        ));
    }
    for (id, entry) in &doc.entries {
        stable_id("extensionId", id)
            .map_err(|_| ExtensionRegistryError::Invalid("invalid extension id"))?;
        if id != &entry.extension_id
            || entry.loads_external_code
            || entry.enabled_capabilities > entry.capabilities
            || entry
                .capabilities
                .saturating_add(entry.tools)
                .saturating_add(entry.hooks)
                > 4096
        {
            return Err(ExtensionRegistryError::Invalid(
                "invalid descriptor identity or counts",
            ));
        }
        valid_relative(&entry.manifest_path)?;
        resolve(root, Path::new(&entry.manifest_path))?;
    }
    Ok(doc)
}
fn valid_relative(path: &str) -> Result<(), ExtensionRegistryError> {
    if path.is_empty()
        || path.len() > 4096
        || path.chars().any(char::is_control)
        || path.contains('\\')
        || !Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err(ExtensionRegistryError::InvalidPath { path: path.into() });
    }
    Ok(())
}
fn resolve(root: &Path, path: &Path) -> Result<PathBuf, ExtensionRegistryError> {
    crate::integrations::plugin::resolve_under_workspace(root, path).map_err(|_| {
        ExtensionRegistryError::InvalidPath {
            path: path.display().to_string(),
        }
    })
}
