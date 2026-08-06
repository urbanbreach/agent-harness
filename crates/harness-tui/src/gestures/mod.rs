#![allow(
    clippy::mod_module_files,
    reason = "Task 11 requires a directory facade with mod.rs"
)]

//! Deterministic pointer gesture classification, state, and hit routing.

mod classifier;
mod device;
mod drag;
mod routing;
mod scroll;

pub use classifier::{classify, GestureEvent, GestureHistory, GestureInput, GestureKind};
pub use device::GestureDevice;
pub use drag::{DragError, DragLifecycle, DragSnapshot};
pub use routing::{route_hit_target, HitSurface, HitTarget, Point};
pub use scroll::{ScrollDirection, ScrollEmission, ScrollGesture};

/// A new input after this gap starts a new gesture.
pub const GESTURE_BOUNDARY_MS: u64 = 80;
/// Scroll deltas are eligible for a batched flush after this interval.
pub const FLUSH_INTERVAL_MS: u64 = 16;
