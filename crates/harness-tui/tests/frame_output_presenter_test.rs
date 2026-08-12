use std::io::Write;

use harness_tui::terminal::{FrameKind, FrameOutput, FrameSubmission, Presenter};

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
