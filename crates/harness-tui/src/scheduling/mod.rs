#![allow(
    clippy::mod_module_files,
    reason = "The scheduler facade intentionally groups focused sibling modules"
)]

mod coalesce;
mod decision;
mod dual_clock;
mod scheduler;

pub use coalesce::RedrawCoalescer;
pub use decision::{FrameDecision, FrameReason};
pub use dual_clock::{DualClock, FrameNow};
pub use scheduler::{FrameInputs, FrameScheduler};

pub const ANIMATION_PERIOD_MS: u64 = 1_000 / 30;
pub const FLUSH_DEADLINE_MS: u64 = 16;
