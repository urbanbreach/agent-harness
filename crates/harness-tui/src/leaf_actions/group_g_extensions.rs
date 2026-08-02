//! Leaf action contract for Group G (extensions/plugins/MCP/settings) — Todo 26.
//!
//! Names the real backend owner for extension capabilities and defines the
//! typed action surface. Wiring into shared Action/keybinding/slash registries
//! is reserved for Todo 28; this module is a plain value contract with no
//! app-state or registry dependency.
//!
//! Capabilities covered:
//! - `tui.extensions_plugins_ui` — TUI management surface for plugins/extensions
//! - Plugin permission/execution, MCP registration, settings/privacy

/// Availability of a leaf action backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActionAvailability {
    #[default]
    Unwired,
    Available,
    Unavailable(&'static str),
}

/// Input validation result for a leaf action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputValidation {
    Valid,
    Invalid(&'static str),
}

/// Resolution outcome for a leaf action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeafActionResolution {
    pub capability_id: &'static str,
    pub backend_owner: &'static str,
    pub availability: ActionAvailability,
    pub replay_safe: bool,
}

/// Group identifier for aggregator wiring (Todo 28).
pub const GROUP_ID: &str = "G";

/// Primary backend owner for extension capabilities.
pub const BACKEND_OWNER: &str = "crates/harness-tui/src/app.rs";

/// Capability IDs covered by this group.
pub const CAPABILITY_IDS: &[&str] = &["tui.extensions_plugins_ui"];

/// Typed leaf action for extensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExtensionAction {
    #[default]
    None,
    /// Open the extensions/plugins panel.
    OpenExtensionsPanel,
    /// Toggle a plugin on or off.
    TogglePlugin,
    /// Show a plugin permission prompt.
    PluginPermissionPrompt,
    /// Execute a plugin action.
    ExecutePlugin,
    /// Register an MCP server.
    McpRegistration,
    /// Open settings panel.
    OpenSettings,
    /// Open privacy panel.
    OpenPrivacy,
}

/// Plugin permission state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PluginPermissionState {
    #[default]
    NotRequested,
    Pending,
    Granted,
    Denied,
}

/// Resolve a capability ID to its backend owner and availability.
pub fn resolve(capability_id: &str) -> Option<LeafActionResolution> {
    let (owner, available) = match capability_id {
        "tui.extensions_plugins_ui" => {
            ("crates/harness-tui/src/app.rs", ActionAvailability::Unwired)
        }
        _ => return None,
    };
    Some(LeafActionResolution {
        capability_id: capability_id_to_static(capability_id)?,
        backend_owner: owner,
        availability: available,
        replay_safe: false,
    })
}

/// Validate input for an extension action.
pub fn validate_input(action: ExtensionAction, input: &str) -> InputValidation {
    match action {
        ExtensionAction::None => InputValidation::Invalid("action is None"),
        ExtensionAction::OpenExtensionsPanel
        | ExtensionAction::OpenSettings
        | ExtensionAction::OpenPrivacy => {
            if input.is_empty() {
                InputValidation::Valid
            } else {
                InputValidation::Invalid("panel open takes no input")
            }
        }
        ExtensionAction::TogglePlugin | ExtensionAction::ExecutePlugin => {
            if input.is_empty() {
                InputValidation::Invalid("plugin action requires a plugin name")
            } else if input.len() > 256 {
                InputValidation::Invalid("plugin name exceeds 256 chars")
            } else {
                InputValidation::Valid
            }
        }
        ExtensionAction::PluginPermissionPrompt => {
            if input.is_empty() {
                InputValidation::Invalid("permission prompt requires a plugin name")
            } else {
                InputValidation::Valid
            }
        }
        ExtensionAction::McpRegistration => {
            if input.is_empty() {
                InputValidation::Invalid("MCP registration requires a server name")
            } else if input.len() > 256 {
                InputValidation::Invalid("server name exceeds 256 chars")
            } else {
                InputValidation::Valid
            }
        }
    }
}

/// Check whether an action is safe during replay.
pub fn is_replay_safe(action: ExtensionAction) -> bool {
    matches!(
        action,
        ExtensionAction::None
            | ExtensionAction::OpenExtensionsPanel
            | ExtensionAction::OpenSettings
            | ExtensionAction::OpenPrivacy
    )
}

/// Return the group identifier.
pub fn group_id() -> &'static str {
    GROUP_ID
}

/// Return the capability IDs for this group.
pub fn capability_ids() -> &'static [&'static str] {
    CAPABILITY_IDS
}

fn capability_id_to_static(id: &str) -> Option<&'static str> {
    CAPABILITY_IDS.iter().find(|c| **c == id).copied()
}
