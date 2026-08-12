//! Terminal-frame capture, presentation acknowledgement, and backend deduplication.

mod backend;
mod queue;

pub use backend::{FrameBackendMetrics, FrameOutputBackend};
pub use queue::{
    FrameKind, FrameOutput, FrameOutputMetrics, FrameOutputReceiver, FrameOutputWriter,
    FrameSubmission, FrameWriterMetrics, SerializedFrame,
};
