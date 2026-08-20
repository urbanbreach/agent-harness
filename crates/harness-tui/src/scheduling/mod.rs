#![allow(
    clippy::mod_module_files,
    reason = "The scheduler facade intentionally groups focused sibling modules"
)]

mod coalesce;
mod decision;
mod dual_clock;
mod frame_cadence;
mod motion_demand;
mod runtime_arbiter;
mod runtime_pacer;
mod runtime_wheel;
mod scheduler;
mod scheduler_deadlines;

pub use coalesce::RedrawCoalescer;
pub use decision::{FrameDecision, FrameReason};
pub use dual_clock::{DualClock, FrameNow};
pub(crate) use frame_cadence::runtime_flush_interval_ms;
pub use frame_cadence::MIN_DRAW_INTERVAL_ENV;
pub use motion_demand::{MotionCadence, MotionDemand, MotionPlan};
pub use runtime_arbiter::{
    ArbiterClock, BatchBudget, DeferredLiveUpdate, FairnessTurn, RuntimeArbiter, RuntimeDecision,
    RuntimePriority, RuntimeReady, SystemArbiterClock, INPUT_BATCH_LIMIT, INPUT_BATCH_TIME,
    LIVE_BATCH_LIMIT, LIVE_BATCH_TIME,
};
pub use runtime_pacer::{RuntimePacer, RuntimePacerAction};
pub use runtime_wheel::{WheelBatch, WheelDirection, WheelSample, MAX_WHEEL_STEPS_PER_FLUSH};
pub(crate) use scheduler::active_animation_period_ms;
pub use scheduler::{FrameInputs, FrameScheduler};

pub const ANIMATION_PERIOD_MS: u64 = 1_000 / 30;
pub const FLUSH_DEADLINE_MS: u64 = 16;
