use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::plugin::{
    InstalledPlugin, PluginActivationPermission, PluginLifecycleError, PluginLifecycleRegistry,
    PluginLifecycleSummary,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PluginLifecycleEvent {
    Installed {
        id: String,
    },
    Activated {
        id: String,
    },
    Deactivated {
        id: String,
    },
    Removed {
        id: String,
    },
    ExecutionStarted {
        id: String,
        operation_id: String,
    },
    ExecutionFinished {
        id: String,
        operation_id: String,
        success: bool,
    },
    Cancelled {
        id: String,
        operation_id: String,
    },
    Upgraded {
        id: String,
    },
}

#[derive(Debug, PartialEq, Eq)]
pub enum PluginRuntimeError {
    Lifecycle(PluginLifecycleError),
    ExecutionSurfaceNotRegistered { id: String },
    OperationCancelled { id: String, operation_id: String },
    ExecutionFailed { id: String, message: String },
    NotEnabledForExecution { id: String },
}

impl fmt::Display for PluginRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lifecycle(e) => write!(f, "{e}"),
            Self::ExecutionSurfaceNotRegistered { id } => {
                write!(f, "plugin `{id}` execution surface not registered")
            }
            Self::OperationCancelled { id, operation_id } => {
                write!(f, "plugin `{id}` operation `{operation_id}` was cancelled")
            }
            Self::ExecutionFailed { id, message } => {
                write!(f, "plugin `{id}` execution failed: {message}")
            }
            Self::NotEnabledForExecution { id } => {
                write!(f, "plugin `{id}` is not enabled; cannot execute")
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

pub trait PluginExecutionSurface: fmt::Debug + Send + Sync {
    fn plugin_id(&self) -> &str;
    fn execute(&self, input: &str) -> Result<String, String>;
}

#[derive(Debug, Clone)]
pub struct HelloWorldPlugin {
    plugin_id: String,
}

impl HelloWorldPlugin {
    pub fn new(plugin_id: impl Into<String>) -> Self {
        Self {
            plugin_id: plugin_id.into(),
        }
    }
}

impl PluginExecutionSurface for HelloWorldPlugin {
    fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    fn execute(&self, input: &str) -> Result<String, String> {
        if input.is_empty() {
            return Err("hello-world plugin requires non-empty input".to_string());
        }
        Ok(format!("hello from {}: {input}", self.plugin_id))
    }
}

#[derive(Debug, Clone)]
pub struct FailingPlugin {
    plugin_id: String,
}

impl FailingPlugin {
    pub fn new(plugin_id: impl Into<String>) -> Self {
        Self {
            plugin_id: plugin_id.into(),
        }
    }
}

impl PluginExecutionSurface for FailingPlugin {
    fn plugin_id(&self) -> &str {
        &self.plugin_id
    }

    fn execute(&self, _input: &str) -> Result<String, String> {
        Err("intentional failure for isolation test".to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginExecutionResult {
    pub plugin_id: String,
    pub operation_id: String,
    pub output: String,
}

pub struct PluginRuntimeContract {
    registry: PluginLifecycleRegistry,
    events: Vec<PluginLifecycleEvent>,
    execution_surfaces: BTreeMap<String, Box<dyn PluginExecutionSurface>>,
    cancelled_operations: BTreeSet<String>,
}

impl PluginRuntimeContract {
    pub fn new(workspace_root: impl Into<std::path::PathBuf>) -> Self {
        Self {
            registry: PluginLifecycleRegistry::new(workspace_root),
            events: Vec::new(),
            execution_surfaces: BTreeMap::new(),
            cancelled_operations: BTreeSet::new(),
        }
    }

    pub fn open(workspace_root: impl Into<std::path::PathBuf>) -> Result<Self, PluginRuntimeError> {
        Ok(Self {
            registry: PluginLifecycleRegistry::open(workspace_root)?,
            events: Vec::new(),
            execution_surfaces: BTreeMap::new(),
            cancelled_operations: BTreeSet::new(),
        })
    }

    pub fn events(&self) -> &[PluginLifecycleEvent] {
        &self.events
    }

    pub fn registry(&self) -> &PluginLifecycleRegistry {
        &self.registry
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

    pub fn register_execution_surface(
        &mut self,
        surface: Box<dyn PluginExecutionSurface>,
    ) -> Result<(), PluginRuntimeError> {
        let id = surface.plugin_id().to_string();
        if self.execution_surfaces.contains_key(&id) {
            return Err(PluginRuntimeError::ExecutionSurfaceNotRegistered { id });
        }
        self.execution_surfaces.insert(id, surface);
        Ok(())
    }

    pub fn cancel_operation(&mut self, operation_id: &str) {
        self.cancelled_operations.insert(operation_id.to_string());
    }

    pub fn execute_plugin(
        &mut self,
        plugin_id: &str,
        operation_id: &str,
        input: &str,
    ) -> Result<PluginExecutionResult, PluginRuntimeError> {
        if !self.registry.is_enabled(plugin_id) {
            return Err(PluginRuntimeError::NotEnabledForExecution {
                id: plugin_id.to_string(),
            });
        }
        if self.cancelled_operations.contains(operation_id) {
            self.events.push(PluginLifecycleEvent::Cancelled {
                id: plugin_id.to_string(),
                operation_id: operation_id.to_string(),
            });
            return Err(PluginRuntimeError::OperationCancelled {
                id: plugin_id.to_string(),
                operation_id: operation_id.to_string(),
            });
        }
        let output = {
            let surface = self.execution_surfaces.get(plugin_id).ok_or(
                PluginRuntimeError::ExecutionSurfaceNotRegistered {
                    id: plugin_id.to_string(),
                },
            )?;
            self.events.push(PluginLifecycleEvent::ExecutionStarted {
                id: plugin_id.to_string(),
                operation_id: operation_id.to_string(),
            });
            surface.execute(input)
        };
        let success = output.is_ok();
        self.events.push(PluginLifecycleEvent::ExecutionFinished {
            id: plugin_id.to_string(),
            operation_id: operation_id.to_string(),
            success,
        });
        output
            .map(|output| PluginExecutionResult {
                plugin_id: plugin_id.to_string(),
                operation_id: operation_id.to_string(),
                output,
            })
            .map_err(|message| PluginRuntimeError::ExecutionFailed {
                id: plugin_id.to_string(),
                message,
            })
    }

    pub fn upgrade_plugin(
        &mut self,
        plugin_id: &str,
        new_package_root: impl AsRef<Path>,
        permission: PluginActivationPermission,
    ) -> Result<&InstalledPlugin, PluginRuntimeError> {
        let old_package_root = self.registry.get(plugin_id).map(|p| p.package_root.clone());
        let was_enabled = self.registry.is_enabled(plugin_id);

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

        if let Err(install_err) = self
            .registry
            .install_from_package_root(new_package_root.as_ref())
        {
            if let Some(old_path) = &old_package_root {
                let _ = self.registry.install_from_package_root(old_path);
                if was_enabled {
                    let _ = self.registry.activate(plugin_id, permission);
                }
            }
            return Err(install_err.into());
        }
        self.events.push(PluginLifecycleEvent::Installed {
            id: plugin_id.to_string(),
        });

        if let Err(activate_err) = self.registry.activate(plugin_id, permission) {
            let _ = self.registry.remove(plugin_id);
            if let Some(old_path) = &old_package_root {
                let _ = self.registry.install_from_package_root(old_path);
                if was_enabled {
                    let _ = self.registry.activate(plugin_id, permission);
                }
            }
            return Err(activate_err.into());
        }
        self.events.push(PluginLifecycleEvent::Activated {
            id: plugin_id.to_string(),
        });
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

    pub fn persist_if_durable(&self) -> Result<(), PluginRuntimeError> {
        Ok(self.registry.persist_if_durable()?)
    }
}
