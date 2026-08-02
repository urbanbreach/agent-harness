//! Leaf action contract for Group C (minimal/screen/compact/expand) — Todo 26.
//!
//! Names the real backend owner for screen mode capabilities and defines the
//! typed action surface. Wiring into shared Action/keybinding/slash registries
//! is reserved for Todo 28; this module is a plain value contract with no
//! app-state or registry dependency.
//!
//! Capabilities covered:
//! - `tui.minimal_mode` — reduced chrome, `--minimal` flag wiring
//! - Compact/expand transcript and chrome toggles

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
pub const GROUP_ID: &str = "C";

/// Primary backend owner for screen mode capabilities.
pub const BACKEND_OWNER: &str = "crates/harness-tui/src/app.rs";

/// Capability IDs covered by this group.
pub const CAPABILITY_IDS: &[&str] = &["tui.minimal_mode"];

/// Typed leaf action for screen modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenModeAction {
    #[default]
    None,
    /// Toggle minimal mode (reduced chrome).
    ToggleMinimal,
    /// Toggle compact transcript layout.
    ToggleCompact,
    /// Expand the current view (e.g. full-screen overlay).
    Expand,
    /// Collapse an expanded view back to default.
    Collapse,
}

/// Screen mode state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenMode {
    #[default]
    Normal,
    Minimal,
    Compact,
    Expanded,
}

/// Resolve a capability ID to its backend owner and availability.
pub fn resolve(capability_id: &str) -> Option<LeafActionResolution> {
    let (owner, available) = match capability_id {
        "tui.minimal_mode" => ("crates/harness-tui/src/app.rs", ActionAvailability::Unwired),
        _ => return None,
    };
    Some(LeafActionResolution {
        capability_id: capability_id_to_static(capability_id)?,
        backend_owner: owner,
        availability: available,
        replay_safe: true,
    })
}

/// Validate input for a screen mode action.
pub fn validate_input(action: ScreenModeAction, input: &str) -> InputValidation {
    match action {
        ScreenModeAction::None => InputValidation::Invalid("action is None"),
        ScreenModeAction::ToggleMinimal
        | ScreenModeAction::ToggleCompact
        | ScreenModeAction::Expand
        | ScreenModeAction::Collapse => {
            if input.is_empty() {
                InputValidation::Valid
            } else {
                InputValidation::Invalid("screen mode toggle takes no input")
            }
        }
    }
}

/// Check whether an action is safe during replay.
pub fn is_replay_safe(action: ScreenModeAction) -> bool {
    matches!(
        action,
        ScreenModeAction::None
            | ScreenModeAction::ToggleMinimal
            | ScreenModeAction::ToggleCompact
            | ScreenModeAction::Expand
            | ScreenModeAction::Collapse
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
