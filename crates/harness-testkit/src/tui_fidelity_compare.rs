mod cells;
mod comparison;
mod error;
mod hashing;
mod motion;
mod ordered_motion;
mod pixels;
mod presentation_timing;
mod presentation_timing_gate;
mod secret_scan;
mod self_compare;
mod timing;
mod types;

pub use comparison::{compare_capture, compare_capture_with_profile};
pub use hashing::hash_bytes;
pub use ordered_motion::{
    compare_ordered_motion, compare_ordered_presentations, normalize_ordered_motion,
};
pub use presentation_timing::{
    derive_comparison_presentation_timing, derive_presentation_timing, NativeTimingMetrics,
    PresentationTimingMetrics,
};
pub use presentation_timing_gate::{
    compare_presentation_timing, MAX_GAP_MULTIPLIER, P95_LIMIT_PERCENT,
};
pub use types::*;
