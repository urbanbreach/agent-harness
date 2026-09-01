//! Terminal-frame capture, presentation acknowledgement, and backend deduplication.

mod backend;
mod capture;
mod hyperlinks;
mod model;
mod queue;
mod worker;

pub use backend::{FrameBackendMetrics, FrameOutputBackend};
pub use capture::FrameOutputWriter;
pub(crate) use hyperlinks::{set_frame_hyperlinks, FrameHyperlink};
pub use model::{
    FrameAck, FrameAckOutcome, FrameKind, FrameOutputFailure, FrameOutputMetrics, FrameSubmission,
    FrameWriteStage, FrameWriterMetrics, SerializedFrame,
};
pub use queue::FrameOutput;
pub use worker::FrameOutputReceiver;
