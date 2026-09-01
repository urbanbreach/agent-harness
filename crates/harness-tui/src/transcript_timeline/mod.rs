#![allow(
    clippy::mod_module_files,
    reason = "Task 23 requires a focused timeline facade with sibling modules"
)]

pub mod clipping;
pub mod geometry;
pub mod hit_map;
mod key_navigation;
pub mod markers;
pub mod navigation;
mod navigation_state;
mod response_navigation;

pub use clipping::{clip_marker_label, marker_display_width, marker_label_width};
pub use geometry::{
    geometry_for_rect, geometry_for_viewport, TimelineGeometry, TimelineMarkerRect,
};
pub use hit_map::{TimelineHitMap, TimelineHitRegion};
pub use markers::{
    MarkerInteraction, TimelineMarker, TimelineMarkerStyle, TimelineStatus, TimelineTurn,
};
pub use navigation::{KeyJump, TimelineJump, TimelineNavigation};
pub use navigation_state::{
    ResponsePosition, ScrollAnchor, TimelineNavigationError, TimelineNavigationSnapshot,
};
