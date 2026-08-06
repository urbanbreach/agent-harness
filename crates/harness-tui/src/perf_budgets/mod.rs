//! Performance budgets: frame timing, queue depths, cache bounds, resource limits.

pub mod frame;
pub mod queues;
pub mod resources;
pub mod sampling;

pub use frame::{FrameClock, FrameMetrics, FramePhase};
pub use queues::{BackpressureDecision, QueueBounds};
pub use resources::{CacheBounds, ResourceBudget, ResourceSnapshot};
pub use sampling::{SampleWindow, StressSample};
