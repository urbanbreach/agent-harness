//! Explicit fixtures for the preserved TUI diagnostics tests. Never called during runtime startup.
use super::*;
use crate::{extension_manifest::*, extension_registry::*, store};
use std::path::Path;
pub const PROBE_PLUGIN_PRIMARY_ID: &str = "harness.probe.plugin";
pub const PROBE_PLUGIN_SECONDARY_ID: &str = "harness.probe.plugin.secondary";
pub const PROBE_EXTENSION_PRIMARY_ID: &str = "harness.probe.extension";
pub const PROBE_EXTENSION_ALT_ID: &str = "harness.probe.extension.alt";
pub const PROBE_EXTENSION_TOOLS_ID: &str = "harness.probe.extension.tools";
pub const PROBE_ACP_AGENT_NAME: &str = "harness.probe.agent";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiPluginLifecycleProduct {
    pub summary: PluginLifecycleSummary,
    pub last_install: PluginInstallOutcome,
    pub last_activate: PluginActivateOutcome,
    pub last_deactivate: PluginDeactivateOutcome,
    pub last_remove: PluginRemoveOutcome,
    pub first_line: Option<String>,
}
impl MultiPluginLifecycleProduct {
    pub fn meets_multi_plugin_contract(&self) -> bool {
        self.summary.installed >= 2
            && self.summary.enabled >= 1
            && self.summary.disabled >= 1
            && matches!(self.last_install, PluginInstallOutcome::Installed { .. })
            && self.last_activate.loads_code()
            && matches!(
                self.last_deactivate,
                PluginDeactivateOutcome::Deactivated { .. }
            )
            && matches!(self.last_remove, PluginRemoveOutcome::Failed { .. })
    }
}
pub fn run_multi_plugin_lifecycle_product(root: &Path) -> MultiPluginLifecycleProduct {
    let mut registry = PluginLifecycleRegistry::new(root);
    let primary = root.join(".agent-harness/test-plugins/primary");
    let secondary = root.join(".agent-harness/test-plugins/secondary");
    let setup = (|| -> Result<(), Box<dyn std::error::Error>> {
        for (path, id) in [
            (&primary, PROBE_PLUGIN_PRIMARY_ID),
            (&secondary, PROBE_PLUGIN_SECONDARY_ID),
        ] {
            write_fixture(path, id)?;
            store::write_private_atomic(&path.join(PLUGIN_HOOKS_FILE_NAME), b"{}")?;
        }
        registry.install_from_package_root(&primary)?;
        Ok(())
    })();
    if let Err(error) = setup {
        let reason = error.to_string();
        return MultiPluginLifecycleProduct {
            summary: registry.summary(),
            last_install: PluginInstallOutcome::Failed {
                package_root: primary.display().to_string(),
                reason: reason.clone(),
            },
            last_activate: PluginActivateOutcome::Failed {
                id: PROBE_PLUGIN_PRIMARY_ID.into(),
                reason: reason.clone(),
            },
            last_deactivate: PluginDeactivateOutcome::Failed {
                id: PROBE_PLUGIN_SECONDARY_ID.into(),
                reason: reason.clone(),
            },
            last_remove: PluginRemoveOutcome::Failed {
                id: PROBE_PLUGIN_PRIMARY_ID.into(),
                reason,
            },
            first_line: None,
        };
    }
    let last_install = install_plugin_outcome(&mut registry, &secondary);
    let _ = activate_plugin_outcome(
        &mut registry,
        PROBE_PLUGIN_PRIMARY_ID,
        PluginActivationPermission::Granted,
    );
    let last_activate = activate_plugin_outcome(
        &mut registry,
        PROBE_PLUGIN_SECONDARY_ID,
        PluginActivationPermission::Granted,
    );
    let last_deactivate = deactivate_plugin_outcome(&mut registry, PROBE_PLUGIN_SECONDARY_ID);
    let last_remove = remove_plugin_outcome(&mut registry, PROBE_PLUGIN_PRIMARY_ID);
    let first_line = registry.list().next().map(InstalledPlugin::one_line);
    MultiPluginLifecycleProduct {
        summary: registry.summary(),
        last_install,
        last_activate,
        last_deactivate,
        last_remove,
        first_line,
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiDescriptorDiscoverProduct {
    pub discover: ExtensionDiscoverSummary,
    pub registry: ExtensionRegistrySummary,
    pub registry_path: String,
    pub primary: Option<ExtensionManifestSummary>,
    pub last_load: ExtensionLoadOutcome,
    pub discovered_ids: Vec<String>,
}
impl MultiDescriptorDiscoverProduct {
    pub fn meets_multi_descriptor_contract(&self) -> bool {
        self.discover.discovered >= 3
            && self.registry.registered >= 3
            && !self.discover.loads_external_code
            && !self.registry.loads_external_code
            && self.primary.as_ref().is_some_and(|p| {
                p.extension_id == PROBE_EXTENSION_PRIMARY_ID
                    && p.enabled_capabilities > 0
                    && p.tools > 0
                    && !p.loads_external_code
            })
            && matches!(self.last_load, ExtensionLoadOutcome::Loaded { .. })
            && Path::new(&self.registry_path).is_file()
    }
}
pub fn run_multi_descriptor_discover_product(root: &Path) -> MultiDescriptorDiscoverProduct {
    let scan = root.join(".agent-harness/test-extensions");
    let primary_path = scan.join("primary").join(EXTENSION_MANIFEST_FILE_NAME);
    let run = || -> Result<MultiDescriptorDiscoverProduct, Box<dyn std::error::Error>> {
        for (name, id) in [
            ("primary", PROBE_EXTENSION_PRIMARY_ID),
            ("alt", PROBE_EXTENSION_ALT_ID),
            ("tools", PROBE_EXTENSION_TOOLS_ID),
        ] {
            write_fixture(&scan.join(name), id)?;
        }
        let mut registry = ExtensionDescriptorRegistry::open(root)?;
        let discover = registry.discover_and_register(&scan)?;
        Ok(MultiDescriptorDiscoverProduct {
            discover,
            registry: registry.summary(),
            registry_path: registry.registry_path().display().to_string(),
            primary: Some(load_extension_manifest_from_path(&primary_path)?.summary()),
            last_load: load_extension_manifest_outcome(&primary_path),
            discovered_ids: registry
                .list()
                .iter()
                .map(|e| e.extension_id.clone())
                .collect(),
        })
    };
    run().unwrap_or_else(|error| MultiDescriptorDiscoverProduct {
        discover: ExtensionDiscoverSummary::default(),
        registry: ExtensionRegistrySummary::default(),
        registry_path: root.join(EXTENSION_REGISTRY_REL).display().to_string(),
        primary: None,
        last_load: ExtensionLoadOutcome::Failed {
            path: primary_path.display().to_string(),
            reason: error.to_string(),
        },
        discovered_ids: Vec::new(),
    })
}
fn write_fixture(path: &Path, id: &str) -> Result<(), Box<dyn std::error::Error>> {
    store::validate_private_path(path)?;
    store::create_private_dir(path)?;
    let path = path.join(EXTENSION_MANIFEST_FILE_NAME);
    store::validate_private_path(&path)?;
    if !path.try_exists()? {
        let manifest = serde_json::json!({"schemaVersion":EXTENSION_MANIFEST_V1_SCHEMA_VERSION, "id":id, "capabilities":[{"id":"probe.cap"}], "tools":[{"id":"probe.tool", "capabilityId":"probe.cap", "permission":"bash"}]});
        store::write_private_atomic(&path, &serde_json::to_vec(&manifest)?)?;
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MockAcpAgentModeProduct {
    pub fail_connect: AcpConnectOutcome,
    pub fail_bind: AcpBindOutcome,
    pub last_connect: AcpConnectOutcome,
    pub last_bind: AcpBindOutcome,
    pub summary: AcpConnectionSummary,
    pub state: AcpConnectionState,
    pub session: Option<AcpSessionInfo>,
}
impl MockAcpAgentModeProduct {
    pub fn meets_agent_mode_contract(&self) -> bool {
        matches!(self.fail_connect, AcpConnectOutcome::Failed { .. })
            && matches!(self.fail_bind, AcpBindOutcome::Failed { .. })
            && self.last_connect.is_connected()
            && self.last_bind.is_bound()
            && self.summary.is_bound()
            && self
                .session
                .as_ref()
                .is_some_and(|s| s.agent_name == PROBE_ACP_AGENT_NAME)
    }
}
pub fn run_mock_acp_agent_mode_product() -> MockAcpAgentModeProduct {
    let mut failed = AcpConnection::new(MockAcpTransport {
        fail_connect: true,
        ..MockAcpTransport::new()
    });
    let fail_connect = connect_acp_outcome(&mut failed);
    let fail_bind = bind_acp_session_outcome(&mut failed, PROBE_ACP_AGENT_NAME);
    let mut connection = AcpConnection::new(MockAcpTransport::new());
    let last_connect = connect_acp_outcome(&mut connection);
    let last_bind = bind_acp_session_outcome(&mut connection, PROBE_ACP_AGENT_NAME);
    MockAcpAgentModeProduct {
        fail_connect,
        fail_bind,
        last_connect,
        last_bind,
        summary: connection.summary(),
        state: connection.state().clone(),
        session: connection.session().cloned(),
    }
}
