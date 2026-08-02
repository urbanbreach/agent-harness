//! Product orchestration for plugin lifecycle, multi-descriptor discover, and mock ACP.
//!
//! First-party operator/product surfaces (not TUI seed glue). Plugin probes include
//! loadable package entries so activate writes a load receipt (`loads_code=true`).
//! Residual: no dynamic `.so`/wasm execution. ACP uses mock transport only.

use std::fs;
use std::path::Path;

use crate::extension_manifest::{
    load_extension_manifest_outcome, ExtensionDiscoverSummary, ExtensionLoadOutcome,
    ExtensionManifestSummary, EXTENSION_MANIFEST_FILE_NAME, EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
};
use crate::extension_registry::{
    ExtensionDescriptorRegistry, ExtensionRegistrySummary, EXTENSION_REGISTRY_REL,
};

use super::acp::{
    bind_acp_session_outcome, connect_acp_outcome, AcpBindOutcome, AcpConnectOutcome,
    AcpConnection, AcpConnectionState, AcpConnectionSummary, AcpSessionInfo, MockAcpTransport,
};
use super::plugin::{
    activate_plugin_outcome, deactivate_plugin_outcome, install_plugin_outcome,
    remove_plugin_outcome, PluginActivateOutcome, PluginActivationPermission,
    PluginDeactivateOutcome, PluginInstallOutcome, PluginLifecycleSummary, PluginRemoveOutcome,
    PLUGIN_HOOKS_FILE_NAME, PLUGIN_MANIFEST_FILE_NAME, PLUGIN_SKILLS_DIR_NAME,
};
use super::plugin_runtime::PluginRuntimeContract;

/// Canonical multi-plugin probe package ids (product contract).
pub const PROBE_PLUGIN_PRIMARY_ID: &str = "harness.probe.plugin";
pub const PROBE_PLUGIN_SECONDARY_ID: &str = "harness.probe.plugin.secondary";

/// Canonical multi-descriptor extension probe ids (product contract).
pub const PROBE_EXTENSION_PRIMARY_ID: &str = "harness.probe.extension";
pub const PROBE_EXTENSION_ALT_ID: &str = "harness.probe.extension.alt";
pub const PROBE_EXTENSION_TOOLS_ID: &str = "harness.probe.extension.tools";

/// Canonical mock ACP agent name after successful bind.
pub const PROBE_ACP_AGENT_NAME: &str = "harness.probe.agent";

/// Multi-plugin install → activate → deactivate → remove-missing product result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiPluginLifecycleProduct {
    pub summary: PluginLifecycleSummary,
    pub last_install: PluginInstallOutcome,
    pub last_activate: PluginActivateOutcome,
    pub last_deactivate: PluginDeactivateOutcome,
    pub last_remove: PluginRemoveOutcome,
    pub first_line: Option<String>,
}

impl MultiPluginLifecycleProduct {
    /// Product honesty: multi-plugin lifecycle with real package-entry load on activate.
    pub fn meets_multi_plugin_contract(&self) -> bool {
        self.summary.installed >= 2
            && self.summary.enabled >= 1
            && self.summary.disabled >= 1
            && matches!(self.last_install, PluginInstallOutcome::Installed { .. })
            && matches!(
                self.last_activate,
                PluginActivateOutcome::Activated {
                    loads_code: true,
                    ..
                }
            )
            && matches!(
                self.last_deactivate,
                PluginDeactivateOutcome::Deactivated { .. }
            )
            && matches!(self.last_remove, PluginRemoveOutcome::Failed { .. })
            && self
                .first_line
                .as_deref()
                .is_some_and(|line| line.contains("loads_code=true"))
    }
}

/// Ensure probe package dirs exist, then run multi-plugin lifecycle product path.
pub fn run_multi_plugin_lifecycle_product(workspace_root: &Path) -> MultiPluginLifecycleProduct {
    let primary_pkg = workspace_root.join(".harness-plugin-probe");
    let secondary_pkg = workspace_root.join(".harness-plugin-probe-2");
    write_plugin_probe_manifest(
        &primary_pkg,
        PROBE_PLUGIN_PRIMARY_ID,
        "Harness Probe Plugin",
    );
    write_plugin_probe_manifest(
        &secondary_pkg,
        PROBE_PLUGIN_SECONDARY_ID,
        "Harness Probe Plugin Secondary",
    );

    let mut contract = PluginRuntimeContract::new(workspace_root);
    let _ = install_plugin_outcome(contract.registry_mut(), &primary_pkg);
    let last_install = install_plugin_outcome(contract.registry_mut(), &secondary_pkg);
    let _ = activate_plugin_outcome(
        contract.registry_mut(),
        PROBE_PLUGIN_PRIMARY_ID,
        PluginActivationPermission::Granted,
    );
    let last_activate = activate_plugin_outcome(
        contract.registry_mut(),
        PROBE_PLUGIN_SECONDARY_ID,
        PluginActivationPermission::Granted,
    );
    let last_deactivate =
        deactivate_plugin_outcome(contract.registry_mut(), PROBE_PLUGIN_SECONDARY_ID);
    let last_remove = remove_plugin_outcome(contract.registry_mut(), "(missing-remove-probe)");
    let first_line = contract.list().next().map(|plugin| plugin.one_line());
    MultiPluginLifecycleProduct {
        summary: contract.summary(),
        last_install,
        last_activate,
        last_deactivate,
        last_remove,
        first_line,
    }
}

/// Multi-descriptor workspace discover product result (descriptor-only + durable registry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiDescriptorDiscoverProduct {
    pub discover: ExtensionDiscoverSummary,
    pub registry: ExtensionRegistrySummary,
    pub registry_path: String,
    pub primary: Option<ExtensionManifestSummary>,
    pub last_load: ExtensionLoadOutcome,
    pub discovered_ids: Vec<String>,
}

impl MultiDescriptorDiscoverProduct {
    /// Product honesty: ≥3 descriptors registered durably, primary caps+tools, no code load.
    pub fn meets_multi_descriptor_contract(&self) -> bool {
        let primary_ok = self.primary.as_ref().is_some_and(|summary| {
            summary.extension_id == PROBE_EXTENSION_PRIMARY_ID
                && summary.capabilities >= 1
                && summary.enabled_capabilities >= 1
                && summary.tools >= 1
                && !summary.loads_external_code
        });
        self.discover.discovered >= 3
            && self.registry.registered >= 3
            && !self.discover.loads_external_code
            && !self.registry.loads_external_code
            && primary_ok
            && matches!(self.last_load, ExtensionLoadOutcome::Loaded { .. })
            && Path::new(&self.registry_path).is_file()
    }
}

/// Write three extension probes, discover into durable registry, bind primary + load outcome.
pub fn run_multi_descriptor_discover_product(
    workspace_root: &Path,
) -> MultiDescriptorDiscoverProduct {
    let primary_dir = workspace_root.join(".harness-extension-probe");
    let alt_dir = workspace_root.join(".harness-extension-probe-2");
    let tools_dir = workspace_root.join(".harness-extension-probe-3");
    let primary_path = primary_dir.join(EXTENSION_MANIFEST_FILE_NAME);
    write_extension_probe(
        &primary_dir,
        &format!(
            r#"{{"schemaVersion":"{schema}","id":"{id}","displayName":"Harness Probe Extension","version":"0.0.0-probe","capabilities":[{{"id":"probe.cap","defaultEnabled":true}}],"tools":[{{"id":"probe.tool","capabilityId":"probe.cap","permission":"bash"}}]}}"#,
            schema = EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
            id = PROBE_EXTENSION_PRIMARY_ID,
        ),
    );
    write_extension_probe(
        &alt_dir,
        &format!(
            r#"{{"schemaVersion":"{schema}","id":"{id}","displayName":"Harness Probe Extension Alt","version":"0.0.0-probe-2","capabilities":[{{"id":"probe.cap.alt","defaultEnabled":false}}],"hooks":[{{"id":"probe.hook","capabilityId":"probe.cap.alt","lifecycleEvent":"run_started","status":"native"}}]}}"#,
            schema = EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
            id = PROBE_EXTENSION_ALT_ID,
        ),
    );
    write_extension_probe(
        &tools_dir,
        &format!(
            r#"{{"schemaVersion":"{schema}","id":"{id}","displayName":"Harness Probe Extension Tools","version":"0.0.0-probe-3","capabilities":[{{"id":"probe.cap.tools","defaultEnabled":true}}],"tools":[{{"id":"probe.tool.read","capabilityId":"probe.cap.tools","permission":"edit"}},{{"id":"probe.tool.list","capabilityId":"probe.cap.tools","permission":"bash"}}]}}"#,
            schema = EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
            id = PROBE_EXTENSION_TOOLS_ID,
        ),
    );

    let mut registry = match ExtensionDescriptorRegistry::open(workspace_root) {
        Ok(registry) => registry,
        Err(_) => {
            return MultiDescriptorDiscoverProduct {
                discover: ExtensionDiscoverSummary {
                    discovered: 0,
                    loads_external_code: false,
                },
                registry: ExtensionRegistrySummary::default(),
                registry_path: workspace_root
                    .join(EXTENSION_REGISTRY_REL)
                    .display()
                    .to_string(),
                primary: None,
                last_load: ExtensionLoadOutcome::Failed {
                    path: primary_path.display().to_string(),
                    reason: "extension registry open failed".to_string(),
                },
                discovered_ids: Vec::new(),
            };
        }
    };
    let discover =
        registry
            .discover_and_register(workspace_root)
            .unwrap_or(ExtensionDiscoverSummary {
                discovered: 0,
                loads_external_code: false,
            });
    let reloaded = ExtensionDescriptorRegistry::open(workspace_root).unwrap_or(registry);
    let registry_summary = reloaded.summary();
    let primary = reloaded
        .get(PROBE_EXTENSION_PRIMARY_ID)
        .map(|entry| entry.to_summary())
        .or_else(|| reloaded.list().first().map(|entry| entry.to_summary()));
    let last_load = load_extension_manifest_outcome(&primary_path);
    let discovered_ids = reloaded
        .list()
        .into_iter()
        .map(|entry| entry.extension_id.clone())
        .collect();
    MultiDescriptorDiscoverProduct {
        discover,
        registry: registry_summary,
        registry_path: reloaded.registry_path().display().to_string(),
        primary,
        last_load,
        discovered_ids,
    }
}

/// Multi-path mock ACP connect/bind product result (fail-then-success).
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Product honesty: fail path exercised; last outcomes are success + bound session.
    pub fn meets_agent_mode_contract(&self) -> bool {
        matches!(self.fail_connect, AcpConnectOutcome::Failed { .. })
            && matches!(self.fail_bind, AcpBindOutcome::Failed { .. })
            && self.last_connect.is_connected()
            && self.last_bind.is_bound()
            && self.summary.is_bound()
            && self
                .session
                .as_ref()
                .is_some_and(|s| s.agent_name == PROBE_ACP_AGENT_NAME && !s.session_id.is_empty())
    }
}

/// Fail-connect transport path, then success MockAcpTransport connect + bind.
pub fn run_mock_acp_agent_mode_product() -> MockAcpAgentModeProduct {
    let mut fail_transport = MockAcpTransport::new();
    fail_transport.fail_connect = true;
    fail_transport.fail_connect_reason = "probe-connect-denied".to_string();
    let mut fail_acp = AcpConnection::new(fail_transport);
    let fail_connect = connect_acp_outcome(&mut fail_acp);
    let fail_bind = bind_acp_session_outcome(&mut fail_acp, "harness.probe.agent.fail");

    let transport = MockAcpTransport::new();
    let mut acp = AcpConnection::new(transport);
    let last_connect = connect_acp_outcome(&mut acp);
    let last_bind = bind_acp_session_outcome(&mut acp, PROBE_ACP_AGENT_NAME);
    MockAcpAgentModeProduct {
        fail_connect,
        fail_bind,
        last_connect,
        last_bind,
        summary: acp.summary(),
        state: acp.state().clone(),
        session: acp.session().cloned(),
    }
}

fn write_plugin_probe_manifest(package: &Path, id: &str, display: &str) {
    let _ = fs::create_dir_all(package);
    let path = package.join(PLUGIN_MANIFEST_FILE_NAME);
    if !path.is_file() {
        let body = format!(
            r#"{{"schemaVersion":"{schema}","id":"{id}","displayName":"{display}","version":"0.0.0-probe"}}"#,
            schema = EXTENSION_MANIFEST_V1_SCHEMA_VERSION,
            id = id,
            display = display,
        );
        let _ = fs::write(path, body);
    }
    // Loadable package entry so product activate exercises real load + receipt.
    let hooks = package.join(PLUGIN_HOOKS_FILE_NAME);
    if !hooks.is_file() {
        let _ = fs::write(
            &hooks,
            r#"{"hooks":[{"id":"probe.on_start","event":"run_started"}]}"#,
        );
    }
    let skill = package.join(PLUGIN_SKILLS_DIR_NAME).join("probe-skill");
    if !skill.join("SKILL.md").is_file() {
        let _ = fs::create_dir_all(&skill);
        let _ = fs::write(skill.join("SKILL.md"), "# probe skill\n");
    }
}

fn write_extension_probe(dir: &Path, body: &str) {
    let _ = fs::create_dir_all(dir);
    let path = dir.join(EXTENSION_MANIFEST_FILE_NAME);
    let _ = fs::write(path, body);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_workspace(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "harness-integrations-{}-{}-{}",
            label,
            std::process::id(),
            nanos
        ));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn multi_plugin_lifecycle_loads_code_and_writes_receipt() {
        // arrange
        let root = temp_workspace("plugin-life");
        // act
        let product = run_multi_plugin_lifecycle_product(&root);
        // assert
        assert!(
            product.meets_multi_plugin_contract(),
            "expected multi-plugin contract: {product:?}"
        );
        assert!(product.last_activate.loads_code());
        let receipt = root
            .join(".harness-plugin-probe-2")
            .join(super::super::plugin::PLUGIN_LOAD_RECEIPT_FILE_NAME);
        // secondary was deactivated — receipt cleared; primary still enabled with receipt
        let primary_receipt = root
            .join(".harness-plugin-probe")
            .join(super::super::plugin::PLUGIN_LOAD_RECEIPT_FILE_NAME);
        assert!(
            primary_receipt.is_file(),
            "expected load receipt at {}",
            primary_receipt.display()
        );
        let body = fs::read_to_string(&primary_receipt).expect("read receipt");
        assert!(
            body.contains("plugin.load.receipt")
                || body.contains("entrypoints")
                || body.contains("hooks")
                || body.contains(PROBE_PLUGIN_PRIMARY_ID),
            "receipt body={body}"
        );
        let _ = receipt;
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn multi_descriptor_discover_finds_three_probes() {
        // arrange
        let root = temp_workspace("ext-discover");
        // act
        let product = run_multi_descriptor_discover_product(&root);
        // assert
        assert!(
            product.meets_multi_descriptor_contract(),
            "expected multi-descriptor contract: {product:?}"
        );
        assert!(product.discover.discovered >= 3);
        assert!(!product.discover.loads_external_code);
        let primary = root
            .join(".harness-extension-probe")
            .join(EXTENSION_MANIFEST_FILE_NAME);
        assert!(primary.is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn mock_acp_agent_mode_product_meets_contract() {
        // arrange
        // act
        let product = run_mock_acp_agent_mode_product();
        // assert
        assert!(
            product.meets_agent_mode_contract(),
            "expected mock acp contract: {product:?}"
        );
    }
}
