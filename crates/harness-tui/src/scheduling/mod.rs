#![allow(
    clippy::mod_module_files,
    reason = "The scheduler facade intentionally groups focused sibling modules"
)]

mod decision;
mod dual_clock;
mod frame_cadence;
mod motion_demand;

pub use decision::{FrameDecision, FrameReason};
pub use dual_clock::{DualClock, FrameNow};
pub(crate) use frame_cadence::runtime_flush_interval_ms;
pub use frame_cadence::MIN_DRAW_INTERVAL_ENV;
pub use motion_demand::{MotionCadence, MotionDemand, MotionPlan};

pub const ANIMATION_PERIOD_MS: u64 = 4;
pub const FLUSH_DEADLINE_MS: u64 = 4;

pub const LIVE_BATCH_TIME: std::time::Duration = std::time::Duration::from_millis(2);

pub(crate) fn active_animation_period_ms() -> u64 {
    crate::theme_tokens::DESIGN_TOKENS
        .motion_tokens
        .all
        .iter()
        .find(|token| token.kind == crate::theme_tokens::MotionKind::ActiveTick)
        .map_or(ANIMATION_PERIOD_MS, |token| u64::from(token.interval_ms))
}
