use ratatui::{backend::Backend, buffer::Cell};

use super::FrameOutputBackend;
use crate::terminal::frame_output::{set_frame_hyperlinks, FrameHyperlink, FrameOutput};

fn draw_frame(
    output: &mut FrameOutput,
    backend: &mut FrameOutputBackend,
    receiver: &crate::terminal::FrameOutputReceiver,
    cells: &[(u16, u16, Cell)],
    links: Vec<FrameHyperlink>,
) -> Vec<u8> {
    output.begin_frame().expect("begin frame");
    set_frame_hyperlinks(links);
    backend
        .draw(cells.iter().map(|(x, y, cell)| (*x, *y, cell)))
        .expect("draw frame");
    output.finish_frame().expect("finish frame");
    let frame = receiver.try_recv().expect("serialized frame");
    let bytes = frame.bytes.clone();
    receiver.acknowledge(&frame).expect("acknowledge frame");
    let _ = output.take_acknowledgements();
    bytes
}

#[test]
fn backend_balances_osc8_around_linked_cells_and_never_links_padding() {
    // Given: ordinary cells with a safe run covering only the label.
    let (mut output, writer, receiver) = FrameOutput::bounded(2);
    let mut backend = FrameOutputBackend::new(writer);
    let cells = [
        (0, 0, Cell::new("d")),
        (1, 0, Cell::new("o")),
        (2, 0, Cell::new("c")),
        (3, 0, Cell::new(" ")),
        (4, 0, Cell::new("│")),
    ];

    // When: the physical frame is emitted.
    let bytes = draw_frame(
        &mut output,
        &mut backend,
        &receiver,
        &cells,
        vec![FrameHyperlink {
            row: 0,
            start_column: 0,
            end_column: 3,
            destination: "https://example.com/docs".to_string(),
        }],
    );
    let output = String::from_utf8_lossy(&bytes);

    // Then: OSC-8 closes before padding and the border, with no control bytes in cells.
    let open = output
        .find("https://example.com/docs")
        .unwrap_or_else(|| panic!("missing OSC-8 destination: {output:?}"));
    let close = output[open..]
        .find("]8;;")
        .unwrap_or_else(|| panic!("missing OSC-8 close: {output:?}"))
        + open;
    assert!(open < close);
    assert!(output[close..].contains(' ') && output[close..].contains('│'));
    assert!(cells
        .iter()
        .all(|(_, _, cell)| !cell.symbol().contains('\x1b')));
}

#[test]
fn backend_repaints_same_label_when_url_changes_or_link_is_removed() {
    // Given: a stable visible cell whose destination changes and is then removed.
    let (mut output, writer, receiver) = FrameOutput::bounded(2);
    let mut backend = FrameOutputBackend::new(writer);
    let cell = Cell::new("x");
    let first = [FrameHyperlink {
        row: 0,
        start_column: 0,
        end_column: 1,
        destination: "https://example.com/one".to_string(),
    }];
    let _ = draw_frame(
        &mut output,
        &mut backend,
        &receiver,
        &[(0, 0, cell.clone())],
        first.to_vec(),
    );

    // When: no Ratatui cell changes, but URL metadata changes and then disappears.
    let changed = draw_frame(
        &mut output,
        &mut backend,
        &receiver,
        &[],
        vec![FrameHyperlink {
            row: 0,
            start_column: 0,
            end_column: 1,
            destination: "https://example.com/two".to_string(),
        }],
    );
    let removed = draw_frame(&mut output, &mut backend, &receiver, &[], Vec::new());

    // Then: both metadata transitions force physical redraw and unsafe bytes never leak.
    assert!(String::from_utf8_lossy(&changed).contains("https://example.com/two"));
    assert!(String::from_utf8_lossy(&removed).contains('x'));
    assert!(!String::from_utf8_lossy(&removed).contains("https://example.com/two"));
}

#[test]
fn backend_full_clear_does_not_repaint_old_link_cells_after_resize() {
    for region_clear in [false, true] {
        let (mut output, writer, receiver) = FrameOutput::bounded(2);
        let mut backend = FrameOutputBackend::new(writer);
        let _ = draw_frame(
            &mut output,
            &mut backend,
            &receiver,
            &[(54, 25, Cell::new("n"))],
            vec![FrameHyperlink {
                row: 25,
                start_column: 54,
                end_column: 55,
                destination: "https://example.com/old".to_string(),
            }],
        );

        output.begin_frame().expect("begin cleared frame");
        if region_clear {
            backend
                .clear_region(ratatui::backend::ClearType::All)
                .expect("clear full region");
        } else {
            backend.clear().expect("clear backend");
        }
        set_frame_hyperlinks(vec![FrameHyperlink {
            row: 25,
            start_column: 12,
            end_column: 13,
            destination: "https://example.com/new".to_string(),
        }]);
        let cell = Cell::new("x");
        backend
            .draw(std::iter::once((12, 25, &cell)))
            .expect("draw narrow frame");
        output.finish_frame().expect("finish cleared frame");
        let frame = receiver.try_recv().expect("serialized cleared frame");
        let bytes = frame.bytes.clone();
        receiver
            .acknowledge(&frame)
            .expect("acknowledge cleared frame");
        let _ = output.take_acknowledgements();
        let ansi = String::from_utf8_lossy(&bytes);
        assert!(ansi.contains("\x1b[2J"), "{ansi:?}");
        assert!(ansi.contains("\x1b[26;13H"), "{ansi:?}");
        assert!(ansi.contains("https://example.com/new"), "{ansi:?}");
        assert!(
            !ansi.contains("\x1b[26;55H"),
            "cleared linked cell replayed, region_clear={region_clear}: {ansi:?}"
        );

        let changed = draw_frame(
            &mut output,
            &mut backend,
            &receiver,
            &[],
            vec![FrameHyperlink {
                row: 25,
                start_column: 12,
                end_column: 13,
                destination: "https://example.com/changed".to_string(),
            }],
        );
        let ansi = String::from_utf8_lossy(&changed);
        assert!(ansi.contains("\x1b[26;13H"), "{ansi:?}");
        assert!(ansi.contains("https://example.com/changed"), "{ansi:?}");
        assert!(!ansi.contains("\x1b[26;55H"), "{ansi:?}");
    }
}

#[test]
fn backend_rejects_control_characters_inside_http_destination_at_final_admission() {
    // Given: an HTTP-looking destination containing BEL after its scheme.
    let (mut output, writer, receiver) = FrameOutput::bounded(1);
    let mut backend = FrameOutputBackend::new(writer);

    // When: backend admission processes the internally supplied run.
    let bytes = draw_frame(
        &mut output,
        &mut backend,
        &receiver,
        &[(0, 0, Cell::new("x"))],
        vec![FrameHyperlink {
            row: 0,
            start_column: 0,
            end_column: 1,
            destination: "https://example.com/\u{7}evil".to_string(),
        }],
    );

    // Then: neither OSC-8 nor the control-bearing target reaches output.
    let output = String::from_utf8_lossy(&bytes);
    assert!(!output.contains("]8;;") && !output.contains("example.com"));
}

#[test]
fn backend_rejects_unsafe_link_metadata_at_final_admission() {
    // Given: a control-bearing javascript destination in otherwise ordinary cells.
    let (mut output, writer, receiver) = FrameOutput::bounded(1);
    let mut backend = FrameOutputBackend::new(writer);

    // When: backend admission processes the run.
    let bytes = draw_frame(
        &mut output,
        &mut backend,
        &receiver,
        &[(0, 0, Cell::new("x"))],
        vec![FrameHyperlink {
            row: 0,
            start_column: 0,
            end_column: 1,
            destination: "javascript:\u{1b}]8;;evil".to_string(),
        }],
    );

    // Then: no OSC-8 sequence or unsafe destination reaches physical output.
    let output = String::from_utf8_lossy(&bytes);
    assert!(!output.contains("]8;;") && !output.contains("javascript:"));
}
