#![allow(clippy::panic, reason = "owner tests use fail-fast fixture assertions")]

use harness_testkit::parity::{CursorState, SemanticFrame};
use harness_testkit::tui_fidelity::AdapterKind;
use harness_testkit::tui_fidelity_compare::{
    compare_ordered_presentations, compare_presentation_timing, derive_presentation_timing,
    NativeTimingMetrics, PresentationTimingMetrics,
};
use harness_testkit::tui_fidelity_runner::*;

fn external() -> ExternalPresentationEvidence {
    ExternalPresentationEvidence {
        clock: PresentationClock {
            unit: ClockUnit::MonotonicMicroseconds,
            epoch_id: "run-1".into(),
        },
        actual_input_sends: vec![ActualInputSend {
            interaction_id: InteractionId("scenario:action:0".into()),
            action_ordinal: 0,
            scheduled_at: PresentationTimestamp(10),
            sent_at: PresentationTimestamp(12),
            transport_drained_at: None,
        }],
        raw_reads: vec![RawPtyRead {
            read_completed_at: PresentationTimestamp(20),
            byte_len: 1,
            sha256: "a".repeat(64),
            decoder_state: DecoderState::Complete,
        }],
        observations: vec![TimedSemanticObservation {
            observation_ordinal: 0,
            observed_at: PresentationTimestamp(20),
            kind: ObservationKind::ReadCompletionDecode,
            decoder_state: DecoderState::Complete,
            raw_read_ordinals: vec![0],
            frame: SemanticFrame::new(1, 1, CursorState::hidden(0, 0)),
        }],
        interaction_observations: vec![InteractionObservation {
            interaction_id: InteractionId("scenario:action:0".into()),
            first_changed_observation: Some(0),
            diagnostic: None,
        }],
        raw_ansi: ArtifactDigest {
            path: "raw.ansi".into(),
            sha256: "b".repeat(64),
        },
        observations_artifact: ArtifactDigest {
            path: "observations.json".into(),
            sha256: "c".repeat(64),
        },
        metrics_kind: PresentationMetricsKind::ExternalPtyObserved,
        native_visual_observed_at: None,
    }
}

fn native() -> NativePresentationTrace {
    NativePresentationTrace {
        trace_id: "trace".into(),
        causes: vec![NativeCause {
            cause_id: "trace:cause:0".into(),
            interaction_id: Some(InteractionId("scenario:action:0".into())),
            received_at: PresentationTimestamp(13),
            kind: "terminal_input".into(),
            resulting_revision: Some(1),
            outcome: NativeCauseOutcome::VisibleChange,
        }],
        demands: vec![NativeDemand {
            target_revision: 1,
            earliest_requested_at: PresentationTimestamp(14),
            latest_requested_at: PresentationTimestamp(14),
            cause_ids: vec!["trace:cause:0".into()],
            reason: "input".into(),
            coalesced_request_count: 1,
        }],
        frames: vec![NativeFrame {
            sequence: 1,
            revision: 1,
            cause_ids: vec!["trace:cause:0".into()],
            requested_at: PresentationTimestamp(14),
            render_started_at: PresentationTimestamp(14),
            render_ended_at: PresentationTimestamp(15),
            submitted_at: PresentationTimestamp(15),
            frame_kind: "differential".into(),
            byte_count: 3,
            byte_sha256: "d".repeat(64),
        }],
        acknowledgements: vec![NativeFrameAck {
            sequence: 1,
            revision: 1,
            byte_sha256: "d".repeat(64),
            write_started_at: PresentationTimestamp(16),
            write_ended_at: PresentationTimestamp(17),
            acknowledged_at: PresentationTimestamp(18),
            outcome: NativeAckOutcome::CompletedWrite,
        }],
        outcomes: Vec::new(),
        aggregates: NativePresentationAggregates {
            coalesced_requests: 0,
            queue_saturation: 0,
            resyncs: 0,
            full_repaints: 0,
            bytes_written: 3,
            idle_redraws: 0,
        },
    }
}

#[test]
fn grok_external_and_harness_native_round_trip() {
    let values = [
        (
            AdapterKind::Grok,
            PresentationEvidence::ExternalOnly {
                external: external(),
            },
        ),
        (
            AdapterKind::Harness,
            PresentationEvidence::HarnessNative {
                external: external(),
                native: Box::new(native()),
                native_trace_artifact: ArtifactDigest {
                    path: "native.json".into(),
                    sha256: "e".repeat(64),
                },
                links: vec![NativeExternalLink {
                    frame_sequence: 1,
                    byte_sha256: "d".repeat(64),
                    stream_offset: 0,
                }],
            },
        ),
    ];
    for (adapter, value) in values {
        let json = serde_json::to_string(&value).expect("serialize receipt");
        let decoded: PresentationEvidence =
            serde_json::from_str(&json).expect("deserialize receipt");
        assert_eq!(decoded, value);
        validate_presentation_evidence(adapter, &decoded).expect("evidence validates");
    }
}

#[test]
fn harness_external_interaction_without_native_receipt_fails_closed() {
    // Given: an otherwise complete native trace whose scripted terminal receipt lost its identity.
    let mut trace = native();
    trace.causes[0].interaction_id = None;
    let evidence = PresentationEvidence::HarnessNative {
        external: external(),
        native: Box::new(trace),
        native_trace_artifact: ArtifactDigest {
            path: "native.json".into(),
            sha256: "e".repeat(64),
        },
        links: vec![NativeExternalLink {
            frame_sequence: 1,
            byte_sha256: "d".repeat(64),
            stream_offset: 0,
        }],
    };

    // When: production receipt validation checks the Harness evidence.
    let error = validate_presentation_evidence(AdapterKind::Harness, &evidence)
        .expect_err("unlinked terminal input must fail closed");

    // Then: the unmatched external interaction is rejected without rejecting unscripted causes.
    assert!(matches!(
        error,
        PresentationValidationError::ExternalInteractionUnlinked { .. }
    ));
}

#[test]
fn harness_external_only_is_rejected() {
    let error = validate_presentation_evidence(
        AdapterKind::Harness,
        &PresentationEvidence::ExternalOnly {
            external: external(),
        },
    )
    .expect_err("Harness cannot use external-only evidence");
    assert_eq!(
        error,
        PresentationValidationError::HarnessNativeEvidenceRequired
    );
}

#[test]
fn duplicate_ack_and_unknown_fields_fail_closed() {
    let mut trace = native();
    trace
        .acknowledgements
        .push(trace.acknowledgements[0].clone());
    let error = validate_presentation_evidence(
        AdapterKind::Harness,
        &PresentationEvidence::HarnessNative {
            external: external(),
            native: Box::new(trace),
            native_trace_artifact: ArtifactDigest {
                path: "native.json".into(),
                sha256: "e".repeat(64),
            },
            links: vec![NativeExternalLink {
                frame_sequence: 1,
                byte_sha256: "d".repeat(64),
                stream_offset: 0,
            }],
        },
    )
    .expect_err("duplicate ack rejected");
    assert_eq!(
        error,
        PresentationValidationError::AckCardinality { sequence: 1 }
    );

    let json = serde_json::json!({"kind":"external_only","external":external(),"unexpected":true});
    assert!(serde_json::from_value::<PresentationEvidence>(json).is_err());
}

#[test]
fn duplicate_frame_sequence_and_orphan_ack_fail_closed() {
    // Given: native traces with either a duplicate accepted-frame sequence or an orphan ack.
    let mut duplicate_frame = native();
    duplicate_frame
        .frames
        .push(duplicate_frame.frames[0].clone());
    duplicate_frame
        .acknowledgements
        .push(duplicate_frame.acknowledgements[0].clone());
    let mut orphan_ack = native();
    let mut extra_ack = orphan_ack.acknowledgements[0].clone();
    extra_ack.sequence = 9;
    orphan_ack.acknowledgements.push(extra_ack);

    // When: exact accepted-frame acknowledgement cardinality is validated.
    let duplicate_error = validate_native_fixture(duplicate_frame, vec![native_link(1, 0)]);
    let orphan_error = validate_native_fixture(orphan_ack, vec![native_link(1, 0)]);

    // Then: frame ordering and orphan acknowledgements both fail closed.
    assert_eq!(
        duplicate_error,
        PresentationValidationError::FrameSequenceOrder
    );
    assert_eq!(
        orphan_error,
        PresentationValidationError::AckCardinality { sequence: 9 }
    );
}

#[test]
fn permuted_native_external_links_fail_closed() {
    // Given: two ordered native frames and byte links presented in reverse order.
    let mut trace = native();
    let mut frame = trace.frames[0].clone();
    frame.sequence = 2;
    frame.byte_sha256 = "f".repeat(64);
    trace.frames.push(frame);
    let mut ack = trace.acknowledgements[0].clone();
    ack.sequence = 2;
    ack.byte_sha256 = "f".repeat(64);
    trace.acknowledgements.push(ack);

    // When: native-to-external linkage is required to be bijective and ordered.
    let error = validate_native_fixture(trace, vec![native_link(2, 4), native_link(1, 0)]);

    // Then: a digest-bearing link for the wrong ordered frame is unresolved.
    assert!(matches!(
        error,
        PresentationValidationError::UnresolvedReference { .. }
    ));
}

fn validate_native_fixture(
    trace: NativePresentationTrace,
    links: Vec<NativeExternalLink>,
) -> PresentationValidationError {
    match validate_presentation_evidence(
        AdapterKind::Harness,
        &PresentationEvidence::HarnessNative {
            external: external(),
            native: Box::new(trace),
            native_trace_artifact: ArtifactDigest {
                path: "native.json".into(),
                sha256: "e".repeat(64),
            },
            links,
        },
    ) {
        Ok(()) => panic!("invalid native evidence must fail closed"),
        Err(error) => error,
    }
}

fn native_link(sequence: u64, stream_offset: usize) -> NativeExternalLink {
    NativeExternalLink {
        frame_sequence: sequence,
        byte_sha256: if sequence == 1 { "d" } else { "f" }.repeat(64),
        stream_offset,
    }
}

#[test]
fn pending_resync_and_unknown_native_outcomes_fail_closed() {
    // Given: unresolved, misplaced resync, and unknown outcomes at the native sidecar boundary.
    let outcomes = [
        serde_json::json!({"kind":"pending"}),
        serde_json::json!({
            "kind":"resync_required",
            "rejected_revision":1,
            "replacement_revision":2,
            "recorded_at":9
        }),
        serde_json::json!({"kind":"future_outcome"}),
    ];

    // When: each sidecar is parsed rather than normalized into a no-op.
    for outcome in outcomes {
        let temp = tempfile::tempdir().expect("sidecar tempdir");
        let path = temp.path().join("native.json");
        std::fs::write(&path, runtime_sidecar(outcome).to_string()).expect("write sidecar");

        // Then: parsing rejects every unresolved or unsupported outcome.
        assert!(read_native_trace(&path).is_err());
    }
}

#[test]
fn disagreeing_runtime_frame_and_ack_fail_closed() {
    // Given: a raw sidecar whose duplicated frame record says success but ack says failure.
    let temp = tempfile::tempdir().expect("sidecar tempdir");
    let path = temp.path().join("native.json");
    let mut sidecar = complete_runtime_sidecar();
    sidecar["acknowledgements"][0]["outcome"] =
        serde_json::json!({"kind":"failure","stage":"flush"});
    std::fs::write(&path, sidecar.to_string()).expect("write disagreeing sidecar");

    // When: duplicated integrity-bearing records cross the parser boundary.
    let error = read_native_trace(&path).expect_err("disagreement must fail closed");

    // Then: the raw mismatch is rejected before a completed-write receipt is created.
    assert!(error.to_string().contains("frame/ack integrity differs"));
}

#[test]
fn every_duplicated_runtime_integrity_field_disagreement_fails_closed() {
    // Given: one isolated mismatch for every field duplicated by the raw frame and ack records.
    let disagreements = [
        ("sequence", serde_json::json!(2)),
        ("revision", serde_json::json!(2)),
        ("cause_ids", serde_json::json!(["trace:cause:other"])),
        ("requested_at", serde_json::json!(20)),
        ("render_started_at", serde_json::json!(30)),
        ("render_ended_at", serde_json::json!(40)),
        ("submitted_at", serde_json::json!(50)),
        ("write_started_at", serde_json::json!(60)),
        ("write_ended_at", serde_json::json!(70)),
        ("acknowledged_at", serde_json::json!(80)),
        ("frame_kind", serde_json::json!("differential")),
        ("byte_count", serde_json::json!(4)),
        ("byte_sha256", serde_json::json!("f".repeat(64))),
        (
            "outcome",
            serde_json::json!({"kind":"failure","stage":"flush"}),
        ),
    ];

    // When: each disagreement crosses the native sidecar boundary independently.
    for (field, value) in disagreements {
        let temp = tempfile::tempdir().expect("sidecar tempdir");
        let path = temp.path().join("native.json");
        let mut sidecar = complete_runtime_sidecar();
        sidecar["acknowledgements"][0][field] = value;
        std::fs::write(&path, sidecar.to_string()).expect("write disagreeing sidecar");

        // Then: every mismatch is rejected before native receipt normalization.
        let error = read_native_trace(&path).expect_err("integrity mismatch must fail closed");
        assert!(
            error.to_string().contains("frame/ack integrity differs"),
            "field {field}: {error}"
        );
    }
}

#[test]
fn trace_resync_outcome_is_preserved_and_requires_linked_replacement() {
    // Given: otherwise identical sidecars with linked and unlinked resync replacements.
    let linked_temp = tempfile::tempdir().expect("linked sidecar tempdir");
    let linked_path = linked_temp.path().join("native.json");
    let mut linked = complete_runtime_sidecar();
    linked["outcomes"] = serde_json::json!([{
        "kind":"resync_required","rejected_revision":0,"replacement_revision":1,
        "recorded_at":9
    }]);
    std::fs::write(&linked_path, linked.to_string()).expect("write linked resync");
    let unlinked_temp = tempfile::tempdir().expect("unlinked sidecar tempdir");
    let unlinked_path = unlinked_temp.path().join("native.json");
    let mut unlinked = complete_runtime_sidecar();
    unlinked["outcomes"] = serde_json::json!([{
        "kind":"resync_required","rejected_revision":1,"replacement_revision":2,
        "recorded_at":9
    }]);
    std::fs::write(&unlinked_path, unlinked.to_string()).expect("write unlinked resync");

    // When: typed trace outcomes cross the native-sidecar boundary.
    let trace = read_native_trace(&linked_path).expect("linked resync accepted");
    let unlinked_error = read_native_trace(&unlinked_path).expect_err("unlinked resync rejected");

    // Then: integrity fields survive projection and a missing replacement link fails closed.
    assert_eq!(trace.outcomes[0].rejected_revision, Some(0));
    assert_eq!(trace.outcomes[0].replacement_revision, Some(1));
    assert_eq!(
        trace.outcomes[0].recorded_at,
        Some(PresentationTimestamp(9))
    );
    assert!(unlinked_error
        .to_string()
        .contains("not linked to replacement"));
}

fn runtime_sidecar(outcome: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "trace_id":"trace",
        "causes":[{
            "cause_id":"trace:cause:1","interaction_id":"scenario:action:0",
            "received_at":1,"kind":"terminal_input","resulting_revision":1,"outcome":outcome
        }],
        "demands":[{"target_revision":1,"earliest_requested_at":2,"latest_requested_at":2,
            "cause_ids":["trace:cause:1"],"reason":"terminal_input","coalesced_request_count":0}],
        "frames":[],"acknowledgements":[],"outcomes":[],
        "aggregates":{"coalesced_requests":0,"queue_saturation":0,"resyncs":0,
            "full_repaints":0,"bytes_written":0,"idle_redraws":0}
    })
}

fn complete_runtime_sidecar() -> serde_json::Value {
    let timing = serde_json::json!({
        "sequence":1,"revision":1,"cause_ids":["trace:cause:1"],"requested_at":2,
        "render_started_at":3,"render_ended_at":4,"submitted_at":5,"write_started_at":6,
        "write_ended_at":7,"acknowledged_at":8,"frame_kind":"full_repaint","byte_count":3,
        "byte_sha256":"d".repeat(64)
    });
    let mut frame = timing.clone();
    frame["acknowledgement"] = serde_json::json!({"kind":"success"});
    let mut ack = timing;
    ack["outcome"] = serde_json::json!({"kind":"success"});
    serde_json::json!({
        "trace_id":"trace",
        "causes":[{"cause_id":"trace:cause:1","interaction_id":"scenario:action:0",
            "received_at":1,"kind":"terminal_input","resulting_revision":1,
            "outcome":{"kind":"visible_change","cause_id":"trace:cause:1","revision":1}}],
        "demands":[{"target_revision":1,"earliest_requested_at":2,"latest_requested_at":2,
            "cause_ids":["trace:cause:1"],"reason":"terminal_input","coalesced_request_count":0}],
        "frames":[frame],"acknowledgements":[ack],"outcomes":[],
        "aggregates":{"coalesced_requests":0,"queue_saturation":0,"resyncs":0,
            "full_repaints":1,"bytes_written":3,"idle_redraws":0}
    })
}

#[test]
fn presentation_timing_schedule_offsets_are_provenance_only() {
    // Given: identical observed timestamps with different scripted schedule provenance.
    let baseline = PresentationEvidence::ExternalOnly {
        external: external(),
    };
    let mut shifted = baseline.clone();
    let PresentationEvidence::ExternalOnly { external } = &mut shifted else {
        panic!("external fixture");
    };
    external.actual_input_sends[0].scheduled_at = PresentationTimestamp(10_000);

    // When: application latency is derived from each receipt.
    let baseline_metrics = derive_presentation_timing(&baseline).expect("baseline metrics");
    let shifted_metrics = derive_presentation_timing(&shifted).expect("shifted metrics");

    // Then: schedule-only changes do not enter either latency distribution.
    assert_eq!(baseline_metrics, shifted_metrics);
}

#[test]
fn presentation_timing_rejects_delay_and_long_interval() {
    // Given: a reference metric and candidates at the exact p95 and cadence boundaries.
    let reference = PresentationTimingMetrics {
        external_send_to_changed_observation_micros: vec![100],
        external_observation_timestamps_micros: vec![0, 100, 200],
        external_observation_intervals_micros: vec![100, 100],
        external_cadence_micros: 100,
        native: None,
    };
    let native = NativeTimingMetrics {
        receive_to_successful_flush_micros: vec![10],
        request_to_successful_flush_micros: vec![8],
        completed_write_timestamps_micros: vec![0, 100, 200],
        completed_write_intervals_micros: vec![100, 100],
        coalesced_requests: 0,
        queue_saturation: 0,
        resyncs: 0,
        full_repaints: 1,
        bytes_written: 3,
        idle_redraws: 0,
    };
    let mut boundary = reference.clone();
    boundary.external_send_to_changed_observation_micros = vec![110];
    boundary.native = Some(native.clone());
    let mut delayed = boundary.clone();
    delayed.external_send_to_changed_observation_micros = vec![111];
    let mut long_interval = boundary.clone();
    long_interval.native = Some(NativeTimingMetrics {
        completed_write_intervals_micros: vec![100, 201, 100],
        ..native
    });

    // When: the strict presentation timing gate evaluates each candidate.
    let boundary_result = compare_presentation_timing(&reference, &boundary);
    let delayed_result = compare_presentation_timing(&reference, &delayed);
    let interval_result = compare_presentation_timing(&reference, &long_interval);

    // Then: 110% passes while 111% and a greater-than-twice-cadence gap fail.
    assert!(boundary_result.is_ok());
    assert!(delayed_result.is_err());
    assert!(interval_result.is_err());
}

#[test]
fn production_motion_rejects_swapped_cancellation() {
    // Given: complete typed external receipts with cancellation and recovery frames swapped only on Harness.
    let scenario = harness_testkit::tui_fidelity::Scenario::from_json(include_str!(
        "../src/tui_fidelity_scenarios/baseline/cancel.json"
    ))
    .expect("cancel scenario");
    let mut reference = external();
    reference.actual_input_sends.push(ActualInputSend {
        interaction_id: InteractionId(format!("{}:action:1", scenario.id.0)),
        action_ordinal: 1,
        scheduled_at: PresentationTimestamp(14),
        sent_at: PresentationTimestamp(15),
        transport_drained_at: None,
    });
    reference.actual_input_sends[0].interaction_id =
        InteractionId(format!("{}:action:0", scenario.id.0));
    reference.actual_input_sends[0].sent_at = PresentationTimestamp(10);
    reference.observations = motion_observations();
    let mut candidate = reference.clone();
    let cancellation = candidate.observations[1].frame.clone();
    candidate.observations[1].frame = candidate.observations[2].frame.clone();
    candidate.observations[2].frame = cancellation;
    let reference = PresentationEvidence::ExternalOnly {
        external: reference,
    };
    let candidate = PresentationEvidence::ExternalOnly {
        external: candidate,
    };

    // When: the typed receipts enter the production ordered-motion normalizer and comparator.
    let error = compare_ordered_presentations(&scenario, &reference, &candidate)
        .expect_err("swapped cancellation/recovery must fail");

    // Then: the motion gate observes an ordered semantic-frame defect.
    assert!(
        format!("{error:?}").contains("frame_mismatch_at_ordered_index"),
        "unexpected motion defect: {error:?}"
    );
}

fn motion_observations() -> Vec<TimedSemanticObservation> {
    let phases = ["startup", "cancellation", "recovery", "finish"];
    let mut observations = phases
        .iter()
        .enumerate()
        .map(|(ordinal, phase)| {
            let mut frame = SemanticFrame::new(1, 1, CursorState::hidden(0, 0));
            frame.cells[0].grapheme = (*phase).to_owned();
            TimedSemanticObservation {
                observation_ordinal: ordinal,
                observed_at: PresentationTimestamp(5 + u64::try_from(ordinal).unwrap_or(0) * 10),
                kind: ObservationKind::ReadCompletionDecode,
                decoder_state: DecoderState::Complete,
                raw_read_ordinals: vec![0],
                frame,
            }
        })
        .collect::<Vec<_>>();
    let settled = observations[3].frame.clone();
    for ordinal in 4..7 {
        observations.push(TimedSemanticObservation {
            observation_ordinal: ordinal,
            observed_at: PresentationTimestamp(45),
            kind: ObservationKind::StableRepeat,
            decoder_state: DecoderState::Complete,
            raw_read_ordinals: Vec::new(),
            frame: settled.clone(),
        });
    }
    observations
}
