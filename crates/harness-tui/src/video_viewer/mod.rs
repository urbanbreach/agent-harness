//! Bounded inline video viewer: subprocess lifecycle, progress, cancellation, cleanup.

pub mod lifecycle;
pub mod progress;
pub mod subprocess;

pub use lifecycle::{VideoViewer, ViewerError, ViewerPhase, ViewerState};
pub use progress::{FramePacing, PlaybackProgress};
pub use subprocess::{SubprocessDescriptor, SubprocessReceipt, SubprocessSupervisor};
