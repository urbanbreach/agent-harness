use std::io::{self, Write};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crate::terminal::writer::{BEGIN_SYNCHRONIZED_UPDATE, END_SYNCHRONIZED_UPDATE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Debug)]
pub struct SerializedFrame {
    sequence: u64,
    bytes: Vec<u8>,
    submitted_at: Instant,
}

impl SerializedFrame {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }
}

#[derive(Debug, Default)]
struct CaptureState {
    active: bool,
    bytes: Vec<u8>,
}

fn writer_metrics_lock(metrics: &Mutex<FrameWriterMetrics>) -> MutexGuard<'_, FrameWriterMetrics> {
    match metrics.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn capture_lock(state: &Mutex<CaptureState>) -> MutexGuard<'_, CaptureState> {
    match state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[derive(Debug)]
pub struct FrameOutput {
    sender: SyncSender<SerializedFrame>,
    acknowledgements: Receiver<u64>,
    capture: Arc<Mutex<CaptureState>>,
    active_kind: Option<FrameKind>,
    full_repaint_required: bool,
    next_sequence: u64,
    in_flight: Option<u64>,
    metrics: FrameOutputMetrics,
    writer_metrics: Arc<Mutex<FrameWriterMetrics>>,
    frame_started_at: Option<Instant>,
}

impl FrameOutput {
    pub fn bounded(capacity: usize) -> (Self, FrameOutputWriter, FrameOutputReceiver) {
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let (acknowledge, acknowledgements) = mpsc::channel();
        let capture = Arc::new(Mutex::new(CaptureState::default()));
        let writer_metrics = Arc::new(Mutex::new(FrameWriterMetrics::default()));
        (
            Self {
                sender,
                acknowledgements,
                capture: Arc::clone(&capture),
                active_kind: None,
                full_repaint_required: false,
                next_sequence: 1,
                in_flight: None,
                metrics: FrameOutputMetrics::default(),
                writer_metrics: Arc::clone(&writer_metrics),
                frame_started_at: None,
            },
            FrameOutputWriter { capture },
            FrameOutputReceiver {
                receiver,
                acknowledge,
                writer_metrics,
            },
        )
    }

    pub fn is_ready_for_frame(&mut self) -> bool {
        loop {
            match self.acknowledgements.try_recv() {
                Ok(sequence) if self.in_flight == Some(sequence) => self.in_flight = None,
                Ok(_) => self.full_repaint_required = true,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.in_flight = None;
                    break;
                }
            }
        }
        self.in_flight.is_none()
    }

    pub const fn has_in_flight_frame(&self) -> bool {
        self.in_flight.is_some()
    }

    pub fn begin_frame(&mut self) -> io::Result<FrameKind> {
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
        capture.active = true;
        self.active_kind = Some(kind);
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
        if let Some(started_at) = self.frame_started_at.take() {
            let micros = u64::try_from(started_at.elapsed().as_micros()).unwrap_or(u64::MAX);
            self.metrics.frame_build_time_micros =
                self.metrics.frame_build_time_micros.saturating_add(micros);
            self.metrics.max_frame_build_time_micros =
                self.metrics.max_frame_build_time_micros.max(micros);
        }
        let payload = {
            let mut capture = capture_lock(&self.capture);
            capture.active = false;
            std::mem::take(&mut capture.bytes)
        };
        if payload.is_empty() {
            self.metrics.no_op_frames = self.metrics.no_op_frames.saturating_add(1);
            return Ok(FrameSubmission::Unchanged);
        }

        let capacity = BEGIN_SYNCHRONIZED_UPDATE
            .len()
            .checked_add(payload.len())
            .and_then(|len| len.checked_add(END_SYNCHRONIZED_UPDATE.len()))
            .ok_or_else(|| io::Error::other("terminal frame size overflow"))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| io::Error::other("terminal frame allocation failed"))?;
        bytes.extend_from_slice(BEGIN_SYNCHRONIZED_UPDATE);
        bytes.extend_from_slice(&payload);
        bytes.extend_from_slice(END_SYNCHRONIZED_UPDATE);

        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        match self.sender.try_send(SerializedFrame {
            sequence,
            bytes,
            submitted_at: Instant::now(),
        }) {
            Ok(()) => {
                self.metrics.frames_submitted = self.metrics.frames_submitted.saturating_add(1);
                self.metrics.bytes_submitted = self
                    .metrics
                    .bytes_submitted
                    .saturating_add(u64::try_from(capacity).unwrap_or(u64::MAX));
                self.in_flight = Some(sequence);
                if matches!(kind, FrameKind::FullRepaint) {
                    self.metrics.full_repaints = self.metrics.full_repaints.saturating_add(1);
                    self.full_repaint_required = false;
                }
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

    pub fn abort_frame(&mut self) {
        let mut capture = capture_lock(&self.capture);
        capture.active = false;
        capture.bytes.clear();
        self.active_kind = None;
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
}

#[derive(Clone, Debug)]
pub struct FrameOutputWriter {
    capture: Arc<Mutex<CaptureState>>,
}

impl Write for FrameOutputWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut capture = capture_lock(&self.capture);
        if !capture.active {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "terminal command emitted outside an active frame",
            ));
        }
        capture
            .bytes
            .try_reserve(bytes.len())
            .map_err(|_| io::Error::other("terminal frame allocation failed"))?;
        capture.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
pub struct FrameOutputReceiver {
    receiver: Receiver<SerializedFrame>,
    acknowledge: Sender<u64>,
    writer_metrics: Arc<Mutex<FrameWriterMetrics>>,
}

impl FrameOutputReceiver {
    pub fn try_recv(&self) -> Result<SerializedFrame, TryRecvError> {
        self.receiver.try_recv()
    }

    pub fn acknowledge(&self, frame: &SerializedFrame) -> io::Result<()> {
        self.acknowledge
            .send(frame.sequence())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "frame producer disconnected"))
    }

    pub(crate) fn spawn<W>(self, sink: W) -> io::Result<FrameOutputWorker<W>>
    where
        W: Write + Send + 'static,
    {
        let handle = thread::Builder::new()
            .name("harness-terminal-writer".to_string())
            .spawn(move || self.write_all_to(sink))?;
        Ok(FrameOutputWorker {
            handle: Some(handle),
        })
    }

    fn write_all_to<W: Write>(self, mut sink: W) -> io::Result<W> {
        while let Ok(frame) = self.receiver.recv() {
            sink.write_all(frame.bytes())?;
            sink.flush()?;
            let latency_micros =
                u64::try_from(frame.submitted_at.elapsed().as_micros()).unwrap_or(u64::MAX);
            let mut metrics = writer_metrics_lock(&self.writer_metrics);
            metrics.frames_written = metrics.frames_written.saturating_add(1);
            metrics.bytes_written = metrics
                .bytes_written
                .saturating_add(u64::try_from(frame.bytes().len()).unwrap_or(u64::MAX));
            metrics.writer_latency_micros =
                metrics.writer_latency_micros.saturating_add(latency_micros);
            metrics.max_writer_latency_micros =
                metrics.max_writer_latency_micros.max(latency_micros);
            drop(metrics);
            let _ = self.acknowledge.send(frame.sequence());
        }
        Ok(sink)
    }
}

#[derive(Debug)]
pub(crate) struct FrameOutputWorker<W: Write + Send + 'static> {
    handle: Option<JoinHandle<io::Result<W>>>,
}

impl<W: Write + Send + 'static> FrameOutputWorker<W> {
    pub(crate) fn join(mut self) -> io::Result<W> {
        let Some(handle) = self.handle.take() else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "terminal frame writer already joined",
            ));
        };
        handle
            .join()
            .map_err(|_| io::Error::other("terminal frame writer panicked"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_metrics_capture_physical_flush_latency_and_bytes() {
        let (mut output, mut writer, receiver) = FrameOutput::bounded(1);
        assert_eq!(output.begin_frame().unwrap(), FrameKind::Differential);
        writer.write_all(b"physical frame").unwrap();
        assert!(matches!(
            output.finish_frame().unwrap(),
            FrameSubmission::Accepted(FrameKind::Differential)
        ));
        let worker = receiver.spawn(Vec::<u8>::new()).unwrap();

        while !output.is_ready_for_frame() {
            std::thread::yield_now();
        }
        let metrics = output.writer_metrics();
        drop(output);
        let sink = worker.join().unwrap();

        assert_eq!(metrics.frames_written, 1);
        assert_eq!(metrics.bytes_written, u64::try_from(sink.len()).unwrap());
    }
}
