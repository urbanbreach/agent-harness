use super::plugin::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum PluginLifecycleEvent {
    Installed { id: String },
    Activated { id: String },
    Deactivated { id: String },
    Removed { id: String },
    Upgraded { id: String },
}
#[derive(Debug, thiserror::Error)]
pub enum PluginRuntimeError {
    #[error(transparent)]
    Lifecycle(#[from] PluginLifecycleError),
    #[error("upgrade rollback failed for `{id}`: {original_error}; {rollback_error}")]
    UpgradeRollbackFailed {
        id: String,
        original_error: String,
        rollback_error: String,
    },
    #[error("replacement id `{actual_id}` does not match `{expected_id}`")]
    UpgradeIdMismatch {
        expected_id: String,
        actual_id: String,
    },
}
pub struct PluginRuntimeContract {
    registry: PluginLifecycleRegistry,
    events: Vec<PluginLifecycleEvent>,
}
impl PluginRuntimeContract {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            registry: PluginLifecycleRegistry::new(root),
            events: Vec::new(),
        }
    }
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, PluginRuntimeError> {
        Ok(Self {
            registry: PluginLifecycleRegistry::open(root)?,
            events: Vec::new(),
        })
    }
    pub fn events(&self) -> &[PluginLifecycleEvent] {
        &self.events
    }
    pub fn registry(&self) -> &PluginLifecycleRegistry {
        &self.registry
    }
    pub fn registry_mut(&mut self) -> &mut PluginLifecycleRegistry {
        &mut self.registry
    }
    pub fn get(&self, id: &str) -> Option<&InstalledPlugin> {
        self.registry.get(id)
    }
    pub fn is_enabled(&self, id: &str) -> bool {
        self.registry.is_enabled(id)
    }
    pub fn list(&self) -> impl Iterator<Item = &InstalledPlugin> {
        self.registry.list()
    }
    pub fn len(&self) -> usize {
        self.registry.len()
    }
    pub fn is_empty(&self) -> bool {
        self.registry.is_empty()
    }
    pub fn summary(&self) -> PluginLifecycleSummary {
        self.registry.summary()
    }
    pub fn install_from_package_root(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<&InstalledPlugin, PluginRuntimeError> {
        let id = self.registry.install_from_package_root(path)?.id.clone();
        self.record(PluginLifecycleEvent::Installed { id: id.clone() });
        self.installed(&id)
    }
    pub fn activate(
        &mut self,
        id: &str,
        permission: PluginActivationPermission,
    ) -> Result<&InstalledPlugin, PluginRuntimeError> {
        self.registry.activate(id, permission)?;
        self.record(PluginLifecycleEvent::Activated { id: id.into() });
        self.installed(id)
    }
    pub fn deactivate(&mut self, id: &str) -> Result<&InstalledPlugin, PluginRuntimeError> {
        self.registry.deactivate(id)?;
        self.record(PluginLifecycleEvent::Deactivated { id: id.into() });
        self.installed(id)
    }
    pub fn remove(&mut self, id: &str) -> Result<InstalledPlugin, PluginRuntimeError> {
        let removed = self.registry.remove(id)?;
        self.record(PluginLifecycleEvent::Removed { id: id.into() });
        Ok(removed)
    }
    pub fn upgrade_plugin(
        &mut self,
        id: &str,
        path: impl AsRef<Path>,
        permission: PluginActivationPermission,
    ) -> Result<&InstalledPlugin, PluginRuntimeError> {
        self.registry
            .upgrade(id, path.as_ref(), permission)
            .map_err(|e| match e {
                PluginLifecycleError::UpgradeIdMismatch { expected, actual } => {
                    PluginRuntimeError::UpgradeIdMismatch {
                        expected_id: expected,
                        actual_id: actual,
                    }
                }
                e => e.into(),
            })?;
        self.record(PluginLifecycleEvent::Upgraded { id: id.into() });
        self.installed(id)
    }
    pub fn persist_if_durable(&self) -> Result<(), PluginRuntimeError> {
        Ok(self.registry.persist_if_durable()?)
    }
    fn installed(&self, id: &str) -> Result<&InstalledPlugin, PluginRuntimeError> {
        self.registry
            .get(id)
            .ok_or_else(|| PluginLifecycleError::NotInstalled { id: id.into() }.into())
    }
    fn record(&mut self, event: PluginLifecycleEvent) {
        // ponytail: diagnostics retain the last 256 transitions; the registry is the durable source.
        if self.events.len() == 256 {
            self.events.remove(0);
        }
        self.events.push(event);
    }
}
