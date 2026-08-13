#![allow(
    clippy::expect_used,
    reason = "integration owner tests use fail-fast asserts"
)]

use harness_tui::terminal::{FrameKind, FrameOutput, FrameOutputBackend, FrameSubmission};
use harness_tui::terminal_title::TitleWriter;
use ratatui::{backend::Backend, layout::Position};

#[test]
fn unchanged_title_and_cursor_emit_no_physical_frame() {
    // Given: a presented title and cursor state at the terminal serialization boundary.
    let (mut output, writer, receiver) = FrameOutput::bounded(1);
    let mut backend = FrameOutputBackend::new(writer);
    let mut title_writer = TitleWriter::new();
    output.begin_frame().expect("begin initial frame");
    assert_eq!(
        title_writer.write_title("harness — session — idle", &mut backend),
        Ok(true)
    );
    backend.show_cursor().expect("show cursor");
    backend
        .set_cursor_position(Position::new(4, 2))
        .expect("position cursor");
    output.finish_frame().expect("finish initial frame");
    let initial = receiver.try_recv().expect("initial title frame");
    receiver.acknowledge(&initial).expect("acknowledge frame");
    assert!(output.is_ready_for_frame());

    // When: the next frame repeats the same title, visibility, and cursor position.
    output.begin_frame().expect("begin unchanged frame");
    assert_eq!(
        title_writer.write_title("harness — session — idle", &mut backend),
        Ok(false)
    );
    backend.show_cursor().expect("repeat cursor visibility");
    backend
        .set_cursor_position(Position::new(4, 2))
        .expect("repeat cursor position");
    let submission = output.finish_frame().expect("finish unchanged frame");

    // Then: no title command, synchronized marker, or physical frame is queued.
    assert_eq!(submission, FrameSubmission::Unchanged);
    assert!(receiver.try_recv().is_err());
}

#[test]
fn changed_title_emits_one_physical_frame() {
    // Given: one title has already reached the terminal serialization boundary.
    let (mut output, writer, receiver) = FrameOutput::bounded(1);
    let mut backend = FrameOutputBackend::new(writer);
    let mut title_writer = TitleWriter::new();
    output.begin_frame().expect("begin initial frame");
    title_writer
        .write_title("harness — session — idle", &mut backend)
        .expect("write initial title");
    output.finish_frame().expect("finish initial frame");
    let initial = receiver.try_recv().expect("initial title frame");
    receiver.acknowledge(&initial).expect("acknowledge frame");
    assert!(output.is_ready_for_frame());

    // When: the title candidate changes once.
    output.begin_frame().expect("begin changed frame");
    assert_eq!(
        title_writer.write_title("harness — session — streaming", &mut backend),
        Ok(true)
    );
    let submission = output.finish_frame().expect("finish changed frame");
    let frame = receiver.try_recv().expect("changed title frame");

    // Then: exactly one changed OSC title is accepted for physical presentation.
    assert_eq!(
        submission,
        FrameSubmission::Accepted(FrameKind::Differential)
    );
    let expected = b"\x1b]2;harness \xe2\x80\x94 session \xe2\x80\x94 streaming\x07";
    assert_eq!(
        frame
            .bytes()
            .windows(expected.len())
            .filter(|window| *window == expected)
            .count(),
        1
    );
}
