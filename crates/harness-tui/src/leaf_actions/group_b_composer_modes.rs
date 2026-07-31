//! Leaf action contract for Group B (vim/multiline/history/find) — Todo 26.
//!
//! Names the real backend owner for composer mode capabilities and defines the
//! typed action surface. Wiring into shared Action/keybinding/slash registries
//! is reserved for Todo 28; this module is a plain value contract with no
//! app-state or registry dependency.
//!
//! Capabilities covered:
//! - `tui.vim_mode` — modal editing in composer (normal/insert/visual modes)
//! - Multiline toggle, history navigation, in-composer find

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
pub const GROUP_ID: &str = "B";

/// Primary backend owner for composer mode capabilities.
pub const BACKEND_OWNER: &str = "crates/harness-tui/src/ui_composer.rs";

/// Capability IDs covered by this group.
pub const CAPABILITY_IDS: &[&str] = &["tui.vim_mode"];

/// Typed leaf action for composer modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ComposerModeAction {
    #[default]
    None,
    /// Toggle vim mode on/off.
    ToggleVimMode,
    /// Switch to vim normal mode.
    VimNormal,
    /// Switch to vim insert mode.
    VimInsert,
    /// Switch to vim visual mode.
    VimVisual,
    /// Toggle multiline input.
    MultilineToggle,
    /// Navigate to previous history entry.
    HistoryPrevious,
    /// Navigate to next history entry.
    HistoryNext,
    /// Find text within the composer buffer.
    FindInComposer,
}

/// Vim sub-mode state for the composer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VimSubMode {
    #[default]
    Disabled,
    Normal,
    Insert,
    Visual,
}

/// Resolve a capability ID to its backend owner and availability.
pub fn resolve(capability_id: &str) -> Option<LeafActionResolution> {
    let (owner, available) = match capability_id {
        "tui.vim_mode" => (
            "crates/harness-tui/src/ui_composer.rs",
            ActionAvailability::Unwired,
        ),
        _ => return None,
    };
    Some(LeafActionResolution {
        capability_id: capability_id_to_static(capability_id)?,
        backend_owner: owner,
        availability: available,
        replay_safe: true,
    })
}

/// Validate input for a composer mode action.
pub fn validate_input(action: ComposerModeAction, input: &str) -> InputValidation {
    match action {
        ComposerModeAction::None => InputValidation::Invalid("action is None"),
        ComposerModeAction::ToggleVimMode
        | ComposerModeAction::VimNormal
        | ComposerModeAction::VimInsert
        | ComposerModeAction::VimVisual
        | ComposerModeAction::MultilineToggle
        | ComposerModeAction::HistoryPrevious
        | ComposerModeAction::HistoryNext => {
            if input.is_empty() {
                InputValidation::Valid
            } else {
                InputValidation::Invalid("mode toggle takes no input")
            }
        }
        ComposerModeAction::FindInComposer => {
            if input.is_empty() {
                InputValidation::Invalid("find requires a non-empty search term")
            } else if input.len() > 1024 {
                InputValidation::Invalid("search term exceeds 1024 chars")
            } else {
                InputValidation::Valid
            }
        }
    }
}

/// Check whether an action is safe during replay.
pub fn is_replay_safe(action: ComposerModeAction) -> bool {
    matches!(
        action,
        ComposerModeAction::None
            | ComposerModeAction::VimNormal
            | ComposerModeAction::VimInsert
            | ComposerModeAction::VimVisual
            | ComposerModeAction::FindInComposer
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
