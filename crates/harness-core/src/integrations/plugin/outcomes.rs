use super::*;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
pub fn install_plugin_outcome(
    registry: &mut PluginLifecycleRegistry,
    path: impl AsRef<Path>,
) -> PluginInstallOutcome {
    match registry.install_from_package_root(&path) {
        Ok(p) => PluginInstallOutcome::Installed {
            id: p.id.clone(),
            package_root: p.package_root.display().to_string(),
        },
        Err(e) => PluginInstallOutcome::Failed {
            package_root: path.as_ref().display().to_string(),
            reason: e.to_string(),
        },
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
            Self::Failed { id, reason } => format!("plugin activate: failed id=`{id}` ({reason})"),
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
pub fn activate_plugin_outcome(
    registry: &mut PluginLifecycleRegistry,
    id: &str,
    permission: PluginActivationPermission,
) -> PluginActivateOutcome {
    match registry.activate(id, permission) {
        Ok(p) => PluginActivateOutcome::Activated {
            id: p.id.clone(),
            loads_code: p.loads_code(),
        },
        Err(e) => PluginActivateOutcome::Failed {
            id: id.into(),
            reason: e.to_string(),
        },
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
pub fn deactivate_plugin_outcome(
    registry: &mut PluginLifecycleRegistry,
    id: &str,
) -> PluginDeactivateOutcome {
    match registry.deactivate(id) {
        Ok(p) => PluginDeactivateOutcome::Deactivated { id: p.id.clone() },
        Err(e) => PluginDeactivateOutcome::Failed {
            id: id.into(),
            reason: e.to_string(),
        },
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum PluginRemoveOutcome {
    Removed { id: String },
    Failed { id: String, reason: String },
}
impl PluginRemoveOutcome {
    pub fn one_line(&self) -> String {
        match self {
            Self::Removed { id } => format!("plugin remove: ok id=`{id}` (loads_code=false)"),
            Self::Failed { id, reason } => format!("plugin remove: failed id=`{id}` ({reason})"),
        }
    }
}
pub fn remove_plugin_outcome(
    registry: &mut PluginLifecycleRegistry,
    id: &str,
) -> PluginRemoveOutcome {
    match registry.remove(id) {
        Ok(p) => PluginRemoveOutcome::Removed { id: p.id },
        Err(e) => PluginRemoveOutcome::Failed {
            id: id.into(),
            reason: e.to_string(),
        },
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    pub fn has_enabled(&self) -> bool {
        self.enabled > 0
    }
}
