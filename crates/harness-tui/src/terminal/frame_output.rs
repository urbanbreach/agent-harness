//! Terminal-frame capture, presentation acknowledgement, and backend deduplication.

mod backend;
mod queue;

pub use backend::FrameOutputBackend;
pub use queue::{
    FrameKind, FrameOutput, FrameOutputReceiver, FrameOutputWriter, FrameSubmission,
    SerializedFrame,
};
