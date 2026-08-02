//! Leaf action contract for Group A (plan/approval/view-plan) — Todo 26.
//!
//! Names the real backend owner for each plan-mode capability and defines the
//! typed action surface. Wiring into shared Action/keybinding/slash registries
//! is reserved for Todo 28; this module is a plain value contract with no
//! app-state or registry dependency.
//!
//! Capabilities covered:
//! - `tui.plan_mode` — modal plan editing workflow
//! - `tui.view_plan` — read-only plan viewer
//! - Plan approval / read-only guard / plan-mode entry/exit

/// Availability of a leaf action backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActionAvailability {
    /// Backend owner exists on disk but is not yet wired through the aggregator (Todo 28).
    #[default]
    Unwired,
    /// Backend owner is wired and the action is available.
    Available,
    /// Backend owner is unavailable: missing dependency, environment, or feature.
    Unavailable(&'static str),
}

/// Input validation result for a leaf action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputValidation {
    /// Input is valid for the action.
    Valid,
    /// Input is invalid; the string describes why.
    Invalid(&'static str),
}

/// Resolution outcome for a leaf action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeafActionResolution {
    pub capability_id: &'static str,
    pub backend_owner: &'static str,
    pub availability: ActionAvailability,
    /// Whether the action is safe to perform during replay (read-only).
    pub replay_safe: bool,
}

/// Group identifier for aggregator wiring (Todo 28).
pub const GROUP_ID: &str = "A";

/// Primary backend owner for plan-mode capabilities.
pub const BACKEND_OWNER: &str = "crates/harness-tui/src/app.rs";

/// Capability IDs covered by this group.
pub const CAPABILITY_IDS: &[&str] = &["tui.plan_mode", "tui.view_plan"];

/// Typed leaf action for the plan/approval workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlanAction {
    #[default]
    None,
    /// Enter plan mode (read-only inspection + plan file editing).
    EnterPlanMode,
    /// Exit plan mode and hand off to Build.
    ExitPlanMode,
    /// View the current plan file.
    ViewPlan,
    /// Approve the current plan and exit to Build.
    ApprovePlan,
    /// Reject the current plan with a reason.
    RejectPlan,
    /// Read-only guard: refuse a write action while in plan mode.
    ReadOnlyGuard,
}

/// Whether the action performs a write (not replay-safe).
pub fn is_write_action(action: PlanAction) -> bool {
    matches!(
        action,
        PlanAction::EnterPlanMode
            | PlanAction::ExitPlanMode
            | PlanAction::ApprovePlan
            | PlanAction::RejectPlan
    )
}

/// Resolve a capability ID to its backend owner and availability.
///
/// Returns `None` if the capability ID is not in this group.
pub fn resolve(capability_id: &str) -> Option<LeafActionResolution> {
    let (owner, available) = match capability_id {
        "tui.plan_mode" => ("crates/harness-tui/src/app.rs", ActionAvailability::Unwired),
        "tui.view_plan" => ("crates/harness-tui/src/app.rs", ActionAvailability::Unwired),
        _ => return None,
    };
    Some(LeafActionResolution {
        capability_id: capability_id_to_static(capability_id)?,
        backend_owner: owner,
        availability: available,
        replay_safe: !matches!(capability_id, "tui.plan_mode"),
    })
}

/// Validate input for a plan action.
pub fn validate_input(action: PlanAction, input: &str) -> InputValidation {
    match action {
        PlanAction::None => InputValidation::Invalid("action is None"),
        PlanAction::EnterPlanMode | PlanAction::ExitPlanMode | PlanAction::ViewPlan => {
            if input.is_empty() {
                InputValidation::Valid
            } else {
                InputValidation::Invalid("plan mode toggle takes no input")
            }
        }
        PlanAction::ApprovePlan => {
            if input.is_empty() || input.len() <= 4096 {
                InputValidation::Valid
            } else {
                InputValidation::Invalid("approval note exceeds 4096 chars")
            }
        }
        PlanAction::RejectPlan => {
            if input.is_empty() {
                InputValidation::Invalid("rejection requires a non-empty reason")
            } else if input.len() > 4096 {
                InputValidation::Invalid("rejection reason exceeds 4096 chars")
            } else {
                InputValidation::Valid
            }
        }
        PlanAction::ReadOnlyGuard => {
            if input.is_empty() {
                InputValidation::Invalid("read-only guard requires the refused action name")
            } else {
                InputValidation::Valid
            }
        }
    }
}

/// Check whether an action is safe during replay.
pub fn is_replay_safe(action: PlanAction) -> bool {
    matches!(action, PlanAction::ViewPlan | PlanAction::ReadOnlyGuard)
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
