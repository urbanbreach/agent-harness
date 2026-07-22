//! Plugin package lifecycle: install / activate / deactivate / remove.
//!
//! Install validates an [`ExtensionManifestV1`] under a workspace-scoped root.
//! Activation may load package entries (`plugin_entry.json`, `hooks.json`,
//! `skills/`) and writes a load receipt — not dynamic `.so`/wasm execution.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::extension_manifest::{ExtensionManifestError, ExtensionManifestV1};

use super::plugin_load::{
    clear_package_load_receipt, load_package_entries, LoadedCode, PluginLoadError,
};

/// Canonical relative filename for a plugin package descriptor.
pub const PLUGIN_MANIFEST_FILE_NAME: &str = "extension.manifest.json";

/// Relative durable registry journal under a workspace.
pub const PLUGIN_REGISTRY_REL: &str = ".agent-harness/plugins.json";

const PLUGIN_REGISTRY_SCHEMA_VERSION: &str = "harness-plugin-registry.v1";

pub use super::plugin_load::{
    LoadedCode as PluginLoadedCode, PluginLoadKind, PLUGIN_ENTRY_FILE_NAME, PLUGIN_HOOKS_FILE_NAME,
    PLUGIN_LOAD_RECEIPT_FILE_NAME, PLUGIN_SKILLS_DIR_NAME,
};

/// Operator/coordinator permission gate required before activation.
///
/// Activation is a side-effecting registry mutation. Callers must resolve
/// permission first; this module never auto-grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginActivationPermission {
    Granted,
    Denied,
}

/// Whether an installed plugin package is currently enabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginEnablement {
    Disabled,
    Enabled,
}

impl PluginEnablement {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Enabled => "enabled",
        }
    }

    pub const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

/// A validated, registry-tracked plugin package.
///
/// Serializable so a durable registry can persist membership, enablement, and
/// loaded-entry metadata across operator invocations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPlugin {
    pub id: String,
    pub package_root: PathBuf,
    pub manifest: ExtensionManifestV1,
    pub enablement: PluginEnablement,
    /// Present when activation loaded package entries and wrote a receipt.
    pub loaded: Option<LoadedCode>,
}

impl InstalledPlugin {
    /// Operator-facing one-line diagnostics (`loads_code` reflects real package load).
    pub fn one_line(&self) -> String {
        format!(
            "plugin `{}` enablement={} root=`{}` (loads_code={})",
            self.id,
            self.enablement.as_str(),
            self.package_root.display(),
            self.loads_code()
        )
    }

    pub fn loads_code(&self) -> bool {
        self.loaded.as_ref().is_some_and(LoadedCode::loads_code)
    }
}

/// Fail-closed plugin lifecycle errors.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PluginLifecycleError {
    #[error(
        "plugin package path escapes workspace root (workspace={workspace_root}, path={path})"
    )]
    PathEscapesWorkspace {
        workspace_root: String,
        path: String,
    },
    #[error("plugin package root is not a directory: {path}")]
    PackageRootNotDirectory { path: String },
    #[error("plugin manifest missing at {path}")]
    ManifestNotFound { path: String },
    #[error("plugin manifest invalid at {path}: {source}")]
    ManifestInvalid {
        path: String,
        source: ExtensionManifestError,
    },
    #[error("failed to read plugin manifest at {path}: {message}")]
    ManifestRead { path: String, message: String },
    #[error("plugin `{id}` is already installed")]
    AlreadyInstalled { id: String },
    #[error("plugin `{id}` is not installed")]
    NotInstalled { id: String },
    #[error("plugin activation denied for `{id}` (permission-before-execution)")]
    ActivationDenied { id: String },
    #[error("plugin `{id}` is already enabled")]
    AlreadyEnabled { id: String },
    #[error("plugin `{id}` is not enabled")]
    NotEnabled { id: String },
    #[error("plugin `{id}` is enabled; deactivate before remove")]
    RemoveWhileEnabled { id: String },
    #[error("workspace root unavailable at {path}: {message}")]
    WorkspaceRootUnavailable { path: String, message: String },
    #[error("plugin `{id}` package load failed: {source}")]
    PackageLoadFailed { id: String, source: PluginLoadError },
    #[error("plugin registry io error at {path}: {message}")]
    RegistryIo { path: String, message: String },
    #[error("plugin registry serialize error at {path}: {message}")]
    RegistrySerialize { path: String, message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedPluginRegistry {
    schema_version: String,
    plugins: Vec<InstalledPlugin>,
}

/// In-memory coordinator-owned registry of installed plugin packages.
///
/// Install validates path + descriptor before registration. On any validation
/// failure the registry is left unchanged (no partial / stale entries).
///
/// [`Self::new`] is in-memory only (no filesystem writes), for coordinator/run
/// ownership; [`Self::open`] is durable and persists each successful mutation to
/// [`PLUGIN_REGISTRY_REL`] so an operator CLI lifecycle survives across processes.
#[derive(Debug, Clone, Default)]
pub struct PluginLifecycleRegistry {
    workspace_root: PathBuf,
    packages: BTreeMap<String, InstalledPlugin>,
    persist_path: Option<PathBuf>,
}

impl PluginLifecycleRegistry {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            packages: BTreeMap::new(),
            persist_path: None,
        }
    }

    /// Open a durable registry rooted at `workspace_root`.
    ///
    /// Loads any persisted membership from [`PLUGIN_REGISTRY_REL`] and persists
    /// each successful mutation. Reload trusts the persisted record (members were
    /// validated at install time); descriptors are not re-read on load.
    pub fn open(workspace_root: impl Into<PathBuf>) -> Result<Self, PluginLifecycleError> {
        let workspace_root = workspace_root.into();
        let persist_path = workspace_root.join(PLUGIN_REGISTRY_REL);
        let mut registry = Self {
            workspace_root,
            packages: BTreeMap::new(),
            persist_path: Some(persist_path.clone()),
        };
        if persist_path.is_file() {
            let body = fs::read_to_string(&persist_path).map_err(|err| {
                PluginLifecycleError::RegistryIo {
                    path: persist_path.display().to_string(),
                    message: err.to_string(),
                }
            })?;
            let persisted: PersistedPluginRegistry =
                serde_json::from_str(&body).map_err(|err| {
                    PluginLifecycleError::RegistrySerialize {
                        path: persist_path.display().to_string(),
                        message: err.to_string(),
                    }
                })?;
            for plugin in persisted.plugins {
                registry.packages.insert(plugin.id.clone(), plugin);
            }
        }
        Ok(registry)
    }

    /// Durable journal path when opened durably; `None` for in-memory registries.
    pub fn registry_path(&self) -> Option<&Path> {
        self.persist_path.as_deref()
    }

    fn persist_if_durable(&self) -> Result<(), PluginLifecycleError> {
        let Some(path) = &self.persist_path else {
            return Ok(());
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| PluginLifecycleError::RegistryIo {
                path: parent.display().to_string(),
                message: err.to_string(),
            })?;
        }
        let persisted = PersistedPluginRegistry {
            schema_version: PLUGIN_REGISTRY_SCHEMA_VERSION.to_string(),
            plugins: self.packages.values().cloned().collect(),
        };
        let body = serde_json::to_string_pretty(&persisted).map_err(|err| {
            PluginLifecycleError::RegistrySerialize {
                path: path.display().to_string(),
                message: err.to_string(),
            }
        })?;
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, format!("{body}\n")).map_err(|err| PluginLifecycleError::RegistryIo {
            path: temp.display().to_string(),
            message: err.to_string(),
        })?;
        fs::rename(&temp, path).map_err(|err| PluginLifecycleError::RegistryIo {
            path: path.display().to_string(),
            message: err.to_string(),
        })?;
        Ok(())
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
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
        self.packages
            .get(id)
            .is_some_and(|plugin| plugin.enablement.is_enabled())
    }

    pub fn list(&self) -> impl Iterator<Item = &InstalledPlugin> {
        self.packages.values()
    }

    /// Install a plugin package by validating its descriptor under the workspace.
    ///
    /// Does **not** activate the package. Does **not** execute package code.
    /// On failure, no registration is retained.
    pub fn install_from_package_root(
        &mut self,
        package_root: impl AsRef<Path>,
    ) -> Result<&InstalledPlugin, PluginLifecycleError> {
        let package_root =
            super::path::resolve_under_workspace(&self.workspace_root, package_root.as_ref())?;
        if !package_root.is_dir() {
            return Err(PluginLifecycleError::PackageRootNotDirectory {
                path: package_root.display().to_string(),
            });
        }

        let manifest_path = package_root.join(PLUGIN_MANIFEST_FILE_NAME);
        let raw = fs::read_to_string(&manifest_path).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                PluginLifecycleError::ManifestNotFound {
                    path: manifest_path.display().to_string(),
                }
            } else {
                PluginLifecycleError::ManifestRead {
                    path: manifest_path.display().to_string(),
                    message: err.to_string(),
                }
            }
        })?;

        let manifest = ExtensionManifestV1::parse_json(&raw).map_err(|source| {
            PluginLifecycleError::ManifestInvalid {
                path: manifest_path.display().to_string(),
                source,
            }
        })?;

        let id = manifest.id.clone();
        if self.packages.contains_key(&id) {
            return Err(PluginLifecycleError::AlreadyInstalled { id });
        }

        // Insert only after full validation succeeds (fail closed / no partial).
        self.packages.insert(
            id.clone(),
            InstalledPlugin {
                id: id.clone(),
                package_root,
                manifest,
                enablement: PluginEnablement::Disabled,
                loaded: None,
            },
        );
        self.persist_if_durable()?;

        self.packages
            .get(&id)
            .ok_or(PluginLifecycleError::NotInstalled { id })
    }

    /// Enable an installed plugin after permission is granted.
    ///
    /// When the package root has loadable entries, activation loads them and
    /// writes a receipt under the package root before marking enabled.
    /// Fail-closed: invalid/missing declared entries leave the plugin disabled.
    pub fn activate(
        &mut self,
        id: &str,
        permission: PluginActivationPermission,
    ) -> Result<&InstalledPlugin, PluginLifecycleError> {
        if !matches!(permission, PluginActivationPermission::Granted) {
            return Err(PluginLifecycleError::ActivationDenied { id: id.to_string() });
        }
        let package_root = {
            let plugin = self
                .packages
                .get(id)
                .ok_or_else(|| PluginLifecycleError::NotInstalled { id: id.to_string() })?;
            if plugin.enablement.is_enabled() {
                return Err(PluginLifecycleError::AlreadyEnabled { id: id.to_string() });
            }
            plugin.package_root.clone()
        };

        let loaded = load_package_entries(&package_root, id).map_err(|source| {
            PluginLifecycleError::PackageLoadFailed {
                id: id.to_string(),
                source,
            }
        })?;

        {
            let plugin = self
                .packages
                .get_mut(id)
                .ok_or_else(|| PluginLifecycleError::NotInstalled { id: id.to_string() })?;
            plugin.loaded = loaded;
            plugin.enablement = PluginEnablement::Enabled;
        }
        self.persist_if_durable()?;
        self.packages
            .get(id)
            .ok_or_else(|| PluginLifecycleError::NotInstalled { id: id.to_string() })
    }

    /// Disable an enabled plugin without removing its registration.
    ///
    /// Clears loaded package state and removes the load receipt when present.
    pub fn deactivate(&mut self, id: &str) -> Result<&InstalledPlugin, PluginLifecycleError> {
        let package_root = {
            let plugin = self
                .packages
                .get_mut(id)
                .ok_or_else(|| PluginLifecycleError::NotInstalled { id: id.to_string() })?;
            if !plugin.enablement.is_enabled() {
                return Err(PluginLifecycleError::NotEnabled { id: id.to_string() });
            }
            plugin.package_root.clone()
        };
        clear_package_load_receipt(&package_root);
        {
            let plugin = self
                .packages
                .get_mut(id)
                .ok_or_else(|| PluginLifecycleError::NotInstalled { id: id.to_string() })?;
            plugin.loaded = None;
            plugin.enablement = PluginEnablement::Disabled;
        }
        self.persist_if_durable()?;
        self.packages
            .get(id)
            .ok_or_else(|| PluginLifecycleError::NotInstalled { id: id.to_string() })
    }

    /// Remove a disabled plugin registration.
    ///
    /// Enabled plugins must be deactivated first (fail closed; no silent drop).
    pub fn remove(&mut self, id: &str) -> Result<InstalledPlugin, PluginLifecycleError> {
        let enabled = match self.packages.get(id) {
            Some(plugin) => plugin.enablement.is_enabled(),
            None => {
                return Err(PluginLifecycleError::NotInstalled { id: id.to_string() });
            }
        };
        if enabled {
            return Err(PluginLifecycleError::RemoveWhileEnabled { id: id.to_string() });
        }
        let removed = self
            .packages
            .remove(id)
            .ok_or_else(|| PluginLifecycleError::NotInstalled { id: id.to_string() })?;
        self.persist_if_durable()?;
        Ok(removed)
    }

    /// Operator-facing counts for installed packages (diagnostics only).
    pub fn summary(&self) -> PluginLifecycleSummary {
        let mut summary = PluginLifecycleSummary {
            installed: self.packages.len(),
            ..PluginLifecycleSummary::default()
        };
        for plugin in self.packages.values() {
            if plugin.enablement.is_enabled() {
                summary.enabled = summary.enabled.saturating_add(1);
            } else {
                summary.disabled = summary.disabled.saturating_add(1);
            }
        }
        summary
    }
}

/// Result of a single plugin install attempt (diagnostics only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum PluginInstallOutcome {
    Installed {
        id: String,
        package_root: String,
    },
    Failed {
        package_root: String,
        reason: String,
    },
}

impl PluginInstallOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Installed { id, package_root } => {
                format!("plugin install: ok id=`{id}` root=`{package_root}` (loads_code=false)")
            }
            Self::Failed {
                package_root,
                reason,
            } => format!("plugin install: failed root=`{package_root}` ({reason})"),
        }
    }
}

/// Install a plugin package and return a structured operator-facing outcome.
pub fn install_plugin_outcome(
    registry: &mut PluginLifecycleRegistry,
    package_root: impl AsRef<Path>,
) -> PluginInstallOutcome {
    let root_display = package_root.as_ref().display().to_string();
    match registry.install_from_package_root(package_root) {
        Ok(plugin) => PluginInstallOutcome::Installed {
            id: plugin.id.clone(),
            package_root: plugin.package_root.display().to_string(),
        },
        Err(err) => PluginInstallOutcome::Failed {
            package_root: root_display,
            reason: err.to_string(),
        },
    }
}

/// Result of a single plugin activate attempt (diagnostics only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum PluginActivateOutcome {
    Activated { id: String, loads_code: bool },
    Failed { id: String, reason: String },
}

impl PluginActivateOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Activated { id, loads_code } => {
                format!("plugin activate: ok id=`{id}` (loads_code={loads_code})")
            }
            Self::Failed { id, reason } => {
                format!("plugin activate: failed id=`{id}` ({reason})")
            }
        }
    }

    pub fn loads_code(&self) -> bool {
        matches!(
            self,
            Self::Activated {
                loads_code: true,
                ..
            }
        )
    }
}

/// Activate a plugin package and return a structured operator-facing outcome.
pub fn activate_plugin_outcome(
    registry: &mut PluginLifecycleRegistry,
    id: &str,
    permission: PluginActivationPermission,
) -> PluginActivateOutcome {
    match registry.activate(id, permission) {
        Ok(plugin) => PluginActivateOutcome::Activated {
            id: plugin.id.clone(),
            loads_code: plugin.loads_code(),
        },
        Err(err) => PluginActivateOutcome::Failed {
            id: id.to_string(),
            reason: err.to_string(),
        },
    }
}

/// Result of a single plugin deactivate attempt (diagnostics only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum PluginDeactivateOutcome {
    Deactivated { id: String },
    Failed { id: String, reason: String },
}

impl PluginDeactivateOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Deactivated { id } => {
                format!("plugin deactivate: ok id=`{id}` (loads_code=false)")
            }
            Self::Failed { id, reason } => {
                format!("plugin deactivate: failed id=`{id}` ({reason})")
            }
        }
    }
}

/// Deactivate a plugin package and return a structured operator-facing outcome.
pub fn deactivate_plugin_outcome(
    registry: &mut PluginLifecycleRegistry,
    id: &str,
) -> PluginDeactivateOutcome {
    match registry.deactivate(id) {
        Ok(plugin) => PluginDeactivateOutcome::Deactivated {
            id: plugin.id.clone(),
        },
        Err(err) => PluginDeactivateOutcome::Failed {
            id: id.to_string(),
            reason: err.to_string(),
        },
    }
}

/// Result of a single plugin remove attempt (diagnostics only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum PluginRemoveOutcome {
    Removed { id: String },
    Failed { id: String, reason: String },
}

impl PluginRemoveOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Removed { id } => {
                format!("plugin remove: ok id=`{id}` (loads_code=false)")
            }
            Self::Failed { id, reason } => {
                format!("plugin remove: failed id=`{id}` ({reason})")
            }
        }
    }
}

/// Remove a plugin package registration and return a structured operator-facing outcome.
pub fn remove_plugin_outcome(
    registry: &mut PluginLifecycleRegistry,
    id: &str,
) -> PluginRemoveOutcome {
    match registry.remove(id) {
        Ok(plugin) => PluginRemoveOutcome::Removed { id: plugin.id },
        Err(err) => PluginRemoveOutcome::Failed {
            id: id.to_string(),
            reason: err.to_string(),
        },
    }
}

/// Operator-facing counts for a plugin lifecycle registry (diagnostics only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PluginLifecycleSummary {
    pub installed: usize,
    pub enabled: usize,
    pub disabled: usize,
}

impl PluginLifecycleSummary {
    pub fn one_line(&self) -> String {
        format!(
            "plugins: {} installed ({} enabled, {} disabled)",
            self.installed, self.enabled, self.disabled
        )
    }

    pub const fn has_enabled(&self) -> bool {
        self.enabled > 0
    }
}
