use harness_testkit::tui_fidelity::Viewport;
use harness_testkit::tui_fidelity_runner::{
    ObservationKind, PtyObservationError, PtyObserver, PtyRead,
};
use std::time::{Duration, Instant};

#[test]
fn two_synchronized_envelopes_in_one_read_produce_two_observations() {
    // arrange: one OS read carries two complete, semantically distinct update envelopes.
    let mut observer = PtyObserver::new(Viewport { cols: 8, rows: 2 });
    let read = PtyRead {
        completed_at_micros: 10,
        bytes: b"\x1b[?2026h\x1b[2J\x1b[HA\x1b[?2026l\x1b[?2026h\x1b[2J\x1b[HB\x1b[?2026l".to_vec(),
    };

    // act: the incremental observer consumes the single raw read.
    let started = Instant::now();
    observer.observe(&read);
    let (reads, observations) = observer.finish(3).expect("complete stream");
    let elapsed = started.elapsed();
    let serialized = serde_json::to_vec(&observations).expect("serialize observations");

    // assert: both envelopes retain order and raw-read provenance within the resource caps.
    assert_eq!(reads.len(), 1);
    assert_eq!(observations.len(), 5);
    assert_eq!(
        observations[0].kind,
        ObservationKind::SynchronizedUpdateComplete
    );
    assert_eq!(
        observations[1].kind,
        ObservationKind::SynchronizedUpdateComplete
    );
    assert_eq!(observations[0].raw_read_ordinals, [0]);
    assert_eq!(observations[1].raw_read_ordinals, [0]);
    assert_ne!(observations[0].frame, observations[1].frame);
    assert!(elapsed < Duration::from_millis(50), "elapsed={elapsed:?}");
    assert!(serialized.len() <= 64 * 1024, "bytes={}", serialized.len());
}

#[test]
fn split_delimiters_with_shared_read_boundary_preserve_both_envelopes() {
    // arrange: envelope one ends and envelope two begins inside the same middle raw read.
    let mut observer = PtyObserver::new(Viewport { cols: 8, rows: 2 });
    let reads = [
        b"\x1b[?20".as_slice(),
        b"26h\x1b[2J\x1b[HA\x1b[?2026l\x1b[?20".as_slice(),
        b"26h\x1b[2J\x1b[HB\x1b[?202".as_slice(),
        b"6l".as_slice(),
    ];

    // act: raw chunks are observed without altering their boundaries.
    for (completed_at_micros, bytes) in reads.into_iter().enumerate() {
        observer.observe(&PtyRead {
            completed_at_micros: completed_at_micros as u64,
            bytes: bytes.to_vec(),
        });
    }
    let (raw_reads, observations) = observer.finish(0).expect("complete stream");

    // assert: the two complete envelopes survive split and shared delimiter reads.
    assert_eq!(raw_reads.len(), 4);
    assert_eq!(observations.len(), 2);
    assert_eq!(observations[0].raw_read_ordinals, [1]);
    assert_eq!(observations[1].raw_read_ordinals, [3]);
    assert_ne!(observations[0].frame, observations[1].frame);
}

#[test]
fn split_sync_delimiters_form_one_frame() {
    // arrange
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
    // act
    let (reads, observations) = observer.finish(3).expect("complete stream");
    // assert
    assert_eq!(reads.len(), 4);
    assert_eq!(
        observations[0].kind,
        ObservationKind::SynchronizedUpdateComplete
    );
    assert_eq!(observations.len(), 4);
}

#[test]
fn truncated_stream_cannot_complete_trace() {
    // arrange
    // act
    let mut observer = PtyObserver::new(Viewport { cols: 8, rows: 2 });
    observer.observe(&PtyRead {
        completed_at_micros: 10,
        bytes: b"\x1b[?2026hhello".to_vec(),
    });
    // assert
    assert_eq!(
        observer.finish(3),
        Err(PtyObservationError::TruncatedStream)
    );
}
