use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use harness_tui::presentation::{
    CauseId, PresentationCause, PresentationCauseKind, PresentationClock, PresentationOutcome,
    PresentationRevision, PresentationTimestamp, PresentationTrace, RenderDemand, RenderReason,
};
use harness_tui::terminal::{FrameAckOutcome, FrameKind, FrameOutput, FrameSubmission};

fn cause(id: &str, received_at_micros: u64) -> PresentationCause {
    PresentationCause::new(
        CauseId::new(id),
        PresentationCauseKind::TerminalInput,
        PresentationTimestamp::from_micros(received_at_micros),
        None,
    )
}

#[derive(Clone, Debug, Default)]
struct SharedSink(Arc<Mutex<Vec<u8>>>);

impl SharedSink {
    fn bytes(&self) -> Vec<u8> {
        match self.0.lock() {
            Ok(bytes) => bytes.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

impl Write for SharedSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        match self.0.lock() {
            Ok(mut sink) => sink.extend_from_slice(bytes),
            Err(poisoned) => poisoned.into_inner().extend_from_slice(bytes),
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Debug, Default)]
struct FlushFailingSink;

impl Write for FlushFailingSink {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("injected flush failure"))
    }
}

#[test]
fn coalesced_causes_are_preserved_through_ack() {
    // Given: two visible causes coalesced into one revision-bearing demand.
    let clock = PresentationClock::new();
    let first = CauseId::new("trace:cause:1");
    let second = CauseId::new("trace:cause:2");
    let mut demand = RenderDemand::new(
        PresentationRevision::new(1),
        first.clone(),
        clock.now(),
        RenderReason::TerminalInput,
    );
    demand.coalesce(
        PresentationRevision::new(2),
        second.clone(),
        clock.now(),
        RenderReason::LiveUpdate,
    );
    let (mut output, mut writer, receiver) = FrameOutput::bounded_with_clock(1, clock);
    let sink = SharedSink::default();
    let observed_sink = sink.clone();

    // When: the physical writer accepts and flushes the one coalesced frame.
    assert_eq!(
        output.begin_frame_for(demand).expect("begin frame"),
        FrameKind::Differential
    );
    writer.write_all(b"coalesced frame").expect("capture frame");
    assert_eq!(
        output.finish_frame().expect("finish frame"),
        FrameSubmission::Accepted(FrameKind::Differential)
    );
    receiver
        .write_next(&mut sink.clone())
        .expect("write physical frame");
    assert!(output.is_ready_for_frame());
    let ack = output
        .take_acknowledgements()
        .pop()
        .expect("one acknowledgement");

    // Then: cause order, newest revision, byte identity, and flush timing survive intact.
    assert_eq!(ack.revision(), PresentationRevision::new(2));
    assert_eq!(ack.cause_ids(), &[first, second]);
    assert_eq!(ack.kind(), FrameKind::Differential);
    assert_eq!(ack.byte_count(), observed_sink.bytes().len());
    assert_eq!(ack.byte_sha256().len(), 64);
    assert!(matches!(ack.outcome(), FrameAckOutcome::Success));
    assert!(ack.write_started_at() <= ack.write_ended_at());
    assert!(ack.write_ended_at() <= ack.acknowledged_at());
}

#[test]
fn no_op_cause_closes_without_visible_frame() {
    // Given: a received cause whose render emits no terminal bytes.
    let mut trace = PresentationTrace::new("trace");
    let cause = cause("trace:cause:1", 10);
    trace.record_cause(cause.clone()).expect("record cause");

    // When: the cause is closed as an unchanged render.
    trace
        .record_no_visible_change(cause.id().clone(), PresentationTimestamp::from_micros(20))
        .expect("close no-op cause");

    // Then: the outcome is explicit and no accepted frame is fabricated.
    assert!(matches!(
        trace.outcomes().first(),
        Some(PresentationOutcome::NoVisibleChange { cause_id, .. }) if cause_id == cause.id()
    ));
    assert!(trace.frames().is_empty());
}

#[test]
fn resync_requires_a_linked_replacement_full_repaint() {
    // Given: a rejected differential demand followed by a newer repaint demand.
    let mut trace = PresentationTrace::new("trace");

    // When: the resync outcome is recorded with its replacement revision.
    trace
        .record_resync_required(
            PresentationRevision::new(4),
            PresentationRevision::new(5),
            PresentationTimestamp::from_micros(30),
        )
        .expect("record resync");

    // Then: the failed submission remains distinct from a written frame.
    assert!(matches!(
        trace.outcomes().first(),
        Some(PresentationOutcome::ResyncRequired {
            rejected_revision,
            replacement_revision,
            ..
        }) if *rejected_revision == PresentationRevision::new(4)
            && *replacement_revision == PresentationRevision::new(5)
    ));
    assert!(trace.frames().is_empty());
}

#[test]
fn failed_sink_has_no_success_ack() {
    // Given: one accepted frame and a sink whose physical flush fails.
    let clock = PresentationClock::new();
    let demand = RenderDemand::new(
        PresentationRevision::new(1),
        CauseId::new("trace:cause:1"),
        clock.now(),
        RenderReason::TerminalInput,
    );
    let (mut output, mut writer, receiver) = FrameOutput::bounded_with_clock(1, clock);
    output.begin_frame_for(demand).expect("begin frame");
    writer.write_all(b"failed frame").expect("capture frame");
    assert!(matches!(
        output.finish_frame().expect("finish frame"),
        FrameSubmission::Accepted(FrameKind::Differential)
    ));

    // When: the writer attempts the physical flush.
    assert!(receiver.write_next(&mut FlushFailingSink).is_err());
    let _ = output.is_ready_for_frame();
    let acknowledgements = output.take_acknowledgements();

    // Then: the accepted frame has exactly one typed failure and no success acknowledgement.
    assert_eq!(acknowledgements.len(), 1);
    assert!(matches!(
        acknowledgements[0].outcome(),
        FrameAckOutcome::Failure { .. }
    ));
    assert!(!acknowledgements
        .iter()
        .any(|ack| matches!(ack.outcome(), FrameAckOutcome::Success)));
    assert_eq!(output.writer_metrics().frames_written, 0);
}
