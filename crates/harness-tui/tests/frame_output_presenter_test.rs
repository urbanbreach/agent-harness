use std::io::Write;

use harness_tui::presentation::{
    CauseId, PresentationRevision, PresentationTimestamp, RenderDemand, RenderReason,
};
use harness_tui::terminal::{
    FrameKind, FrameOutput, FrameOutputBackend, FrameOutputFailure, FrameSubmission,
    FrameWriteStage, Presenter,
};
use ratatui::backend::{Backend, ClearType};

#[test]
fn output_waits_for_physical_ack_before_accepting_another_frame() {
    // Given: an output channel with spare queue capacity.
    let (mut output, mut writer, receiver) = FrameOutput::bounded(2);

    // When: one differential frame is submitted but not yet acknowledged.
    assert_eq!(output.begin_frame().unwrap(), FrameKind::Differential);
    writer.write_all(b"first frame").unwrap();
    assert_eq!(
        output.finish_frame().unwrap(),
        FrameSubmission::Accepted(FrameKind::Differential)
    );

    // Then: presentation remains gated until the writer confirms the physical flush.
    assert!(!output.is_ready_for_frame());
    let frame = receiver.try_recv().unwrap();
    receiver.acknowledge(&frame).unwrap();
    assert!(output.is_ready_for_frame());
}

#[test]
fn writer_failure_is_typed_and_keeps_the_frame_slot_fatal() {
    struct FailedWrite;
    impl Write for FailedWrite {
        fn write(&mut self, _buffer: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "defect",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let (mut output, mut writer, receiver) = FrameOutput::bounded(1);
    output.begin_frame().unwrap();
    writer.write_all(b"frame").unwrap();
    output.finish_frame().unwrap();
    assert!(receiver.write_next(&mut FailedWrite).is_err());
    assert!(output.is_ready_for_frame());
    assert_eq!(
        output.take_fatal_failure(),
        Some(FrameOutputFailure::Write(FrameWriteStage::Write))
    );
}

#[test]
fn presenter_metrics_distinguish_requests_submissions_noops_and_bytes() {
    let (mut output, mut writer, receiver) = FrameOutput::bounded(1);

    assert_eq!(output.begin_frame().unwrap(), FrameKind::Differential);
    assert_eq!(output.finish_frame().unwrap(), FrameSubmission::Unchanged);
    assert_eq!(output.begin_frame().unwrap(), FrameKind::Differential);
    writer.write_all(b"changed").unwrap();
    assert_eq!(
        output.finish_frame().unwrap(),
        FrameSubmission::Accepted(FrameKind::Differential)
    );
    let frame = receiver.try_recv().unwrap();
    let payload_len = u64::try_from(frame.bytes().len()).unwrap_or(u64::MAX);

    let metrics = output.metrics();
    assert_eq!(metrics.redraw_requests, 2);
    assert_eq!(metrics.frames_submitted, 1);
    assert_eq!(metrics.no_op_frames, 1);
    assert_eq!(metrics.bytes_submitted, payload_len);
}

#[test]
fn presenter_retains_dirty_work_across_writer_backpressure() {
    let now = std::time::Instant::now();
    let mut presenter = Presenter::new();

    assert!(!presenter.should_present(false));
    assert!(presenter.should_present(true));
    presenter.record_submission(FrameSubmission::ResyncRequired, now);
    assert!(presenter.should_present(true));
    presenter.record_submission(FrameSubmission::Accepted(FrameKind::FullRepaint), now);

    assert!(!presenter.should_present(true));
    assert!(!presenter.force_full_repaint());
}

#[test]
fn presenter_preserves_coalesced_demand_until_submission() {
    // Given: two revision-bearing redraw requests arrive before presentation.
    let now = std::time::Instant::now();
    let mut presenter = Presenter::new();
    presenter.request_redraw_for(
        RenderDemand::new(
            PresentationRevision::new(1),
            CauseId::new("trace:cause:1"),
            PresentationTimestamp::from_micros(10),
            RenderReason::TerminalInput,
        ),
        now,
    );
    presenter.request_redraw_for(
        RenderDemand::new(
            PresentationRevision::new(2),
            CauseId::new("trace:cause:2"),
            PresentationTimestamp::from_micros(20),
            RenderReason::LiveUpdate,
        ),
        now,
    );

    // When: the presenter releases the demand for frame capture.
    let demand = presenter.take_render_demand().expect("coalesced demand");

    // Then: both causes and the newest revision remain ordered and intact.
    assert_eq!(demand.target_revision, PresentationRevision::new(2));
    assert_eq!(
        demand.cause_ids,
        vec![CauseId::new("trace:cause:1"), CauseId::new("trace:cause:2")]
    );
}

#[test]
fn full_repaint_clear_is_captured_inside_the_physical_frame() {
    let (mut output, writer, _receiver) = FrameOutput::bounded(1);
    let mut backend = FrameOutputBackend::new(writer);
    output.require_full_repaint();

    assert_eq!(
        output.begin_frame().expect("begin frame"),
        FrameKind::FullRepaint
    );
    let cursor = backend.get_cursor_position().expect("tracked cursor");
    backend.clear_region(ClearType::All).expect("capture clear");
    backend
        .set_cursor_position(cursor)
        .expect("restore tracked cursor");
    assert_eq!(
        output.finish_frame().expect("finish frame"),
        FrameSubmission::Accepted(FrameKind::FullRepaint)
    );
}

#[test]
fn terminal_drop_cursor_restore_does_not_escape_frame_capture() {
    let (_output, writer, _receiver) = FrameOutput::bounded(1);
    let mut backend = FrameOutputBackend::new(writer);

    backend.prepare_for_terminal_drop();
    backend
        .show_cursor()
        .expect("drop cursor is already restored");
}
