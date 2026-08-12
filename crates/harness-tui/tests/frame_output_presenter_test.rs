use std::io::Write;

use harness_tui::terminal::{FrameKind, FrameOutput, FrameSubmission};

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
