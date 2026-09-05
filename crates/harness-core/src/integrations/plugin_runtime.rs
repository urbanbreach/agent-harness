use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::plugin::{
    InstalledPlugin, PluginActivationPermission, PluginLifecycleError, PluginLifecycleRegistry,
    PluginLifecycleSummary,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PluginLifecycleEvent {
    Installed { id: String },
    Activated { id: String },
    Deactivated { id: String },
    Removed { id: String },
    Upgraded { id: String },
}

#[derive(Debug, PartialEq, Eq)]
pub enum PluginRuntimeError {
    Lifecycle(PluginLifecycleError),
    UpgradeRollbackFailed {
        id: String,
        original_error: String,
        rollback_error: String,
    },
    UpgradeIdMismatch {
        expected_id: String,
        actual_id: String,
    },
}

impl fmt::Display for PluginRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lifecycle(e) => write!(f, "{e}"),
            Self::UpgradeRollbackFailed {
                id,
                original_error,
                rollback_error,
            } => {
                write!(
                    f,
                    "plugin `{id}` upgrade failed ({original_error}) and rollback also failed ({rollback_error}); system may be in an inconsistent state"
                )
            }
            Self::UpgradeIdMismatch {
                expected_id,
                actual_id,
            } => {
                write!(
                    f,
                    "upgrade failed: expected plugin `{expected_id}` but replacement has id `{actual_id}`"
                )
            }
        }
    }
}

impl std::error::Error for PluginRuntimeError {}

impl From<PluginLifecycleError> for PluginRuntimeError {
    fn from(e: PluginLifecycleError) -> Self {
        Self::Lifecycle(e)
    }
}

pub struct PluginRuntimeContract {
    registry: PluginLifecycleRegistry,
    events: Vec<PluginLifecycleEvent>,
}

impl PluginRuntimeContract {
    pub fn new(workspace_root: impl Into<std::path::PathBuf>) -> Self {
        Self {
            registry: PluginLifecycleRegistry::new(workspace_root),
            events: Vec::new(),
        }
    }

    pub fn open(workspace_root: impl Into<std::path::PathBuf>) -> Result<Self, PluginRuntimeError> {
        Ok(Self {
            registry: PluginLifecycleRegistry::open(workspace_root)?,
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
        package_root: impl AsRef<Path>,
    ) -> Result<&InstalledPlugin, PluginRuntimeError> {
        let plugin_id = {
            let plugin = self.registry.install_from_package_root(package_root)?;
            plugin.id.clone()
        };
        self.events.push(PluginLifecycleEvent::Installed {
            id: plugin_id.clone(),
        });
        self.registry
            .get(&plugin_id)
            .ok_or(PluginLifecycleError::NotInstalled { id: plugin_id }.into())
    }

    pub fn activate(
        &mut self,
        id: &str,
        permission: PluginActivationPermission,
    ) -> Result<&InstalledPlugin, PluginRuntimeError> {
        {
            self.registry.activate(id, permission)?;
        }
        self.events
            .push(PluginLifecycleEvent::Activated { id: id.to_string() });
        self.registry
            .get(id)
            .ok_or(PluginLifecycleError::NotInstalled { id: id.to_string() }.into())
    }

    pub fn deactivate(&mut self, id: &str) -> Result<&InstalledPlugin, PluginRuntimeError> {
        {
            self.registry.deactivate(id)?;
        }
        self.events
            .push(PluginLifecycleEvent::Deactivated { id: id.to_string() });
        self.registry
            .get(id)
            .ok_or(PluginLifecycleError::NotInstalled { id: id.to_string() }.into())
    }

    pub fn remove(&mut self, id: &str) -> Result<InstalledPlugin, PluginRuntimeError> {
        let removed = self.registry.remove(id)?;
        self.events
            .push(PluginLifecycleEvent::Removed { id: id.to_string() });
        Ok(removed)
    }

    pub fn upgrade_plugin(
        &mut self,
        plugin_id: &str,
        new_package_root: impl AsRef<Path>,
        permission: PluginActivationPermission,
    ) -> Result<&InstalledPlugin, PluginRuntimeError> {
        let old_package_root = self.registry.get(plugin_id).map(|p| p.package_root.clone());
        let was_enabled = self.registry.is_enabled(plugin_id);
        let event_checkpoint = self.events.len();

        if was_enabled {
            self.registry.deactivate(plugin_id)?;
            self.events.push(PluginLifecycleEvent::Deactivated {
                id: plugin_id.to_string(),
            });
        }
        self.registry.remove(plugin_id)?;
        self.events.push(PluginLifecycleEvent::Removed {
            id: plugin_id.to_string(),
        });

        let ids_before: BTreeSet<String> = self.registry.list().map(|p| p.id.clone()).collect();
        if let Err(install_err) = self
            .registry
            .install_from_package_root(new_package_root.as_ref())
        {
            self.rollback_upgrade(
                plugin_id,
                &old_package_root,
                was_enabled,
                permission,
                event_checkpoint,
                install_err.to_string(),
            )?;
            return Err(install_err.into());
        }
        let actual_id = self
            .registry
            .list()
            .find(|p| !ids_before.contains(&p.id))
            .map(|p| p.id.clone());
        if actual_id.as_deref() != Some(plugin_id) {
            if let Some(wrong_id) = &actual_id {
                if let Err(remove_err) = self.registry.remove(wrong_id) {
                    let rollback_error = match self.rollback_upgrade(
                        plugin_id,
                        &old_package_root,
                        was_enabled,
                        permission,
                        event_checkpoint,
                        format!("replacement has wrong id: expected {plugin_id}, got {wrong_id}"),
                    ) {
                        Ok(()) => remove_err.to_string(),
                        Err(rollback_err) => {
                            format!("{remove_err}; restoration also failed: {rollback_err}")
                        }
                    };
                    return Err(PluginRuntimeError::UpgradeRollbackFailed {
                        id: plugin_id.to_string(),
                        original_error: format!(
                            "replacement has wrong id: expected {plugin_id}, got {wrong_id}"
                        ),
                        rollback_error,
                    });
                }
            }
            self.rollback_upgrade(
                plugin_id,
                &old_package_root,
                was_enabled,
                permission,
                event_checkpoint,
                format!("replacement has wrong id: expected {plugin_id}"),
            )?;
            return Err(PluginRuntimeError::UpgradeIdMismatch {
                expected_id: plugin_id.to_string(),
                actual_id: actual_id.unwrap_or_default(),
            });
        }
        self.events.push(PluginLifecycleEvent::Installed {
            id: plugin_id.to_string(),
        });

        if was_enabled {
            if let Err(activate_err) = self.registry.activate(plugin_id, permission) {
                if let Err(remove_err) = self.registry.remove(plugin_id) {
                    let rollback_error = match self.rollback_upgrade(
                        plugin_id,
                        &old_package_root,
                        was_enabled,
                        permission,
                        event_checkpoint,
                        activate_err.to_string(),
                    ) {
                        Ok(()) => remove_err.to_string(),
                        Err(rollback_err) => {
                            format!("{remove_err}; restoration also failed: {rollback_err}")
                        }
                    };
                    return Err(PluginRuntimeError::UpgradeRollbackFailed {
                        id: plugin_id.to_string(),
                        original_error: activate_err.to_string(),
                        rollback_error,
                    });
                }
                self.rollback_upgrade(
                    plugin_id,
                    &old_package_root,
                    was_enabled,
                    permission,
                    event_checkpoint,
                    activate_err.to_string(),
                )?;
                return Err(activate_err.into());
            }
            self.events.push(PluginLifecycleEvent::Activated {
                id: plugin_id.to_string(),
            });
        }
        self.events.push(PluginLifecycleEvent::Upgraded {
            id: plugin_id.to_string(),
        });

        self.registry.get(plugin_id).ok_or(
            PluginLifecycleError::NotInstalled {
                id: plugin_id.to_string(),
            }
            .into(),
        )
    }

    fn rollback_upgrade(
        &mut self,
        plugin_id: &str,
        old_package_root: &Option<PathBuf>,
        was_enabled: bool,
        permission: PluginActivationPermission,
        event_checkpoint: usize,
        original_error: String,
    ) -> Result<(), PluginRuntimeError> {
        self.events.truncate(event_checkpoint);
        if let Some(old_path) = old_package_root {
            if let Err(rollback_err) = self.registry.install_from_package_root(old_path) {
                return Err(PluginRuntimeError::UpgradeRollbackFailed {
                    id: plugin_id.to_string(),
                    original_error,
                    rollback_error: rollback_err.to_string(),
                });
            }
            self.events.push(PluginLifecycleEvent::Installed {
                id: plugin_id.to_string(),
            });
            if was_enabled {
                if let Err(rollback_err) = self.registry.activate(plugin_id, permission) {
                    return Err(PluginRuntimeError::UpgradeRollbackFailed {
                        id: plugin_id.to_string(),
                        original_error,
                        rollback_error: rollback_err.to_string(),
                    });
                }
                self.events.push(PluginLifecycleEvent::Activated {
                    id: plugin_id.to_string(),
                });
            }
        }
        Ok(())
    }

    pub fn persist_if_durable(&self) -> Result<(), PluginRuntimeError> {
        Ok(self.registry.persist_if_durable()?)
    }
}
