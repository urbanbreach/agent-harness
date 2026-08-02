//! Leaf action contract for Group H (navigation/import/rewind/queue) — Todo 26.
//!
//! Names the real backend owner for navigation capabilities and defines the
//! typed action surface. Wiring into shared Action/keybinding/slash registries
//! is reserved for Todo 28; this module is a plain value contract with no
//! app-state or registry dependency.
//!
//! Capabilities covered:
//! - Foreign import UI journey, rewind TUI, memory palette
//! - Prompt queue, file completion, home/docs/share/import

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
pub const GROUP_ID: &str = "H";

/// Primary backend owner for navigation capabilities.
pub const BACKEND_OWNER: &str = "crates/harness-tui/src/app.rs";

/// Capability IDs covered by this group (journey-style, no inventory rows).
pub const CAPABILITY_IDS: &[&str] = &[
    "tui.foreign_import_ui_journey",
    "tui.rewind_tui",
    "tui.memory_palette",
    "tui.prompt_queue",
    "tui.file_completion",
    "tui.home",
    "tui.docs",
    "tui.share",
    "tui.import",
];

/// Typed leaf action for navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavigationAction {
    #[default]
    None,
    /// Foreign import UI journey.
    ForeignImport,
    /// Rewind the TUI to a prior state.
    RewindTui,
    /// Open the memory palette.
    MemoryPalette,
    /// Enqueue a prompt for later execution.
    EnqueuePrompt,
    /// File completion in the composer.
    FileCompletion,
    /// Navigate home.
    Home,
    /// Open docs.
    Docs,
    /// Share the current session.
    Share,
    /// Import a file or session.
    Import,
}

/// Resolve a capability ID to its backend owner and availability.
pub fn resolve(capability_id: &str) -> Option<LeafActionResolution> {
    let (owner, available, replay_safe) = match capability_id {
        "tui.foreign_import_ui_journey" => (
            "crates/harness-tui/src/app/foreign_import.rs",
            ActionAvailability::Available,
            true,
        ),
        "tui.rewind_tui" => (
            "crates/harness-tui/src/app.rs",
            ActionAvailability::Unwired,
            true,
        ),
        "tui.memory_palette" => (
            "crates/harness-tui/src/app.rs",
            ActionAvailability::Unwired,
            true,
        ),
        "tui.prompt_queue" => (
            "crates/harness-tui/src/app.rs",
            ActionAvailability::Unwired,
            false,
        ),
        "tui.file_completion" => (
            "crates/harness-tui/src/ui_composer.rs",
            ActionAvailability::Unwired,
            true,
        ),
        "tui.home" => (
            "crates/harness-tui/src/app.rs",
            ActionAvailability::Unwired,
            true,
        ),
        "tui.docs" => (
            "crates/harness-tui/src/app.rs",
            ActionAvailability::Unavailable("docs service not configured"),
            true,
        ),
        "tui.share" => (
            "crates/harness-tui/src/app.rs",
            ActionAvailability::Unavailable("share service not configured"),
            false,
        ),
        "tui.import" => (
            "crates/harness-tui/src/app.rs",
            ActionAvailability::Unwired,
            false,
        ),
        _ => return None,
    };
    Some(LeafActionResolution {
        capability_id: capability_id_to_static(capability_id)?,
        backend_owner: owner,
        availability: available,
        replay_safe,
    })
}

/// Validate input for a navigation action.
pub fn validate_input(action: NavigationAction, input: &str) -> InputValidation {
    match action {
        NavigationAction::None => InputValidation::Invalid("action is None"),
        NavigationAction::ForeignImport
        | NavigationAction::RewindTui
        | NavigationAction::MemoryPalette
        | NavigationAction::Home
        | NavigationAction::Docs => {
            if input.is_empty() {
                InputValidation::Valid
            } else {
                InputValidation::Invalid("navigation action takes no input")
            }
        }
        NavigationAction::EnqueuePrompt => {
            if input.is_empty() {
                InputValidation::Invalid("enqueue requires a non-empty prompt")
            } else if input.len() > 8192 {
                InputValidation::Invalid("prompt exceeds 8192 chars")
            } else {
                InputValidation::Valid
            }
        }
        NavigationAction::FileCompletion => {
            if input.is_empty() {
                InputValidation::Valid
            } else if input.len() > 4096 {
                InputValidation::Invalid("file path exceeds 4096 chars")
            } else {
                InputValidation::Valid
            }
        }
        NavigationAction::Share | NavigationAction::Import => {
            if input.is_empty() {
                InputValidation::Invalid("share/import requires a target path")
            } else if input.len() > 4096 {
                InputValidation::Invalid("path exceeds 4096 chars")
            } else {
                InputValidation::Valid
            }
        }
    }
}

/// Check whether an action is safe during replay.
pub fn is_replay_safe(action: NavigationAction) -> bool {
    matches!(
        action,
        NavigationAction::None
            | NavigationAction::ForeignImport
            | NavigationAction::RewindTui
            | NavigationAction::MemoryPalette
            | NavigationAction::FileCompletion
            | NavigationAction::Home
            | NavigationAction::Docs
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
