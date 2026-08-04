mod eligibility;
mod model;
mod projection;
mod status;

pub use eligibility::{
    DashboardEligibilityRules, DashboardEntryEligibility, EligibilityExclusion, SelectionKey,
    SelectionKeyError,
};
pub use model::{
    DashboardActivity, DashboardGroup, DashboardGroupKey, DashboardReadModel,
    DashboardRelationship, DashboardRow, DashboardStatus,
};
pub use projection::{
    DashboardProjectionError, DashboardReplayRegistry, DashboardSessionInput,
    build_dashboard_read_model,
};
