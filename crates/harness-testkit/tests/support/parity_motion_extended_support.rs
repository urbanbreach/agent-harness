use super::*;

// ---------------------------------------------------------------------------
// Tests: streaming deltas family.
// ---------------------------------------------------------------------------

#[test]
fn streaming_deltas_baseline_passes() {
    // arrange
    let trace = baseline_passing_trace(TraceSource::Reference);

    // act
    let defects = validate_motion_trace_with_families(&trace, &[MotionFamily::StreamingDeltas]);

    // assert
    assert!(
        defects.is_ok(),
        "streaming deltas family must accept baseline trace: {:?}",
        defects.err()
    );
}

#[test]
fn streaming_deltas_rejects_missing_phase() {
    // arrange
    let mut trace = baseline_passing_trace(TraceSource::Reference);
    trace
        .frames
        .retain(|tick_frame| tick_frame.phase != MotionPhase::StreamingDelta);

    // act
    let defects = validate_motion_trace_with_families(&trace, &[MotionFamily::StreamingDeltas])
        .expect_err("missing streaming deltas must be rejected");

    // assert
    assert_defect(
        &defects,
        MotionFamily::StreamingDeltas,
        "missing_streaming_delta_phase",
    );
}

#[test]
fn streaming_deltas_rejects_tick_collision() {
    // arrange: collapse two streaming deltas onto the same tick.
    let mut trace = baseline_passing_trace(TraceSource::Reference);
    let first_delta = trace
        .frames
        .iter()
        .position(|tick_frame| tick_frame.phase == MotionPhase::StreamingDelta)
        .expect("first delta");
    trace.frames[first_delta + 1].tick = trace.frames[first_delta].tick;

    // act
    let defects = validate_motion_trace_with_families(&trace, &[MotionFamily::StreamingDeltas])
        .expect_err("streaming delta tick collisions must be rejected");

    // assert
    assert_defect(
        &defects,
        MotionFamily::StreamingDeltas,
        "streaming_delta_tick_collision",
    );
}

// ---------------------------------------------------------------------------
// Tests: cancellation ordering family.
// ---------------------------------------------------------------------------

#[test]
fn cancellation_ordering_baseline_passes() {
    // arrange
    let trace = baseline_cancellation_trace(TraceSource::Reference);

    // act
    let defects =
        validate_motion_trace_with_families(&trace, &[MotionFamily::CancellationOrdering]);

    // assert
    assert!(
        defects.is_ok(),
        "cancellation baseline must satisfy the family: {:?}",
        defects.err()
    );
}

#[test]
fn cancellation_ordering_rejects_missing_cancellation() {
    // arrange: drop the cancellation frame.
    let mut trace = baseline_cancellation_trace(TraceSource::Reference);
    trace
        .frames
        .retain(|tick_frame| tick_frame.phase != MotionPhase::Cancellation);

    // act
    let defects =
        validate_motion_trace_with_families(&trace, &[MotionFamily::CancellationOrdering])
            .expect_err("missing cancellation must be rejected");

    // assert
    assert_defect(
        &defects,
        MotionFamily::CancellationOrdering,
        "missing_cancellation_phase",
    );
}

#[test]
fn cancellation_ordering_rejects_missing_recovered() {
    // arrange: drop the cancel_recovered frame.
    let mut trace = baseline_cancellation_trace(TraceSource::Reference);
    trace
        .frames
        .retain(|tick_frame| tick_frame.phase != MotionPhase::CancelRecovered);

    // act
    let defects =
        validate_motion_trace_with_families(&trace, &[MotionFamily::CancellationOrdering])
            .expect_err("missing cancel_recovered must be rejected");

    // assert
    assert_defect(
        &defects,
        MotionFamily::CancellationOrdering,
        "missing_cancel_recovered_phase",
    );
}

#[test]
fn cancellation_ordering_rejects_recovered_before_cancellation() {
    // arrange: swap the order of cancellation and cancel_recovered frames.
    let mut trace = baseline_cancellation_trace(TraceSource::Reference);
    let cancel_idx = trace
        .frames
        .iter()
        .position(|tick_frame| tick_frame.phase == MotionPhase::Cancellation)
        .expect("cancellation");
    let recovered_idx = trace
        .frames
        .iter()
        .position(|tick_frame| tick_frame.phase == MotionPhase::CancelRecovered)
        .expect("cancel_recovered");
    let recovered_frame = trace.frames.remove(recovered_idx);
    trace.frames.insert(cancel_idx, recovered_frame);

    // act
    let defects =
        validate_motion_trace_with_families(&trace, &[MotionFamily::CancellationOrdering])
            .expect_err("recovered-before-cancel must be rejected");

    // assert
    assert_defect(
        &defects,
        MotionFamily::CancellationOrdering,
        "cancel_recovered_before_cancellation",
    );
}

#[test]
fn cancellation_ordering_rejects_stale_spinner() {
    // arrange: inject a spinner glyph frame after cancel_recovered.
    let mut trace = baseline_cancellation_trace(TraceSource::Reference);
    let recovered_idx = trace
        .frames
        .iter()
        .position(|tick_frame| tick_frame.phase == MotionPhase::CancelRecovered)
        .expect("cancel_recovered");
    trace.frames.insert(
        recovered_idx + 1,
        TickFrame {
            tick: trace.frames[recovered_idx].tick + 1,
            phase: MotionPhase::FinishFlash,
            frame: spinner_frame("⠋"),
        },
    );

    // act
    let defects =
        validate_motion_trace_with_families(&trace, &[MotionFamily::CancellationOrdering])
            .expect_err("stale spinner must be rejected");

    // assert
    assert_defect(
        &defects,
        MotionFamily::CancellationOrdering,
        "stale_spinner_after_cancel",
    );
}

// ---------------------------------------------------------------------------
// Tests: cross-source comparison.
// ---------------------------------------------------------------------------

#[test]
fn compare_motion_traces_rejects_self_oracle() {
    // arrange
    let reference = baseline_passing_trace(TraceSource::Reference);

    // act
    let defects = compare_motion_traces(
        &reference,
        &reference,
        &IdentityMaskRegistry::new(),
        MotionFamily::all(),
    )
    .expect_err("self-oracle comparison must be rejected");

    // assert
    assert_defect(&defects, MotionFamily::OrderedMotion, "self_oracle");
}

#[test]
fn compare_motion_traces_accepts_identical_cross_source_baselines() {
    // arrange: reference and candidate share the same content but are
    // labelled with different sources.
    let reference = baseline_passing_trace(TraceSource::Reference);
    let candidate = baseline_passing_trace(TraceSource::Harness);

    // act
    let result = compare_motion_traces(
        &reference,
        &candidate,
        &IdentityMaskRegistry::new(),
        MotionFamily::all(),
    );

    // assert
    assert!(
        result.is_ok(),
        "identical cross-source baselines must compare equal: {:?}",
        result.err()
    );
}

#[test]
fn compare_motion_traces_rejects_no_shared_ticks() {
    // arrange: candidate uses a completely disjoint tick set.
    let reference = baseline_passing_trace(TraceSource::Reference);
    let mut candidate = baseline_passing_trace(TraceSource::Harness);
    for tick_frame in candidate.frames.iter_mut() {
        tick_frame.tick += 1_000;
    }

    // act
    let defects = compare_motion_traces(
        &reference,
        &candidate,
        &IdentityMaskRegistry::new(),
        MotionFamily::all(),
    )
    .expect_err("disjoint ticks must be rejected");

    // assert
    assert_defect(&defects, MotionFamily::OrderedMotion, "no_shared_ticks");
}

#[test]
fn compare_motion_traces_rejects_frame_mismatch_at_shared_tick() {
    // arrange: mutate one cell in the candidate's streaming delta frame.
    let reference = baseline_passing_trace(TraceSource::Reference);
    let mut candidate = baseline_passing_trace(TraceSource::Harness);
    let candidate_delta = candidate
        .frames
        .iter_mut()
        .find(|tick_frame| tick_frame.phase == MotionPhase::StreamingDelta)
        .expect("candidate streaming delta");
    candidate_delta
        .frame
        .set_cell(SemanticCell::blank(0, 0).with_grapheme("X", 1))
        .expect("set");

    // act
    let defects = compare_motion_traces(
        &reference,
        &candidate,
        &IdentityMaskRegistry::new(),
        MotionFamily::all(),
    )
    .expect_err("frame mismatch at shared tick must be rejected");

    // assert
    assert_defect(
        &defects,
        MotionFamily::OrderedMotion,
        "frame_mismatch_at_shared_tick",
    );
}

// ---------------------------------------------------------------------------
// Tests: full family matrix runs over the synthetic baselines and writes the
// evidence receipt.
// ---------------------------------------------------------------------------

#[test]
fn full_family_matrix_accepts_reference_and_candidate_baselines() {
    // arrange: bound every proof to the exact on-disk binary identities.
    let identities = capture_binary_identities();
    let reference = baseline_passing_trace(TraceSource::Reference);
    let candidate = baseline_passing_trace(TraceSource::Harness);

    // act
    let reference_defects = validate_motion_trace(&reference);
    let candidate_defects = validate_motion_trace(&candidate);
    let cross = compare_motion_traces(
        &reference,
        &candidate,
        &IdentityMaskRegistry::new(),
        MotionFamily::all(),
    );

    // assert
    assert!(
        reference_defects.is_ok(),
        "reference baseline: {:?}",
        reference_defects.err()
    );
    assert!(
        candidate_defects.is_ok(),
        "candidate baseline: {:?}",
        candidate_defects.err()
    );
    assert!(cross.is_ok(), "cross-source: {:?}", cross.err());

    // Build the evidence receipt with one passing row per family.
    let rows = MotionFamily::all()
        .iter()
        .map(|family| EvidenceRow {
            family: family.as_str().to_owned(),
            scenario: "matched_contract_trace".to_owned(),
            verdict: "pass".to_owned(),
            detail: format!(
                "family {} validated through motion contract with matched reference/candidate traces",
                family
            ),
        })
        .collect::<Vec<_>>();
    let receipt_path = write_evidence_receipt(&identities, MotionFamily::all(), &rows);
    let receipt_bytes = fs::read(&receipt_path).expect("receipt readable");
    let receipt: serde_json::Value =
        serde_json::from_slice(&receipt_bytes).expect("receipt parses");
    assert_eq!(receipt["schema"], "grok-parity-motion-timing-receipt-v1");
    assert_eq!(receipt["proof_dimension"], "P5");
    assert_eq!(
        receipt["reference_identity"]["binary_sha256"],
        ACTIVE_REFERENCE_SHA256
    );
    assert_eq!(
        receipt["candidate_identity"]["binary_sha256"],
        expected_candidate_sha256()
    );
    assert_eq!(receipt["summary"]["passed"], MotionFamily::all().len());
    assert_eq!(receipt["summary"]["rejected"], 0);
    assert_eq!(receipt["acceptance_eligible"], false);
}

#[test]
fn failure_mutation_per_family_is_rejected() {
    // arrange: bound to binary identities.
    let identities = capture_binary_identities();
    let mutations: BTreeMap<&str, (MotionFamily, FrameTrace)> = [
        (
            "missing_phase",
            (MotionFamily::OrderedMotion, {
                let mut trace = baseline_passing_trace(TraceSource::Harness);
                trace
                    .frames
                    .retain(|tick_frame| tick_frame.phase != MotionPhase::FinishFlash);
                trace
            }),
        ),
        (
            "insufficient_settle_dwell",
            (MotionFamily::SettleDwell, {
                let mut trace = baseline_passing_trace(TraceSource::Harness);
                let last = trace
                    .frames
                    .iter()
                    .rposition(|tick_frame| tick_frame.phase == MotionPhase::SettleRepeat)
                    .expect("last settle");
                trace.frames.remove(last);
                trace
            }),
        ),
        (
            "missing_scroll_flush_phase",
            (MotionFamily::ScrollFlush, {
                let mut trace = baseline_passing_trace(TraceSource::Harness);
                trace
                    .frames
                    .retain(|tick_frame| tick_frame.phase != MotionPhase::ScrollFlush);
                trace
            }),
        ),
        (
            "missing_resize_settled_phase",
            (MotionFamily::ResizeDebounce, {
                let mut trace = baseline_passing_trace(TraceSource::Harness);
                trace
                    .frames
                    .retain(|tick_frame| tick_frame.phase != MotionPhase::ResizeSettled);
                trace
            }),
        ),
        (
            "streaming_delta_tick_collision",
            (MotionFamily::StreamingDeltas, {
                let mut trace = baseline_passing_trace(TraceSource::Harness);
                let first_delta = trace
                    .frames
                    .iter()
                    .position(|tick_frame| tick_frame.phase == MotionPhase::StreamingDelta)
                    .expect("first delta");
                trace.frames[first_delta + 1].tick = trace.frames[first_delta].tick;
                trace
            }),
        ),
        (
            "stale_spinner_after_cancel",
            (MotionFamily::CancellationOrdering, {
                let mut trace = baseline_cancellation_trace(TraceSource::Harness);
                let recovered_idx = trace
                    .frames
                    .iter()
                    .position(|tick_frame| tick_frame.phase == MotionPhase::CancelRecovered)
                    .expect("cancel_recovered");
                trace.frames.insert(
                    recovered_idx + 1,
                    TickFrame {
                        tick: trace.frames[recovered_idx].tick + 1,
                        phase: MotionPhase::FinishFlash,
                        frame: spinner_frame("⠙"),
                    },
                );
                trace
            }),
        ),
    ]
    .into_iter()
    .collect();

    // act + assert
    let mut rows = Vec::new();
    for (reason, (family, trace)) in &mutations {
        let defects = validate_motion_trace_with_families(trace, &[*family])
            .err()
            .unwrap_or_default();
        assert_defect(&defects, *family, reason);
        rows.push(EvidenceRow {
            family: family.as_str().to_owned(),
            scenario: format!("failure_mutation_{reason}"),
            verdict: "rejected".to_owned(),
            detail: format!(
                "mutation {reason} for family {family} was rejected with a typed MotionDefect"
            ),
        });
    }
    let families: Vec<MotionFamily> = mutations.values().map(|(family, _)| *family).collect();
    let receipt_path = write_evidence_receipt(&identities, &families, &rows);
    let receipt_bytes = fs::read(&receipt_path).expect("receipt readable");
    let receipt: serde_json::Value =
        serde_json::from_slice(&receipt_bytes).expect("receipt parses");
    // assert
    assert_eq!(receipt["summary"]["rejected"], mutations.len());
    assert_eq!(receipt["summary"]["passed"], 0);
    assert_eq!(receipt["acceptance_eligible"], false);
}
