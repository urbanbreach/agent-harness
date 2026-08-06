pub mod anchors;
pub mod autoscroll;
pub mod easing;
pub mod follow;
pub mod scrollbar;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScrollError {
    NonFinite(&'static str),
    NonPositive(&'static str),
    Negative(&'static str),
    EmptyLayout,
    MissingAnchor,
    InvalidGeometry,
}

impl std::fmt::Display for ScrollError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinite(field) => write!(formatter, "{field} must be finite"),
            Self::NonPositive(field) => write!(formatter, "{field} must be positive"),
            Self::Negative(field) => write!(formatter, "{field} must not be negative"),
            Self::EmptyLayout => formatter.write_str("transcript layout must not be empty"),
            Self::MissingAnchor => formatter.write_str("logical anchor is not in the layout"),
            Self::InvalidGeometry => formatter.write_str("scroll geometry is invalid"),
        }
    }
}

impl std::error::Error for ScrollError {}

pub type ScrollResult<T> = Result<T, ScrollError>;

pub use anchors::{BlockPlacement, LogicalAnchor, TranscriptLayout};
pub use autoscroll::{AutoscrollStep, DragAutoscroll, DragViewport, Edge};
pub use easing::{
    EasingKind, FractionalScroll, MotionPreference, ScrollFrame, ScrollTransition,
    TransitionRequest,
};
pub use follow::{FollowMode, FollowState};
pub use scrollbar::{ScrollbarDrag, ScrollbarGeometry};
