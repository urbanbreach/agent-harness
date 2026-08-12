use std::time::Instant;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::presentation::{
    CauseId, PresentationClock, PresentationFrame, PresentationRevision, PresentationTimestamp,
    RenderDemand,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameKind {
    Differential,
    FullRepaint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameSubmission {
    Accepted(FrameKind),
    Unchanged,
    ResyncRequired,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameOutputMetrics {
    pub redraw_requests: u64,
    pub frames_submitted: u64,
    pub frames_coalesced: u64,
    pub delayed_by_in_flight: u64,
    pub no_op_frames: u64,
    pub full_repaints: u64,
    pub bytes_submitted: u64,
    pub capture_write_calls: u64,
    pub frame_build_time_micros: u64,
    pub max_frame_build_time_micros: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameWriterMetrics {
    pub frames_written: u64,
    pub bytes_written: u64,
    pub writer_latency_micros: u64,
    pub max_writer_latency_micros: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameWriteStage {
    Write,
    Flush,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FrameOutputFailure {
    #[error("terminal frame writer failed during {0:?}")]
    Write(FrameWriteStage),
    #[error("terminal frame writer acknowledgement channel disconnected")]
    Disconnected,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FrameAckOutcome {
    Success,
    Failure { stage: FrameWriteStage },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameAck {
    pub sequence: u64,
    pub revision: PresentationRevision,
    pub cause_ids: Vec<CauseId>,
    pub requested_at: PresentationTimestamp,
    pub render_started_at: PresentationTimestamp,
    pub render_ended_at: PresentationTimestamp,
    pub submitted_at: PresentationTimestamp,
    pub write_started_at: PresentationTimestamp,
    pub write_ended_at: PresentationTimestamp,
    pub acknowledged_at: PresentationTimestamp,
    pub frame_kind: FrameKind,
    pub byte_count: usize,
    pub byte_sha256: String,
    pub outcome: FrameAckOutcome,
}

impl FrameAck {
    pub const fn revision(&self) -> PresentationRevision {
        self.revision
    }

    pub fn cause_ids(&self) -> &[CauseId] {
        &self.cause_ids
    }

    pub const fn kind(&self) -> FrameKind {
        self.frame_kind
    }

    pub const fn byte_count(&self) -> usize {
        self.byte_count
    }

    pub fn byte_sha256(&self) -> &str {
        &self.byte_sha256
    }

    pub const fn outcome(&self) -> &FrameAckOutcome {
        &self.outcome
    }

    pub const fn write_started_at(&self) -> PresentationTimestamp {
        self.write_started_at
    }

    pub const fn write_ended_at(&self) -> PresentationTimestamp {
        self.write_ended_at
    }

    pub const fn acknowledged_at(&self) -> PresentationTimestamp {
        self.acknowledged_at
    }
}

impl From<FrameAck> for PresentationFrame {
    fn from(ack: FrameAck) -> Self {
        Self {
            sequence: ack.sequence,
            revision: ack.revision,
            cause_ids: ack.cause_ids,
            requested_at: ack.requested_at,
            render_started_at: ack.render_started_at,
            render_ended_at: ack.render_ended_at,
            submitted_at: ack.submitted_at,
            write_started_at: ack.write_started_at,
            write_ended_at: ack.write_ended_at,
            acknowledged_at: ack.acknowledged_at,
            frame_kind: ack.frame_kind,
            byte_count: ack.byte_count,
            byte_sha256: ack.byte_sha256,
            acknowledgement: ack.outcome,
        }
    }
}

#[derive(Debug)]
pub struct SerializedFrame {
    pub(crate) sequence: u64,
    pub(crate) bytes: Vec<u8>,
    pub(crate) byte_sha256: String,
    pub(crate) kind: FrameKind,
    pub(crate) demand: RenderDemand,
    pub(crate) render_started_at: PresentationTimestamp,
    pub(crate) render_ended_at: PresentationTimestamp,
    pub(crate) submitted_at: PresentationTimestamp,
    pub(crate) submitted_instant: Instant,
    pub(crate) clock: PresentationClock,
}

impl SerializedFrame {
    pub(crate) fn new(
        sequence: u64,
        bytes: Vec<u8>,
        kind: FrameKind,
        demand: RenderDemand,
        render_started_at: PresentationTimestamp,
        render_ended_at: PresentationTimestamp,
        clock: PresentationClock,
    ) -> Self {
        let submitted_instant = Instant::now();
        let byte_sha256 = hex_digest(&Sha256::digest(&bytes));
        let submitted_at = clock.timestamp(submitted_instant);
        Self {
            sequence,
            bytes,
            byte_sha256,
            kind,
            demand,
            render_started_at,
            render_ended_at,
            submitted_at,
            submitted_instant,
            clock,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
