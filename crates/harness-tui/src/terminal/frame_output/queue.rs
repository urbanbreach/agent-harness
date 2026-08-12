use std::io::{self, Write};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};

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

#[derive(Debug)]
pub struct SerializedFrame {
    sequence: u64,
    bytes: Vec<u8>,
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
}

impl FrameOutput {
    pub fn bounded(capacity: usize) -> (Self, FrameOutputWriter, FrameOutputReceiver) {
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let (acknowledge, acknowledgements) = mpsc::channel();
        let capture = Arc::new(Mutex::new(CaptureState::default()));
        (
            Self {
                sender,
                acknowledgements,
                capture: Arc::clone(&capture),
                active_kind: None,
                full_repaint_required: false,
                next_sequence: 1,
                in_flight: None,
            },
            FrameOutputWriter { capture },
            FrameOutputReceiver {
                receiver,
                acknowledge,
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
        if !self.is_ready_for_frame() {
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
        Ok(kind)
    }

    pub fn finish_frame(&mut self) -> io::Result<FrameSubmission> {
        let kind = self.active_kind.take().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "finish_frame called with no active terminal frame",
            )
        })?;
        let payload = {
            let mut capture = capture_lock(&self.capture);
            capture.active = false;
            std::mem::take(&mut capture.bytes)
        };
        if payload.is_empty() {
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
        match self.sender.try_send(SerializedFrame { sequence, bytes }) {
            Ok(()) => {
                self.in_flight = Some(sequence);
                if matches!(kind, FrameKind::FullRepaint) {
                    self.full_repaint_required = false;
                }
                Ok(FrameSubmission::Accepted(kind))
            }
            Err(TrySendError::Full(_)) => {
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
        self.full_repaint_required = true;
    }

    pub fn require_full_repaint(&mut self) {
        self.full_repaint_required = true;
    }

    pub const fn requires_full_repaint(&self) -> bool {
        self.full_repaint_required
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
