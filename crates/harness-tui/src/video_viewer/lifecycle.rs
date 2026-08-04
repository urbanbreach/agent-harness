use std::{
    error::Error,
    fmt::{Display, Formatter},
};

use super::{
    progress::{FramePacing, PlaybackProgress},
    subprocess::{SubprocessDescriptor, SubprocessReceipt},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewerPhase {
    Idle,
    Opening(SubprocessDescriptor),
    Decoding,
    Playing { progress: PlaybackProgress },
    Error(String),
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerState {
    phase: ViewerPhase,
    pacing: FramePacing,
    receipt: Option<SubprocessReceipt>,
    cancelled: bool,
}

impl Default for ViewerState {
    fn default() -> Self {
        Self::new(FramePacing::default_pacing())
    }
}

impl ViewerState {
    pub fn new(pacing: FramePacing) -> Self {
        Self {
            phase: ViewerPhase::Idle,
            pacing,
            receipt: None,
            cancelled: false,
        }
    }
    pub const fn phase(&self) -> &ViewerPhase {
        &self.phase
    }

    pub fn open(&mut self, descriptor: SubprocessDescriptor) -> Result<(), ViewerError> {
        descriptor.validate()?;
        self.phase = ViewerPhase::Opening(descriptor);
        self.receipt = None;
        self.cancelled = false;
        Ok(())
    }

    pub fn advance_to_decoding(&mut self) {
        if matches!(self.phase, ViewerPhase::Opening(_)) {
            self.phase = ViewerPhase::Decoding;
        }
    }

    pub fn start_playback(&mut self, total_frames: u64, total_ms: u64) {
        if matches!(self.phase, ViewerPhase::Decoding) {
            self.phase = ViewerPhase::Playing {
                progress: PlaybackProgress::new(total_frames, total_ms),
            };
        }
    }

    pub fn tick_playback(&mut self, frames: u64, ms: u64) {
        if let ViewerPhase::Playing { progress } = &mut self.phase {
            progress.advance(frames, ms);
            if progress.is_complete() {
                self.phase = ViewerPhase::Closed;
            }
        }
    }

    pub fn report_error(&mut self, message: String) {
        self.phase = ViewerPhase::Error(message);
    }
    pub fn cancel(&mut self) {
        self.cancelled = true;
        self.phase = ViewerPhase::Closed;
    }
    pub fn close(&mut self, receipt: SubprocessReceipt) {
        self.receipt = Some(receipt);
        self.phase = ViewerPhase::Closed;
    }
    pub const fn receipt(&self) -> Option<&SubprocessReceipt> {
        self.receipt.as_ref()
    }
    pub const fn is_cancelled(&self) -> bool {
        self.cancelled
    }
    pub fn cleanup_verified(&self) -> bool {
        self.receipt
            .as_ref()
            .is_none_or(SubprocessReceipt::cleanup_complete)
    }
    pub const fn pacing(&self) -> FramePacing {
        self.pacing
    }
}

pub type VideoViewer = ViewerState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewerError {
    UnknownBinary,
    OversizedMedia,
    MalformedArg(String),
    UnknownRequest,
    Cancelled,
    SubprocessFailed(i32),
}

impl Display for ViewerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownBinary => formatter.write_str("unknown binary"),
            Self::OversizedMedia => formatter.write_str("media exceeds viewer bounds"),
            Self::MalformedArg(arg) => write!(formatter, "malformed argument: {arg}"),
            Self::UnknownRequest => formatter.write_str("unknown subprocess request"),
            Self::Cancelled => formatter.write_str("viewer cancelled"),
            Self::SubprocessFailed(code) => {
                write!(formatter, "subprocess failed with exit code {code}")
            }
        }
    }
}

impl Error for ViewerError {}
