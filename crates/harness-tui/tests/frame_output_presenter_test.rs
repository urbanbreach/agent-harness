use std::io::Write;

use harness_tui::terminal::{
    FrameKind, FrameOutput, FrameOutputBackend, FrameOutputFailure, FrameSubmission,
    FrameWriteStage,
};
use ratatui::backend::{Backend, ClearType};

#[test]
fn output_waits_for_physical_ack_before_accepting_another_frame() {
    // arrange
    // Given: an output channel with spare queue capacity.
    let (mut output, mut writer, receiver) = FrameOutput::bounded(2);

    // When: one differential frame is submitted but not yet acknowledged.
    assert_eq!(output.begin_frame().unwrap(), FrameKind::Differential);
    writer.write_all(b"first frame").unwrap();
    assert_eq!(
        output.finish_frame().unwrap(),
        FrameSubmission::Accepted(FrameKind::Differential)
    );

    // act
    // Then: presentation remains gated until the writer confirms the physical flush.
    // assert
    assert!(!output.is_ready_for_frame());
    let frame = receiver.try_recv().unwrap();
    receiver.acknowledge(&frame).unwrap();
    assert!(output.is_ready_for_frame());
}

#[test]
fn writer_failure_is_typed_and_keeps_the_frame_slot_fatal() {
    // arrange
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

    // act
    let (mut output, mut writer, receiver) = FrameOutput::bounded(1);
    output.begin_frame().unwrap();
    writer.write_all(b"frame").unwrap();
    output.finish_frame().unwrap();
    // assert
    assert!(receiver.write_next(&mut FailedWrite).is_err());
    assert!(output.is_ready_for_frame());
    assert_eq!(
        output.take_fatal_failure(),
        Some(FrameOutputFailure::Write(FrameWriteStage::Write))
    );
}

#[test]
fn presenter_metrics_distinguish_requests_submissions_noops_and_bytes() {
    // arrange
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

    // act
    let metrics = output.metrics();
    // assert
    assert_eq!(metrics.redraw_requests, 2);
    assert_eq!(metrics.frames_submitted, 1);
    assert_eq!(metrics.no_op_frames, 1);
    assert_eq!(metrics.bytes_submitted, payload_len);
}

#[test]
fn full_repaint_clear_is_captured_inside_the_physical_frame() {
    // arrange
    // act
    let (mut output, writer, _receiver) = FrameOutput::bounded(1);
    let mut backend = FrameOutputBackend::new(writer);
    output.require_full_repaint();

    // assert
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
    // arrange
    let (_output, writer, _receiver) = FrameOutput::bounded(1);
    let mut backend = FrameOutputBackend::new(writer);

    // act
    backend.prepare_for_terminal_drop();
    backend
        .show_cursor()
        // assert
        .expect("drop cursor is already restored");
}
