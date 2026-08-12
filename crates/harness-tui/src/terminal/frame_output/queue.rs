use crossbeam_channel::{bounded, Receiver, Sender, TryRecvError, TrySendError};
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::capture::{capture_lock, synchronized_bytes, CaptureState, FrameOutputWriter};
use super::model::{
    FrameAck, FrameAckOutcome, FrameKind, FrameOutputFailure, FrameOutputMetrics, FrameSubmission,
    FrameWriterMetrics, SerializedFrame,
};
use super::worker::{writer_metrics_lock, FrameOutputReceiver};
use crate::presentation::{PresentationClock, RenderDemand};
use crate::terminal::writer::{BEGIN_SYNCHRONIZED_UPDATE, END_SYNCHRONIZED_UPDATE};

#[derive(Debug)]
pub struct FrameOutput {
    sender: Sender<SerializedFrame>,
    acknowledgements: Receiver<FrameAck>,
    completed_acknowledgements: Vec<FrameAck>,
    capture: Arc<Mutex<CaptureState>>,
    clock: PresentationClock,
    active_kind: Option<FrameKind>,
    active_demand: Option<RenderDemand>,
    full_repaint_required: bool,
    next_sequence: u64,
    in_flight: Option<u64>,
    metrics: FrameOutputMetrics,
    writer_metrics: Arc<Mutex<FrameWriterMetrics>>,
    frame_started_at: Option<Instant>,
    fatal_failure: Option<FrameOutputFailure>,
}

impl FrameOutput {
    pub fn bounded(capacity: usize) -> (Self, FrameOutputWriter, FrameOutputReceiver) {
        Self::bounded_with_clock(capacity, PresentationClock::new())
    }

    pub fn bounded_with_clock(
        capacity: usize,
        clock: PresentationClock,
    ) -> (Self, FrameOutputWriter, FrameOutputReceiver) {
        let (sender, receiver) = bounded(capacity);
        let (acknowledge, acknowledgements) = bounded(1);
        let capture = Arc::new(Mutex::new(CaptureState::default()));
        let writer_metrics = Arc::new(Mutex::new(FrameWriterMetrics::default()));
        let output = Self {
            sender,
            acknowledgements,
            completed_acknowledgements: Vec::new(),
            capture: Arc::clone(&capture),
            clock,
            active_kind: None,
            active_demand: None,
            full_repaint_required: false,
            next_sequence: 1,
            in_flight: None,
            metrics: FrameOutputMetrics::default(),
            writer_metrics: Arc::clone(&writer_metrics),
            frame_started_at: None,
            fatal_failure: None,
        };
        (
            output,
            FrameOutputWriter { capture },
            FrameOutputReceiver::new(receiver, acknowledge, writer_metrics),
        )
    }

    pub fn is_ready_for_frame(&mut self) -> bool {
        loop {
            match self.acknowledgements.try_recv() {
                Ok(ack) => self.record_acknowledgement(ack),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.in_flight.is_some() {
                        self.fatal_failure = Some(FrameOutputFailure::Disconnected);
                    }
                    break;
                }
            }
        }
        self.in_flight.is_none()
    }

    fn record_acknowledgement(&mut self, ack: FrameAck) {
        if self.in_flight == Some(ack.sequence) {
            self.in_flight = None;
        } else {
            self.full_repaint_required = true;
        }
        if let FrameAckOutcome::Failure { stage } = ack.outcome {
            self.fatal_failure = Some(FrameOutputFailure::Write(stage));
        }
        self.completed_acknowledgements.push(ack);
    }

    pub fn accept_acknowledgement(&mut self, ack: FrameAck) {
        self.record_acknowledgement(ack);
    }

    pub const fn has_in_flight_frame(&self) -> bool {
        self.in_flight.is_some()
    }

    pub fn acknowledgement_receiver(&self) -> &Receiver<FrameAck> {
        &self.acknowledgements
    }

    pub fn take_fatal_failure(&mut self) -> Option<FrameOutputFailure> {
        self.fatal_failure.take()
    }

    pub fn begin_frame(&mut self) -> io::Result<FrameKind> {
        let demand = RenderDemand::startup(self.clock.now());
        self.begin_frame_for(demand)
    }

    pub fn begin_frame_for(&mut self, demand: RenderDemand) -> io::Result<FrameKind> {
        self.metrics.redraw_requests = self.metrics.redraw_requests.saturating_add(1);
        if !self.is_ready_for_frame() {
            self.metrics.delayed_by_in_flight = self.metrics.delayed_by_in_flight.saturating_add(1);
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "terminal frame is still being presented",
            ));
        }
        let mut capture = capture_lock(&self.capture);
        if capture.active {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "terminal frame capture is already active",
            ));
        }
        let kind = if self.full_repaint_required {
            FrameKind::FullRepaint
        } else {
            FrameKind::Differential
        };
        capture.bytes.clear();
        capture.write_calls = 0;
        capture.active = true;
        self.active_kind = Some(kind);
        self.active_demand = Some(demand);
        self.frame_started_at = Some(Instant::now());
        Ok(kind)
    }

    pub fn finish_frame(&mut self) -> io::Result<FrameSubmission> {
        let kind = self.active_kind.take().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "finish_frame called with no active terminal frame",
            )
        })?;
        let demand = self.active_demand.take().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "active frame has no render demand",
            )
        })?;
        let started_at = self.frame_started_at.take().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "active frame has no start time",
            )
        })?;
        let render_ended_instant = Instant::now();
        let micros = u64::try_from(started_at.elapsed().as_micros()).unwrap_or(u64::MAX);
        self.metrics.frame_build_time_micros =
            self.metrics.frame_build_time_micros.saturating_add(micros);
        self.metrics.max_frame_build_time_micros =
            self.metrics.max_frame_build_time_micros.max(micros);
        let (payload, write_calls) = {
            let mut capture = capture_lock(&self.capture);
            capture.active = false;
            (std::mem::take(&mut capture.bytes), capture.write_calls)
        };
        self.metrics.capture_write_calls =
            self.metrics.capture_write_calls.saturating_add(write_calls);
        if payload.is_empty() {
            self.metrics.no_op_frames = self.metrics.no_op_frames.saturating_add(1);
            return Ok(FrameSubmission::Unchanged);
        }
        let bytes =
            synchronized_bytes(payload, BEGIN_SYNCHRONIZED_UPDATE, END_SYNCHRONIZED_UPDATE)?;
        let capacity = bytes.len();
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        let frame = SerializedFrame::new(
            sequence,
            bytes,
            kind,
            demand,
            self.clock.timestamp(started_at),
            self.clock.timestamp(render_ended_instant),
            self.clock.clone(),
        );
        match self.sender.try_send(frame) {
            Ok(()) => {
                self.record_accepted(sequence, capacity, kind);
                Ok(FrameSubmission::Accepted(kind))
            }
            Err(TrySendError::Full(_)) => {
                self.metrics.frames_coalesced = self.metrics.frames_coalesced.saturating_add(1);
                self.full_repaint_required = true;
                Ok(FrameSubmission::ResyncRequired)
            }
            Err(TrySendError::Disconnected(_)) => {
                self.full_repaint_required = true;
                Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "terminal frame writer disconnected",
                ))
            }
        }
    }

    fn record_accepted(&mut self, sequence: u64, byte_count: usize, kind: FrameKind) {
        self.metrics.frames_submitted = self.metrics.frames_submitted.saturating_add(1);
        self.metrics.bytes_submitted = self
            .metrics
            .bytes_submitted
            .saturating_add(u64::try_from(byte_count).unwrap_or(u64::MAX));
        self.in_flight = Some(sequence);
        if matches!(kind, FrameKind::FullRepaint) {
            self.metrics.full_repaints = self.metrics.full_repaints.saturating_add(1);
            self.full_repaint_required = false;
        }
    }

    pub fn abort_frame(&mut self) {
        let mut capture = capture_lock(&self.capture);
        capture.active = false;
        capture.bytes.clear();
        self.active_kind = None;
        self.active_demand = None;
        self.frame_started_at = None;
        self.full_repaint_required = true;
    }

    pub fn require_full_repaint(&mut self) {
        self.full_repaint_required = true;
    }

    pub const fn requires_full_repaint(&self) -> bool {
        self.full_repaint_required
    }

    pub const fn metrics(&self) -> FrameOutputMetrics {
        self.metrics
    }

    pub fn writer_metrics(&self) -> FrameWriterMetrics {
        *writer_metrics_lock(&self.writer_metrics)
    }

    pub fn take_acknowledgements(&mut self) -> Vec<FrameAck> {
        let _ = self.is_ready_for_frame();
        std::mem::take(&mut self.completed_acknowledgements)
    }
}
