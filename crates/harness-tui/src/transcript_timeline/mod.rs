mod key_navigation;
pub mod markers;
pub mod navigation;

pub use markers::{MarkerInteraction, TimelineMarker, TimelineMarkerStyle, TimelineStatus};
pub use navigation::{KeyJump, TimelineJump};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResponsePosition {
    pub index: usize,
    pub total: usize,
}
