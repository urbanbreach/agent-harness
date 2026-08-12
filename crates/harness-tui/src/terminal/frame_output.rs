//! Terminal-frame capture, presentation acknowledgement, and backend deduplication.

mod backend;
mod capture;
mod model;
mod queue;
mod worker;

pub use backend::{FrameBackendMetrics, FrameOutputBackend};
pub use capture::FrameOutputWriter;
pub use model::{
    FrameAck, FrameAckOutcome, FrameKind, FrameOutputMetrics, FrameSubmission, FrameWriteStage,
    FrameWriterMetrics, SerializedFrame,
};
pub use queue::FrameOutput;
pub use worker::FrameOutputReceiver;
