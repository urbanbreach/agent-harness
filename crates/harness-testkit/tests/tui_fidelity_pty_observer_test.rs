use harness_testkit::tui_fidelity::Viewport;
use harness_testkit::tui_fidelity_runner::{
    ObservationKind, PtyObservationError, PtyObserver, PtyRead,
};

#[test]
fn split_sync_delimiters_form_one_frame() {
    let mut observer = PtyObserver::new(Viewport { cols: 8, rows: 2 });
    for (completed_at_micros, bytes) in [
        (10, b"\x1b[?20".as_slice()),
        (11, b"26hhello".as_slice()),
        (12, b"\x1b[?202".as_slice()),
        (13, b"6l".as_slice()),
    ] {
        observer.observe(&PtyRead {
            completed_at_micros,
            bytes: bytes.to_vec(),
        });
    }
    let (reads, observations) = observer.finish(3).expect("complete stream");
    assert_eq!(reads.len(), 4);
    assert_eq!(
        observations[0].kind,
        ObservationKind::SynchronizedUpdateComplete
    );
    assert_eq!(observations.len(), 4);
}

#[test]
fn truncated_stream_cannot_complete_trace() {
    let mut observer = PtyObserver::new(Viewport { cols: 8, rows: 2 });
    observer.observe(&PtyRead {
        completed_at_micros: 10,
        bytes: b"\x1b[?2026hhello".to_vec(),
    });
    assert_eq!(
        observer.finish(3),
        Err(PtyObservationError::TruncatedStream)
    );
}
