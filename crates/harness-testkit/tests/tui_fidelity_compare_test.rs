#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast independent fixtures"
)]

use std::io::Cursor;

use harness_testkit::parity::motion::synthetic_passing_trace;
use harness_testkit::parity::{
    CellModifiers, CursorState, ResolvedRgb, SemanticCell, SemanticFrame, TraceSource,
};
use harness_testkit::tui_fidelity_compare::{
    compare_cells, compare_motion, compare_png_bytes, compare_presentation_timing, compare_timing,
    hash_artifacts, reject_self_comparison, scan_artifacts, verify_freshness, AcceptanceProfile,
    ArtifactInputs, ArtifactRoots, CellSnapshot, ComparatorError, FocusState, IdentityPixelSpan,
    MotionTrace, NativeTimingMetrics, PixelRect, PresentationTimingMetrics, TimingPhase,
    TimingTrace, ZOrderEntry,
};

#[test]
fn packet2_required_gates_include_timing_without_weakening_full_parity() {
    // arrange
    let full = AcceptanceProfile::FullParity.required_gates();
    // act
    let packet2 = AcceptanceProfile::Packet2Scheduling.required_gates();
    // assert
    assert_eq!(full.len(), 9);
    assert!(packet2.contains(&"timing"));
    assert!(full
        .iter()
        .all(|gate| packet2.contains(gate) || ["semantic_cell", "pixel", "motion"].contains(gate)));
}

#[test]
fn rejects_one_cell_glyph_style_cursor_focus_and_z_order_difference() {
    // arrange
    let expected = cell_snapshot();
    let mut changed = expected.clone();
    changed.frame.cells[0].grapheme = "x".to_owned();
    changed.frame.cells[0].modifiers = CellModifiers {
        bold: true,
        ..CellModifiers::default()
    };
    changed.frame.cursor = CursorState::visible_block(0, 1);
    changed.focus = Some(FocusState::new(0, 1));
    changed.z_order[0].depth = 1;

    // act
    let error = compare_cells(&expected, &changed, &Default::default())
        .expect_err("every exact semantic difference must be rejected");

    // assert
    assert!(matches!(error, ComparatorError::Cells { diffs, .. } if diffs.len() >= 5));
}

#[test]
fn rejects_one_pixel_difference_outside_identity_span() {
    // arrange
    let expected = png(&[[10, 20, 30, 255], [40, 50, 60, 255]]);
    let actual = png(&[[10, 20, 30, 255], [41, 50, 60, 255]]);
    let identity = [IdentityPixelSpan::new("title", PixelRect::new(0, 0, 1, 1))];

    // act
    let error = compare_png_bytes(&expected, &actual, &identity)
        .expect_err("an unmasked pixel must be rejected");

    // assert
    assert!(matches!(error, ComparatorError::Pixels { diffs, .. } if diffs.len() == 1));
}

#[test]
fn rejects_missing_motion_checkpoint() {
    // arrange
    let reference = motion_trace(TraceSource::Reference);
    let mut candidate = motion_trace(TraceSource::Harness);
    candidate
        .checkpoints
        .retain(|checkpoint| checkpoint != "mid");

    // act
    let error = compare_motion(&reference, &candidate).expect_err("mid checkpoint is required");

    // assert
    assert!(matches!(error, ComparatorError::Motion { .. }));
}

#[test]
fn rejects_timestamp_drift_over_one_frame() {
    // arrange
    let reference = TimingTrace::new(vec![0, 33, 66], vec![10, 10, 10]);
    let candidate = TimingTrace::new(vec![0, 50, 66], vec![10, 10, 10]);

    // act
    let error = compare_timing(&reference, &candidate)
        .expect_err("timestamp drift greater than one frame must be rejected");

    // assert
    assert!(matches!(error, ComparatorError::Timing { .. }));
}

#[test]
fn rejects_gross_p95_smoothness_defect() {
    // arrange
    let reference = TimingTrace::new(vec![0, 33, 66], vec![10, 10, 10]);
    let candidate = TimingTrace::new(vec![0, 33, 66], vec![20, 20, 20]);

    // act
    let error = compare_timing(&reference, &candidate)
        .expect_err("gross p95 timing regression must be rejected");

    // assert
    assert!(matches!(error, ComparatorError::Timing { .. }));
}

#[test]
fn accepts_declared_75ms_phase_cadence() {
    // arrange: both adapters preserve the three observed phases at the runner's 75ms cadence.
    let trace = TimingTrace::new(vec![0, 75, 150], vec![10, 10, 10]);

    // act: the timing comparator evaluates the declared cadence.
    let result = compare_timing(&trace, &trace);

    // assert: a cadence matching the execution contract is accepted.
    assert!(
        result.is_ok(),
        "75ms phase cadence must not hit a fixed 67ms bound"
    );
}

#[test]
fn rejects_p95_above_exact_relative_limit() {
    // arrange: the candidate exceeds the common-boundary p95 by more than 10%.
    let reference = TimingTrace::new(vec![0, 75, 150], vec![100, 100, 100]);
    let candidate = TimingTrace::new(vec![0, 75, 150], vec![111, 111, 111]);

    // act: the timing comparator evaluates p95 latency.
    let result = compare_timing(&reference, &candidate);

    // assert: no absolute jitter bypass weakens the 110% contract.
    assert!(result.is_err());
}

#[test]
fn packet2_timing_accepts_110_and_32ms_but_rejects_111_and_33ms() {
    // arrange
    let reference = presentation_metrics(100_000, 16_000, 32_000);
    let exact = presentation_metrics(110_000, 16_000, 32_000);
    let p95_over = presentation_metrics(111_000, 16_000, 32_000);
    let gap_over = presentation_metrics(110_000, 16_000, 33_000);

    // act
    let results = [
        compare_presentation_timing(&reference, &exact),
        compare_presentation_timing(&reference, &p95_over),
        compare_presentation_timing(&reference, &gap_over),
    ];
    // assert
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
    assert!(results[2].is_err());
}

fn presentation_metrics(latency: u64, cadence: u64, gap: u64) -> PresentationTimingMetrics {
    PresentationTimingMetrics {
        external_send_to_changed_observation_micros: vec![latency],
        external_observation_timestamps_micros: vec![1, gap + 1],
        external_observation_intervals_micros: vec![gap],
        external_cadence_micros: cadence,
        native: Some(NativeTimingMetrics {
            receive_to_successful_flush_micros: vec![1],
            request_to_successful_flush_micros: vec![1],
            completed_write_timestamps_micros: vec![1, 2],
            completed_write_intervals_micros: vec![1],
            coalesced_requests: 0,
            queue_saturation: 0,
            resyncs: 0,
            full_repaints: 1,
            bytes_written: 1,
            idle_redraws: 0,
        }),
    }
}

#[test]
fn rejects_reversed_timing_phase_order() {
    // arrange
    let reference = TimingTrace::with_phase_order(
        vec![0, 75, 150],
        vec![10, 10, 10],
        vec![TimingPhase::Rest, TimingPhase::Mid, TimingPhase::Settled],
    );
    let candidate = TimingTrace::with_phase_order(
        vec![0, 75, 150],
        vec![10, 10, 10],
        vec![TimingPhase::Rest, TimingPhase::Settled, TimingPhase::Mid],
    );

    // act
    let error = compare_timing(&reference, &candidate).expect_err("reversed phase must fail");

    // assert
    assert!(
        matches!(error, ComparatorError::Timing { defects, .. } if defects.iter().any(|defect| defect.reason == "phase_order"))
    );
}

#[test]
fn rejects_missing_timing_phase() {
    // arrange
    let reference = TimingTrace::with_phase_order(
        vec![0, 75, 150],
        vec![10, 10, 10],
        vec![TimingPhase::Rest, TimingPhase::Mid, TimingPhase::Settled],
    );
    let candidate = TimingTrace::with_phase_order(
        vec![0, 75],
        vec![10, 10],
        vec![TimingPhase::Rest, TimingPhase::Mid],
    );

    // act
    let error = compare_timing(&reference, &candidate).expect_err("missing phase must fail");

    // assert
    assert!(
        matches!(error, ComparatorError::Timing { defects, .. } if defects.iter().any(|defect| defect.reason == "phase_order" || defect.reason == "phase_count"))
    );
}

#[test]
fn rejects_excessive_adapter_phase_window() {
    // arrange
    let reference = TimingTrace::with_phase_order(
        vec![0, 75, 150],
        vec![10, 10, 10],
        vec![TimingPhase::Rest, TimingPhase::Mid, TimingPhase::Settled],
    );
    let candidate = TimingTrace::with_phase_order(
        vec![0, 75, 600],
        vec![10, 10, 10],
        vec![TimingPhase::Rest, TimingPhase::Mid, TimingPhase::Settled],
    );

    // act
    let error = compare_timing(&reference, &candidate).expect_err("excessive phase must fail");

    // assert
    assert!(
        matches!(error, ComparatorError::Timing { defects, .. } if defects.iter().any(|defect| defect.reason == "phase_window"))
    );
}

#[test]
fn rejects_stale_hash_after_source_edit() {
    // arrange
    let baseline = hash_artifacts(&artifact_inputs(b"source-v1")).expect("baseline hashes");
    let current = hash_artifacts(&artifact_inputs(b"source-v2")).expect("current hashes");

    // act
    let error = verify_freshness(&baseline, &current).expect_err("edited source is stale");

    // assert
    assert!(matches!(error, ComparatorError::Hashing { .. }));
}

#[test]
fn rejects_same_binary_on_both_sides() {
    // arrange
    let digest = "a".repeat(64);
    // act
    let error = reject_self_comparison(&digest, &digest)
        .expect_err("identical binary digests must be rejected");

    // assert
    assert!(matches!(error, ComparatorError::SelfComparison { .. }));
}

#[test]
fn rejects_secret_tokens_in_candidate_or_reference_artifacts() {
    // arrange
    let temp = tempfile::tempdir().expect("temporary artifact root");
    let reference = temp.path().join("reference");
    let candidate = temp.path().join("candidate");
    std::fs::create_dir_all(&reference).expect("reference root");
    std::fs::create_dir_all(&candidate).expect("candidate root");
    std::fs::write(
        candidate.join("metadata.json"),
        "{\"token\":\"sk-ant-abcdefghijklmnop\"}",
    )
    .expect("candidate artifact");
    let roots = ArtifactRoots::new(reference, candidate);

    // act
    let error = scan_artifacts(&roots).expect_err("secret-bearing artifacts must be rejected");

    // assert
    assert!(matches!(error, ComparatorError::Secrets { .. }));
}

#[test]
fn passes_exact_independent_fixtures_for_every_dimension() {
    // arrange
    let expected = cell_snapshot();
    let actual = cell_snapshot();
    // act
    let cells = compare_cells(&expected, &actual, &Default::default());
    // assert
    assert!(cells.is_ok());

    let bytes = png(&[[10, 20, 30, 255], [40, 50, 60, 255]]);
    assert!(compare_png_bytes(&bytes, &bytes, &[]).is_ok());

    let reference = motion_trace(TraceSource::Reference);
    let candidate = motion_trace(TraceSource::Harness);
    assert!(compare_motion(&reference, &candidate).is_ok());

    let timing = TimingTrace::new(vec![0, 33, 66], vec![10, 10, 10]);
    assert!(compare_timing(&timing, &timing).is_ok());

    let hashes = hash_artifacts(&artifact_inputs(b"source-v1")).expect("fixture hashes");
    assert!(verify_freshness(&hashes, &hashes).is_ok());
    assert!(reject_self_comparison(&"a".repeat(64), &"b".repeat(64)).is_ok());

    let temp = tempfile::tempdir().expect("temporary artifact root");
    let reference_root = temp.path().join("reference");
    let candidate_root = temp.path().join("candidate");
    std::fs::create_dir_all(&reference_root).expect("reference root");
    std::fs::create_dir_all(&candidate_root).expect("candidate root");
    assert!(scan_artifacts(&ArtifactRoots::new(reference_root, candidate_root)).is_ok());
}

fn cell_snapshot() -> CellSnapshot {
    let mut frame = SemanticFrame::new(2, 1, CursorState::hidden(0, 0));
    frame
        .set_cell(
            SemanticCell::blank(0, 0)
                .with_grapheme("a", 1)
                .with_fg(ResolvedRgb::new(220, 220, 220)),
        )
        .expect("cell in bounds");
    CellSnapshot {
        frame,
        focus: Some(FocusState::new(0, 0)),
        z_order: vec![ZOrderEntry::new("content", 0)],
    }
}

fn motion_trace(source: TraceSource) -> MotionTrace {
    let frame_trace = synthetic_passing_trace(source).expect("synthetic motion trace");
    let timestamps = (0..frame_trace.frames.len())
        .map(|index| u64::try_from(index).expect("small fixture index") * 33)
        .collect();
    MotionTrace::from_frame_trace(frame_trace, timestamps)
}

fn artifact_inputs(source: &[u8]) -> ArtifactInputs {
    ArtifactInputs::new(b"renderer", b"font", source, b"scenario", b"config")
}

fn png(pixels: &[[u8; 4]]) -> Vec<u8> {
    let width = u32::try_from(pixels.len()).expect("small PNG fixture");
    let mut image = image::RgbaImage::new(width, 1);
    for (x, pixel) in pixels.iter().enumerate() {
        let x = u32::try_from(x).expect("small PNG fixture coordinate");
        image.put_pixel(x, 0, image::Rgba(*pixel));
    }
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("encode PNG fixture");
    bytes.into_inner()
}
