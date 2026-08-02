//! Leaf action contract for Group I (theme/terminal/mouse/timestamps/debug) — Todo 26.
//!
//! Names the real backend owner for preference capabilities and defines the
//! typed action surface. Wiring into shared Action/keybinding/slash registries
//! is reserved for Todo 28; this module is a plain value contract with no
//! app-state or registry dependency.
//!
//! Capabilities covered:
//! - `tui.theme_auto_system` — auto/system theme selection
//! - `tui.themes` — named theme switching
//! - Terminal fallback, mouse, timestamps, effort/personas/debug/always-approve/auto

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
pub const GROUP_ID: &str = "I";

/// Capability IDs covered by this group.
pub const CAPABILITY_IDS: &[&str] = &["tui.theme_auto_system", "tui.themes", "tui.mouse"];

/// Typed leaf action for preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PreferenceAction {
    #[default]
    None,
    /// Toggle auto/system theme selection.
    ToggleThemeAuto,
    /// Select a named theme.
    SelectTheme,
    /// Fall back to a reduced terminal capability.
    TerminalFallback,
    /// Toggle mouse support.
    ToggleMouse,
    /// Toggle timestamp display.
    ToggleTimestamps,
    /// Set the effort level.
    SetEffort,
    /// Select a persona.
    SelectPersona,
    /// Toggle debug mode.
    ToggleDebug,
    /// Toggle always-approve mode.
    ToggleAlwaysApprove,
    /// Toggle auto mode.
    ToggleAuto,
}

/// Theme mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Manual,
    Auto,
    System,
}

/// Effort level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EffortLevel {
    Low,
    #[default]
    Medium,
    High,
    Ultra,
}

/// Resolve a capability ID to its backend owner and availability.
pub fn resolve(capability_id: &str) -> Option<LeafActionResolution> {
    let (owner, available) = match capability_id {
        "tui.theme_auto_system" => (
            "crates/harness-tui/src/theme.rs",
            ActionAvailability::Unwired,
        ),
        "tui.themes" => (
            "crates/harness-tui/src/theme.rs",
            ActionAvailability::Unwired,
        ),
        "tui.mouse" => ("crates/harness-tui/src/app.rs", ActionAvailability::Unwired),
        _ => return None,
    };
    Some(LeafActionResolution {
        capability_id: capability_id_to_static(capability_id)?,
        backend_owner: owner,
        availability: available,
        replay_safe: true,
    })
}

/// Validate input for a preference action.
pub fn validate_input(action: PreferenceAction, input: &str) -> InputValidation {
    match action {
        PreferenceAction::None => InputValidation::Invalid("action is None"),
        PreferenceAction::ToggleThemeAuto
        | PreferenceAction::TerminalFallback
        | PreferenceAction::ToggleMouse
        | PreferenceAction::ToggleTimestamps
        | PreferenceAction::ToggleDebug
        | PreferenceAction::ToggleAlwaysApprove
        | PreferenceAction::ToggleAuto => {
            if input.is_empty() {
                InputValidation::Valid
            } else {
                InputValidation::Invalid("toggle takes no input")
            }
        }
        PreferenceAction::SelectTheme => {
            if input.is_empty() {
                InputValidation::Invalid("theme selection requires a theme name")
            } else if input.len() > 64 {
                InputValidation::Invalid("theme name exceeds 64 chars")
            } else {
                InputValidation::Valid
            }
        }
        PreferenceAction::SetEffort => match input {
            "low" | "medium" | "high" | "ultra" => InputValidation::Valid,
            _ => InputValidation::Invalid("effort must be low/medium/high/ultra"),
        },
        PreferenceAction::SelectPersona => {
            if input.is_empty() {
                InputValidation::Invalid("persona selection requires a persona name")
            } else if input.len() > 64 {
                InputValidation::Invalid("persona name exceeds 64 chars")
            } else {
                InputValidation::Valid
            }
        }
    }
}

/// Check whether an action is safe during replay.
pub fn is_replay_safe(action: PreferenceAction) -> bool {
    matches!(
        action,
        PreferenceAction::None
            | PreferenceAction::ToggleThemeAuto
            | PreferenceAction::SelectTheme
            | PreferenceAction::TerminalFallback
            | PreferenceAction::ToggleMouse
            | PreferenceAction::ToggleTimestamps
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
