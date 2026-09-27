use super::plugin_load::{self, LoadedCode};
use crate::{extension_manifest::*, store};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
mod outcomes;
mod storage;
pub use super::plugin_load::{
    LoadedCode as PluginLoadedCode, PluginLoadError, PluginLoadKind, PLUGIN_ENTRY_FILE_NAME,
    PLUGIN_HOOKS_FILE_NAME, PLUGIN_LOAD_RECEIPT_FILE_NAME, PLUGIN_SKILLS_DIR_NAME,
};
pub use outcomes::*;
pub const PLUGIN_MANIFEST_FILE_NAME: &str = EXTENSION_MANIFEST_FILE_NAME;
pub const PLUGIN_REGISTRY_REL: &str = ".agent-harness/plugins.json";
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginActivationPermission {
    Granted,
    Denied,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginEnablement {
    Disabled,
    Enabled,
}
impl PluginEnablement {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Enabled => "enabled",
        }
    }
    pub fn is_enabled(self) -> bool {
        self == Self::Enabled
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstalledPlugin {
    pub id: String,
    pub package_root: PathBuf,
    pub manifest: ExtensionManifestV1,
    pub enablement: PluginEnablement,
    pub loaded: Option<LoadedCode>,
}
impl InstalledPlugin {
    pub fn loads_code(&self) -> bool {
        self.loaded.as_ref().is_some_and(LoadedCode::loads_code)
    }
    pub fn one_line(&self) -> String {
        format!(
            "plugin `{}` enablement={} root=`{}` (loads_code={})",
            self.id,
            self.enablement.as_str(),
            self.package_root.display(),
            self.loads_code()
        )
    }
}
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PluginLifecycleError {
    #[error("plugin package escapes workspace (workspace={workspace_root}, path={path})")]
    PathEscapesWorkspace {
        workspace_root: String,
        path: String,
    },
    #[error("plugin root is not a directory: {path}")]
    PackageRootNotDirectory { path: String },
    #[error("plugin manifest missing at {path}")]
    ManifestNotFound { path: String },
    #[error("plugin manifest invalid at {path}: {source}")]
    ManifestInvalid {
        path: String,
        source: ExtensionManifestError,
    },
    #[error("cannot read plugin manifest at {path}: {message}")]
    ManifestRead { path: String, message: String },
    #[error("plugin `{id}` is already installed")]
    AlreadyInstalled { id: String },
    #[error("plugin `{id}` is not installed")]
    NotInstalled { id: String },
    #[error("plugin activation denied for `{id}`")]
    ActivationDenied { id: String },
    #[error("plugin `{id}` is already enabled")]
    AlreadyEnabled { id: String },
    #[error("plugin `{id}` is not enabled")]
    NotEnabled { id: String },
    #[error("plugin `{id}` is enabled; deactivate before removal")]
    RemoveWhileEnabled { id: String },
    #[error("workspace unavailable at {path}: {message}")]
    WorkspaceRootUnavailable { path: String, message: String },
    #[error("plugin `{id}` package load failed: {source}")]
    PackageLoadFailed { id: String, source: PluginLoadError },
    #[error("plugin registry I/O error at {path}: {message}")]
    RegistryIo { path: String, message: String },
    #[error("plugin registry encoding error at {path}: {message}")]
    RegistrySerialize { path: String, message: String },
    #[error("replacement plugin id `{actual}` does not match `{expected}`")]
    UpgradeIdMismatch { expected: String, actual: String },
}
#[derive(Clone, Debug, Default)]
pub struct PluginLifecycleRegistry {
    workspace_root: PathBuf,
    packages: BTreeMap<String, InstalledPlugin>,
    persist_path: Option<PathBuf>,
}
impl PluginLifecycleRegistry {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: root.into(),
            ..Self::default()
        }
    }
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, PluginLifecycleError> {
        let root = root.into();
        let path = root.join(PLUGIN_REGISTRY_REL);
        let packages = storage::load(&root, &path)?;
        Ok(Self {
            workspace_root: root,
            packages,
            persist_path: Some(path),
        })
    }
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }
    pub fn registry_path(&self) -> Option<&Path> {
        self.persist_path.as_deref()
    }
    pub fn len(&self) -> usize {
        self.packages.len()
    }
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }
    pub fn get(&self, id: &str) -> Option<&InstalledPlugin> {
        self.packages.get(id)
    }
    pub fn is_enabled(&self, id: &str) -> bool {
        self.get(id).is_some_and(|p| p.enablement.is_enabled())
    }
    pub fn list(&self) -> impl Iterator<Item = &InstalledPlugin> {
        self.packages.values()
    }
    pub fn summary(&self) -> PluginLifecycleSummary {
        let enabled = self
            .packages
            .values()
            .filter(|p| p.enablement.is_enabled())
            .count();
        PluginLifecycleSummary {
            installed: self.len(),
            enabled,
            disabled: self.len() - enabled,
        }
    }
    /// Mutations already persist. Refuse to overwrite a newer writer's snapshot.
    pub fn persist_if_durable(&self) -> Result<(), PluginLifecycleError> {
        if let Some(path) = &self.persist_path {
            let _lock = store::lock_private_parent(path)
                .map_err(|e| storage::io_error(path, &e.to_string()))?;
            let current = storage::load(&self.workspace_root, path)?;
            if current != self.packages {
                return Err(storage::io_error(
                    path,
                    "registry changed since it was loaded",
                ));
            }
        }
        Ok(())
    }
    pub fn install_from_package_root(
        &mut self,
        package_root: impl AsRef<Path>,
    ) -> Result<&InstalledPlugin, PluginLifecycleError> {
        let package = inspect(&self.workspace_root, package_root.as_ref())?;
        let id = package.id.clone();
        self.update(|packages| {
            if packages.contains_key(&package.id) {
                return Err(PluginLifecycleError::AlreadyInstalled { id: package.id });
            }
            packages.insert(package.id.clone(), package);
            Ok(())
        })?;
        self.require(&id)
    }
    pub fn activate(
        &mut self,
        id: &str,
        permission: PluginActivationPermission,
    ) -> Result<&InstalledPlugin, PluginLifecycleError> {
        permitted(id, permission)?;
        let root = self.workspace_root.clone();
        self.update(|packages| {
            let plugin = packages.get_mut(id).ok_or_else(|| missing(id))?;
            if plugin.enablement.is_enabled() {
                return Err(PluginLifecycleError::AlreadyEnabled { id: id.into() });
            }
            let current = inspect(&root, &plugin.package_root)?;
            if current.id != id {
                return Err(PluginLifecycleError::UpgradeIdMismatch {
                    expected: id.into(),
                    actual: current.id,
                });
            }
            plugin.manifest = current.manifest;
            plugin.loaded =
                plugin_load::inspect_package(&plugin.package_root).map_err(|source| {
                    PluginLifecycleError::PackageLoadFailed {
                        id: id.into(),
                        source,
                    }
                })?;
            plugin.enablement = PluginEnablement::Enabled;
            Ok(())
        })?;
        self.require(id)
    }
    pub fn deactivate(&mut self, id: &str) -> Result<&InstalledPlugin, PluginLifecycleError> {
        self.update(|packages| {
            let plugin = packages.get_mut(id).ok_or_else(|| missing(id))?;
            if !plugin.enablement.is_enabled() {
                return Err(PluginLifecycleError::NotEnabled { id: id.into() });
            }
            plugin.enablement = PluginEnablement::Disabled;
            plugin.loaded = None;
            Ok(())
        })?;
        self.require(id)
    }
    pub fn remove(&mut self, id: &str) -> Result<InstalledPlugin, PluginLifecycleError> {
        self.update(|packages| {
            if packages
                .get(id)
                .ok_or_else(|| missing(id))?
                .enablement
                .is_enabled()
            {
                return Err(PluginLifecycleError::RemoveWhileEnabled { id: id.into() });
            }
            packages.remove(id).ok_or_else(|| missing(id))
        })
    }
    pub(super) fn upgrade(
        &mut self,
        id: &str,
        path: &Path,
        permission: PluginActivationPermission,
    ) -> Result<&InstalledPlugin, PluginLifecycleError> {
        let mut replacement = inspect(&self.workspace_root, path)?;
        if replacement.id != id {
            return Err(PluginLifecycleError::UpgradeIdMismatch {
                expected: id.into(),
                actual: replacement.id,
            });
        }
        self.update(|packages| {
            let old = packages.get(id).ok_or_else(|| missing(id))?;
            if old.enablement.is_enabled() {
                permitted(id, permission)?;
                replacement.loaded = plugin_load::inspect_package(&replacement.package_root)
                    .map_err(|source| PluginLifecycleError::PackageLoadFailed {
                        id: id.into(),
                        source,
                    })?;
                replacement.enablement = PluginEnablement::Enabled;
            }
            packages.insert(id.into(), replacement);
            Ok(())
        })?;
        self.require(id)
    }
    fn require(&self, id: &str) -> Result<&InstalledPlugin, PluginLifecycleError> {
        self.get(id).ok_or_else(|| missing(id))
    }
    fn update<T>(
        &mut self,
        change: impl FnOnce(&mut BTreeMap<String, InstalledPlugin>) -> Result<T, PluginLifecycleError>,
    ) -> Result<T, PluginLifecycleError> {
        let _lock = self
            .persist_path
            .as_ref()
            .map(|p| {
                store::lock_private_parent(p).map_err(|e| storage::io_error(p, &e.to_string()))
            })
            .transpose()?;
        let current = match &self.persist_path {
            Some(path) => storage::load(&self.workspace_root, path)?,
            None => self.packages.clone(),
        };
        let mut next = current.clone();
        let result = change(&mut next)?;
        storage::commit(self.persist_path.as_deref(), &current, &next)?;
        self.packages = next;
        Ok(result)
    }
}
pub(crate) fn resolve_under_workspace(
    root: &Path,
    path: &Path,
) -> Result<PathBuf, PluginLifecycleError> {
    let root = root
        .canonicalize()
        .map_err(|e| PluginLifecycleError::WorkspaceRootUnavailable {
            path: root.display().to_string(),
            message: e.to_string(),
        })?;
    let joined = root.join(path);
    store::validate_private_path(&joined)
        .map_err(|e| storage::io_error(&joined, &e.to_string()))?;
    let resolved = crate::tool::resolve_file_path(&root, path).map_err(|_| {
        PluginLifecycleError::PackageRootNotDirectory {
            path: joined.display().to_string(),
        }
    })?;
    if !resolved.starts_with(&root) {
        return Err(PluginLifecycleError::PathEscapesWorkspace {
            workspace_root: root.display().to_string(),
            path: resolved.display().to_string(),
        });
    }
    Ok(resolved)
}
fn inspect(root: &Path, path: &Path) -> Result<InstalledPlugin, PluginLifecycleError> {
    let root = resolve_under_workspace(root, path)?;
    if !root.is_dir() {
        return Err(PluginLifecycleError::PackageRootNotDirectory {
            path: root.display().to_string(),
        });
    }
    let path = root.join(PLUGIN_MANIFEST_FILE_NAME);
    let manifest = load_extension_manifest_from_path(&path).map_err(|source| {
        PluginLifecycleError::ManifestInvalid {
            path: path.display().to_string(),
            source,
        }
    })?;
    Ok(InstalledPlugin {
        id: manifest.id.clone(),
        package_root: root,
        manifest,
        enablement: PluginEnablement::Disabled,
        loaded: None,
    })
}
fn missing(id: &str) -> PluginLifecycleError {
    PluginLifecycleError::NotInstalled { id: id.into() }
}
fn permitted(id: &str, permission: PluginActivationPermission) -> Result<(), PluginLifecycleError> {
    if permission == PluginActivationPermission::Granted {
        Ok(())
    } else {
        Err(PluginLifecycleError::ActivationDenied { id: id.into() })
    }
}
