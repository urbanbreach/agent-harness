//! Coordinator-owned integration lifecycle foundation (plugins + ACP).
//!
//! # Honest MVP bounds
//!
//! - **Plugins:** validated descriptor packages + enable/disable; activation may
//!   load package entries (`hooks.json` / `skills/` / `plugin_entry.json`) and
//!   write a load receipt. No dynamic `.so`/wasm execution, marketplace, or remote install.
//! - **ACP:** connection state machine with a stubbable transport trait.
//!   Full ACP protocol / IDE transports are out of scope for this module.
//!
//! Permission-before-execution applies to plugin activation. Fail closed on
//! invalid descriptors, invalid package entries, and path escapes.

pub mod acp;
pub mod acp_file;
mod path;
pub mod plugin;
mod plugin_load;
pub mod product;

pub use acp::{
    bind_acp_session_outcome, connect_acp_outcome, AcpBindOutcome, AcpConnectOutcome,
    AcpConnection, AcpConnectionState, AcpConnectionSummary, AcpError, AcpSessionInfo,
    AcpTransport, MockAcpTransport,
};
pub use acp_file::{
    run_file_acp_agent_mode_product, FileAcpAgentModeProduct, FileAcpTransport,
    ACP_CONNECTED_MARKER, ACP_FILE_TRANSPORT_REL, ACP_FRAME_LOG,
};
pub use plugin::{
    activate_plugin_outcome, deactivate_plugin_outcome, install_plugin_outcome,
    remove_plugin_outcome, InstalledPlugin, PluginActivateOutcome, PluginActivationPermission,
    PluginDeactivateOutcome, PluginEnablement, PluginInstallOutcome, PluginLifecycleError,
    PluginLifecycleRegistry, PluginLifecycleSummary, PluginLoadKind, PluginLoadedCode,
    PluginRemoveOutcome, PLUGIN_ENTRY_FILE_NAME, PLUGIN_HOOKS_FILE_NAME,
    PLUGIN_LOAD_RECEIPT_FILE_NAME, PLUGIN_MANIFEST_FILE_NAME, PLUGIN_REGISTRY_REL,
    PLUGIN_SKILLS_DIR_NAME,
};
pub use product::{
    run_mock_acp_agent_mode_product, run_multi_descriptor_discover_product,
    run_multi_plugin_lifecycle_product, MockAcpAgentModeProduct, MultiDescriptorDiscoverProduct,
    MultiPluginLifecycleProduct, PROBE_ACP_AGENT_NAME, PROBE_EXTENSION_ALT_ID,
    PROBE_EXTENSION_PRIMARY_ID, PROBE_EXTENSION_TOOLS_ID, PROBE_PLUGIN_PRIMARY_ID,
    PROBE_PLUGIN_SECONDARY_ID,
};
