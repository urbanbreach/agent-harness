//! Leaf action contract for Group F (notifications/tips) — Todo 26.
//!
//! Names the real backend owner for notice capabilities and defines the typed
//! action surface. Wiring into shared Action/keybinding/slash registries is
//! reserved for Todo 28; this module is a plain value contract with no
//! app-state or registry dependency.
//!
//! Capabilities covered:
//! - `tui.notifications` — toast/banner notifications
//! - `tui.tips` — contextual tips/hints

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
pub const GROUP_ID: &str = "F";

/// Primary backend owner for notice capabilities.
pub const BACKEND_OWNER: &str = "crates/harness-tui/src/app.rs";

/// Capability IDs covered by this group.
pub const CAPABILITY_IDS: &[&str] = &["tui.notifications", "tui.tips"];

/// Typed leaf action for notices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoticeAction {
    #[default]
    None,
    /// Show a notification toast/banner.
    ShowNotification,
    /// Dismiss the current notification.
    DismissNotification,
    /// Show a contextual tip.
    ShowTip,
    /// Dismiss the current tip.
    DismissTip,
    /// Show an announcement.
    ShowAnnouncement,
    /// Show release notes.
    ShowReleaseNotes,
}

/// Notice severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoticeLevel {
    #[default]
    Info,
    Warning,
    Error,
    Success,
}

/// Resolve a capability ID to its backend owner and availability.
pub fn resolve(capability_id: &str) -> Option<LeafActionResolution> {
    let (owner, available) = match capability_id {
        "tui.notifications" => ("crates/harness-tui/src/app.rs", ActionAvailability::Unwired),
        "tui.tips" => ("crates/harness-tui/src/app.rs", ActionAvailability::Unwired),
        _ => return None,
    };
    Some(LeafActionResolution {
        capability_id: capability_id_to_static(capability_id)?,
        backend_owner: owner,
        availability: available,
        replay_safe: true,
    })
}

/// Validate input for a notice action.
pub fn validate_input(action: NoticeAction, input: &str) -> InputValidation {
    match action {
        NoticeAction::None => InputValidation::Invalid("action is None"),
        NoticeAction::ShowNotification => {
            if input.is_empty() {
                InputValidation::Invalid("notification requires a non-empty message")
            } else if input.len() > 1024 {
                InputValidation::Invalid("notification message exceeds 1024 chars")
            } else {
                InputValidation::Valid
            }
        }
        NoticeAction::ShowTip => {
            if input.is_empty() {
                InputValidation::Invalid("tip requires a non-empty body")
            } else if input.len() > 2048 {
                InputValidation::Invalid("tip body exceeds 2048 chars")
            } else {
                InputValidation::Valid
            }
        }
        NoticeAction::DismissNotification
        | NoticeAction::DismissTip
        | NoticeAction::ShowAnnouncement
        | NoticeAction::ShowReleaseNotes => {
            if input.is_empty() {
                InputValidation::Valid
            } else {
                InputValidation::Invalid("dismiss/show takes no input")
            }
        }
    }
}

/// Check whether an action is safe during replay.
pub fn is_replay_safe(action: NoticeAction) -> bool {
    matches!(
        action,
        NoticeAction::None
            | NoticeAction::ShowNotification
            | NoticeAction::ShowTip
            | NoticeAction::ShowAnnouncement
            | NoticeAction::ShowReleaseNotes
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
