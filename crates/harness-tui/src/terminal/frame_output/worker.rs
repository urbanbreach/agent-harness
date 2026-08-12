use crossbeam_channel::{Receiver, Sender, TryRecvError};
use std::io::{self, Write};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use super::model::{
    FrameAck, FrameAckOutcome, FrameWriteStage, FrameWriterMetrics, SerializedFrame,
};

#[derive(Debug)]
pub struct FrameOutputReceiver {
    receiver: Receiver<SerializedFrame>,
    acknowledge: Sender<FrameAck>,
    writer_metrics: Arc<Mutex<FrameWriterMetrics>>,
}

impl FrameOutputReceiver {
    pub(super) fn new(
        receiver: Receiver<SerializedFrame>,
        acknowledge: Sender<FrameAck>,
        writer_metrics: Arc<Mutex<FrameWriterMetrics>>,
    ) -> Self {
        Self {
            receiver,
            acknowledge,
            writer_metrics,
        }
    }

    pub fn try_recv(&self) -> Result<SerializedFrame, TryRecvError> {
        self.receiver.try_recv()
    }

    pub fn acknowledge(&self, frame: &SerializedFrame) -> io::Result<()> {
        let now = frame.clock.now();
        let ack = FrameAck {
            sequence: frame.sequence,
            revision: frame.demand.target_revision,
            cause_ids: frame.demand.cause_ids.clone(),
            requested_at: frame.demand.earliest_requested_at,
            render_started_at: frame.render_started_at,
            render_ended_at: frame.render_ended_at,
            submitted_at: frame.submitted_at,
            write_started_at: now,
            write_ended_at: now,
            acknowledged_at: now,
            frame_kind: frame.kind,
            byte_count: frame.bytes.len(),
            byte_sha256: frame.byte_sha256.clone(),
            outcome: FrameAckOutcome::Success,
        };
        self.acknowledge
            .send(ack)
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "frame producer disconnected"))
    }

    pub fn write_next<W: Write>(&self, sink: &mut W) -> io::Result<()> {
        let frame = self.receiver.recv().map_err(|_| {
            io::Error::new(io::ErrorKind::BrokenPipe, "frame producer disconnected")
        })?;
        write_frame(frame, sink, &self.acknowledge, &self.writer_metrics)
    }

    pub(crate) fn spawn<W>(self, sink: W) -> io::Result<FrameOutputWorker<W>>
    where
        W: Write + Send + 'static,
    {
        spawn_writer(self.receiver, self.acknowledge, self.writer_metrics, sink)
    }
}

pub(super) fn writer_metrics_lock(
    metrics: &Mutex<FrameWriterMetrics>,
) -> MutexGuard<'_, FrameWriterMetrics> {
    match metrics.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub(super) fn write_frame<W: Write>(
    frame: SerializedFrame,
    sink: &mut W,
    acknowledge: &Sender<FrameAck>,
    writer_metrics: &Arc<Mutex<FrameWriterMetrics>>,
) -> io::Result<()> {
    let write_started_instant = Instant::now();
    let write_started_at = frame.clock.timestamp(write_started_instant);
    if let Err(error) = sink.write_all(frame.bytes()) {
        send_ack(
            frame,
            write_started_at,
            FrameAckOutcome::Failure {
                stage: FrameWriteStage::Write,
            },
            acknowledge,
        );
        return Err(error);
    }
    if let Err(error) = sink.flush() {
        send_ack(
            frame,
            write_started_at,
            FrameAckOutcome::Failure {
                stage: FrameWriteStage::Flush,
            },
            acknowledge,
        );
        return Err(error);
    }
    let latency_micros =
        u64::try_from(frame.submitted_instant.elapsed().as_micros()).unwrap_or(u64::MAX);
    {
        let mut metrics = writer_metrics_lock(writer_metrics);
        metrics.frames_written = metrics.frames_written.saturating_add(1);
        metrics.bytes_written = metrics
            .bytes_written
            .saturating_add(u64::try_from(frame.bytes.len()).unwrap_or(u64::MAX));
        metrics.writer_latency_micros =
            metrics.writer_latency_micros.saturating_add(latency_micros);
        metrics.max_writer_latency_micros = metrics.max_writer_latency_micros.max(latency_micros);
    }
    send_ack(
        frame,
        write_started_at,
        FrameAckOutcome::Success,
        acknowledge,
    );
    Ok(())
}

fn send_ack(
    frame: SerializedFrame,
    write_started_at: crate::presentation::PresentationTimestamp,
    outcome: FrameAckOutcome,
    acknowledge: &Sender<FrameAck>,
) {
    let write_ended_at = frame.clock.now();
    let acknowledged_at = frame.clock.now();
    let ack = FrameAck {
        sequence: frame.sequence,
        revision: frame.demand.target_revision,
        cause_ids: frame.demand.cause_ids,
        requested_at: frame.demand.earliest_requested_at,
        render_started_at: frame.render_started_at,
        render_ended_at: frame.render_ended_at,
        submitted_at: frame.submitted_at,
        write_started_at,
        write_ended_at,
        acknowledged_at,
        frame_kind: frame.kind,
        byte_count: frame.bytes.len(),
        byte_sha256: frame.byte_sha256,
        outcome,
    };
    let _ = acknowledge.send(ack);
}

pub(super) fn spawn_writer<W>(
    receiver: Receiver<SerializedFrame>,
    acknowledge: Sender<FrameAck>,
    writer_metrics: Arc<Mutex<FrameWriterMetrics>>,
    mut sink: W,
) -> io::Result<FrameOutputWorker<W>>
where
    W: Write + Send + 'static,
{
    let handle = thread::Builder::new()
        .name("harness-terminal-writer".to_string())
        .spawn(move || {
            while let Ok(frame) = receiver.recv() {
                write_frame(frame, &mut sink, &acknowledge, &writer_metrics)?;
            }
            Ok(sink)
        })?;
    Ok(FrameOutputWorker {
        handle: Some(handle),
    })
}

#[derive(Debug)]
pub(crate) struct FrameOutputWorker<W: Write + Send + 'static> {
    handle: Option<JoinHandle<io::Result<W>>>,
}

impl<W: Write + Send + 'static> FrameOutputWorker<W> {
    pub(crate) fn join(mut self) -> io::Result<W> {
        let handle = self.handle.take().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "terminal frame writer already joined",
            )
        })?;
        handle
            .join()
            .map_err(|_| io::Error::other("terminal frame writer panicked"))?
    }
}
