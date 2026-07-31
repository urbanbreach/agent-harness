//! Leaf action contract for Group E (inline_media/clipboard) — Todo 26.
//!
//! Names the real backend owner for media capabilities and defines the typed
//! action surface. Wiring into shared Action/keybinding/slash registries is
//! reserved for Todo 28; this module is a plain value contract with no
//! app-state or registry dependency.
//!
//! Capabilities covered:
//! - `tui.inline_media` — inline media rendering in transcript blocks
//! - Clipboard/media failure recovery

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
pub const GROUP_ID: &str = "E";

/// Capability IDs covered by this group.
pub const CAPABILITY_IDS: &[&str] = &["tui.inline_media"];

/// Typed leaf action for media.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaAction {
    #[default]
    None,
    /// Render inline media in a transcript block.
    RenderInlineMedia,
    /// Paste image from clipboard.
    ClipboardImagePaste,
    /// Recover from a media/clipboard failure.
    MediaFailureRecovery,
}

/// Media failure reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaFailureReason {
    #[default]
    None,
    ClipboardUnavailable,
    MediaDecodeFailed,
    TerminalDoesNotSupportInlineMedia,
}

/// Resolve a capability ID to its backend owner and availability.
pub fn resolve(capability_id: &str) -> Option<LeafActionResolution> {
    let (owner, available) = match capability_id {
        "tui.inline_media" => (
            "crates/harness-tui/src/ui_transcript.rs",
            ActionAvailability::Unavailable("terminal inline media protocol not negotiated"),
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

/// Validate input for a media action.
pub fn validate_input(action: MediaAction, input: &str) -> InputValidation {
    match action {
        MediaAction::None => InputValidation::Invalid("action is None"),
        MediaAction::RenderInlineMedia => {
            if input.is_empty() {
                InputValidation::Invalid("render requires a media path or url")
            } else if input.len() > 4096 {
                InputValidation::Invalid("media path exceeds 4096 chars")
            } else {
                InputValidation::Valid
            }
        }
        MediaAction::ClipboardImagePaste | MediaAction::MediaFailureRecovery => {
            if input.is_empty() {
                InputValidation::Valid
            } else {
                InputValidation::Invalid("media toggle takes no input")
            }
        }
    }
}

/// Check whether an action is safe during replay.
pub fn is_replay_safe(action: MediaAction) -> bool {
    matches!(
        action,
        MediaAction::None | MediaAction::RenderInlineMedia | MediaAction::MediaFailureRecovery
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
