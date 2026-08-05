//! Leaf action contract for Group D (dashboard/session-status) — Todo 26.
//!
//! Capabilities covered:
//! - `cli.dashboard` — CLI dashboard command (deferred to Wave 3 TUI)
//! - `tui.session_status_dashboard` — TUI session status dashboard

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
pub const GROUP_ID: &str = "D";

/// Canonical slash aliases that enter the interactive dashboard.
pub const STATUS_COMMANDS: &[&str] = &["status", "dashboard"];

/// Capability IDs covered by this group.
pub const CAPABILITY_IDS: &[&str] = &["cli.dashboard", "tui.session_status_dashboard"];

/// Typed leaf action for dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DashboardAction {
    #[default]
    None,
    /// Open the dashboard panel.
    OpenDashboard,
    /// Show session status summary.
    ShowSessionStatus,
    /// Show usage statistics.
    ShowUsage,
    /// Show context information.
    ShowContext,
    /// Show active tasks.
    ShowTasks,
}

impl DashboardAction {
    pub const fn is_interactive(self) -> bool {
        matches!(
            self,
            Self::OpenDashboard
                | Self::ShowSessionStatus
                | Self::ShowUsage
                | Self::ShowContext
                | Self::ShowTasks
        )
    }
}

/// Resolve a slash alias to the interactive dashboard action.
pub fn action_for_command(command: &str) -> Option<DashboardAction> {
    STATUS_COMMANDS.contains(&command)
        .then_some(DashboardAction::OpenDashboard)
}

/// Resolve a capability ID to its backend owner and availability.
pub fn resolve(capability_id: &str) -> Option<LeafActionResolution> {
    let (owner, available) = match capability_id {
        "cli.dashboard" => ("crates/harness/src/lib.rs", ActionAvailability::Unwired),
        "tui.session_status_dashboard" => (
            "crates/harness-tui/src/app.rs",
            ActionAvailability::Available,
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

/// Validate input for a dashboard action.
pub fn validate_input(action: DashboardAction, input: &str) -> InputValidation {
    match action {
        DashboardAction::None => InputValidation::Invalid("action is None"),
        DashboardAction::OpenDashboard
        | DashboardAction::ShowSessionStatus
        | DashboardAction::ShowUsage
        | DashboardAction::ShowContext
        | DashboardAction::ShowTasks => {
            if input.is_empty() {
                InputValidation::Valid
            } else {
                InputValidation::Invalid("dashboard action takes no input")
            }
        }
    }
}

/// Check whether an action is safe during replay.
pub fn is_replay_safe(action: DashboardAction) -> bool {
    matches!(
        action,
        DashboardAction::None
            | DashboardAction::OpenDashboard
            | DashboardAction::ShowSessionStatus
            | DashboardAction::ShowUsage
            | DashboardAction::ShowContext
            | DashboardAction::ShowTasks
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
