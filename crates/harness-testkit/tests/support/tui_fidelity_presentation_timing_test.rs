#[test]
fn external_cadence_starts_at_first_action_receipt_epoch() {
    // arrange: two bootstrap frames precede an observer action and post-start continuous motion.
    let mut evidence = external();
    evidence.action_receipts = vec![
        ActionExecutionReceipt {
            interaction_id: InteractionId("startup:action:observer".into()),
            action_ordinal: 0,
            kind: ActionExecutionKind::Observer,
            scheduled_at: PresentationTimestamp(611_675),
            started_at: PresentationTimestamp(612_000),
            ended_at: PresentationTimestamp(613_000),
            result: ActionExecutionResult::ObservedText,
            semantic_pre: SemanticSnapshot {
                cols: 1,
                rows: 1,
                states: Vec::new(),
            },
            semantic_post: SemanticSnapshot {
                cols: 1,
                rows: 1,
                states: Vec::new(),
            },
            expected_native_cause_count: None,
        },
        ActionExecutionReceipt {
            interaction_id: InteractionId("startup:action:input".into()),
            action_ordinal: 1,
            kind: ActionExecutionKind::Input,
            scheduled_at: PresentationTimestamp(700_000),
            started_at: PresentationTimestamp(700_100),
            ended_at: PresentationTimestamp(700_200),
            result: ActionExecutionResult::Applied,
            semantic_pre: SemanticSnapshot {
                cols: 1,
                rows: 1,
                states: Vec::new(),
            },
            semantic_post: SemanticSnapshot {
                cols: 1,
                rows: 1,
                states: Vec::new(),
            },
            expected_native_cause_count: Some(1),
        },
    ];
    evidence.actual_input_sends = vec![ActualInputSend {
        interaction_id: InteractionId("startup:action:input".into()),
        action_ordinal: 1,
        scheduled_at: PresentationTimestamp(700_000),
        sent_at: PresentationTimestamp(710_000),
        transport_drained_at: None,
    }];
    evidence.interaction_observations = vec![InteractionObservation {
        interaction_id: InteractionId("startup:action:input".into()),
        first_changed_observation: Some(4),
        diagnostic: None,
    }];
    evidence.observations = [84_967, 464_744, 620_000, 650_000, 720_000, 750_000]
        .into_iter()
        .enumerate()
        .map(
            |(observation_ordinal, observed_at)| TimedSemanticObservation {
                observation_ordinal,
                observed_at: PresentationTimestamp(observed_at),
                kind: ObservationKind::ReadCompletionDecode,
                decoder_state: DecoderState::Complete,
                raw_read_ordinals: vec![observation_ordinal],
                frame: SemanticFrame::new(1, 1, CursorState::hidden(0, 0)),
            },
        )
        .collect();

    // act: timing is derived once from valid receipts and once after corrupting the epoch receipt.
    let metrics = derive_presentation_timing(&PresentationEvidence::ExternalOnly {
        external: evidence.clone(),
    })
    .expect("valid scenario epoch");
    let mut earlier_epoch = evidence.clone();
    earlier_epoch.action_receipts[0].scheduled_at = PresentationTimestamp(80_000);
    let earlier_metrics = derive_presentation_timing(&PresentationEvidence::ExternalOnly {
        external: earlier_epoch,
    })
    .expect("earlier valid scenario epoch");
    let mut missing_epoch = evidence.clone();
    missing_epoch.action_receipts.clear();
    let missing = derive_presentation_timing(&PresentationEvidence::ExternalOnly {
        external: missing_epoch,
    });
    evidence.action_receipts[0].started_at = PresentationTimestamp(611_674);
    let malformed =
        derive_presentation_timing(&PresentationEvidence::ExternalOnly { external: evidence });

    // assert: bootstrap cadence is absent, post-start cadence remains, and malformed epochs fail.
    assert_eq!(
        metrics.external_observation_timestamps_micros,
        vec![620_000, 650_000, 720_000, 750_000]
    );
    assert_eq!(
        metrics.external_observation_intervals_micros,
        vec![30_000, 30_000]
    );
    assert!(!metrics
        .external_observation_intervals_micros
        .contains(&379_777));
    assert!(earlier_metrics
        .external_observation_intervals_micros
        .contains(&379_777));
    assert!(missing.is_err());
    assert!(malformed.is_err());
}

#[test]
fn r24_idle_action_boundary_is_not_a_continuous_cadence_gap() {
    // arrange: startup animation is continuous, then click and Esc produce isolated writes.
    let mut evidence = external();
    evidence.actual_input_sends = vec![
        ActualInputSend {
            interaction_id: InteractionId("scenario:action:click".into()),
            action_ordinal: 1,
            scheduled_at: PresentationTimestamp(300_000),
            sent_at: PresentationTimestamp(305_000),
            transport_drained_at: None,
        },
        ActualInputSend {
            interaction_id: InteractionId("scenario:action:escape".into()),
            action_ordinal: 2,
            scheduled_at: PresentationTimestamp(600_000),
            sent_at: PresentationTimestamp(604_000),
            transport_drained_at: None,
        },
    ];
    evidence.interaction_observations = vec![
        InteractionObservation {
            interaction_id: InteractionId("scenario:action:click".into()),
            first_changed_observation: Some(4),
            diagnostic: None,
        },
        InteractionObservation {
            interaction_id: InteractionId("scenario:action:escape".into()),
            first_changed_observation: Some(5),
            diagnostic: None,
        },
    ];
    evidence.observations = [6_575, 87_927, 171_359, 253_706, 309_713, 624_894]
        .into_iter()
        .enumerate()
        .map(
            |(observation_ordinal, observed_at)| TimedSemanticObservation {
                observation_ordinal,
                observed_at: PresentationTimestamp(observed_at),
                kind: ObservationKind::ReadCompletionDecode,
                decoder_state: DecoderState::Complete,
                raw_read_ordinals: vec![observation_ordinal],
                frame: SemanticFrame::new(1, 1, CursorState::hidden(0, 0)),
            },
        )
        .collect();
    let mut trace = native();
    trace.causes = vec![
        native_cause("startup", None, 323, 1, "startup"),
        native_cause("animation-1", None, 84_011, 2, "animation_timer"),
        native_cause("animation-2", None, 167_270, 3, "animation_timer"),
        native_cause("animation-3", None, 249_644, 4, "animation_timer"),
        native_cause(
            "click-down",
            Some("scenario:action:click"),
            305_780,
            6,
            "mouse",
        ),
        native_cause(
            "click-up",
            Some("scenario:action:click"),
            305_804,
            6,
            "mouse",
        ),
        native_cause(
            "escape",
            Some("scenario:action:escape"),
            604_595,
            8,
            "terminal_input",
        ),
    ];
    trace.frames = vec![
        native_frame(1, 1, &["startup"], 6_575),
        native_frame(2, 2, &["animation-1"], 87_927),
        native_frame(3, 3, &["animation-2"], 171_359),
        native_frame(4, 4, &["animation-3"], 253_706),
        native_frame(5, 6, &["click-down", "click-up"], 309_713),
        native_frame(6, 8, &["escape"], 624_894),
    ];
    trace.acknowledgements = trace
        .frames
        .iter()
        .map(|frame| NativeFrameAck {
            sequence: frame.sequence,
            revision: frame.revision,
            byte_sha256: frame.byte_sha256.clone(),
            write_started_at: PresentationTimestamp(frame.requested_at.0 + 1),
            write_ended_at: PresentationTimestamp(frame.render_ended_at.0),
            acknowledged_at: PresentationTimestamp(frame.render_ended_at.0 + 1),
            outcome: NativeAckOutcome::CompletedWrite,
        })
        .collect();
    let evidence = PresentationEvidence::HarnessNative {
        external: evidence,
        native: Box::new(trace),
        native_trace_artifact: ArtifactDigest {
            path: "native.json".into(),
            sha256: "e".repeat(64),
        },
        scheduling_sidecar: None,
        links: Vec::new(),
    };

    // act
    let metrics = derive_presentation_timing(&evidence).expect("r24-shaped timing metrics");
    let native = metrics.native.expect("native metrics");

    // assert: only the continuous animation epoch has cadence, and each input has one latency.
    assert_eq!(
        native.completed_write_intervals_micros,
        vec![83_432, 82_347]
    );
    assert_eq!(
        native.receive_to_successful_flush_micros,
        vec![3_909, 20_299]
    );
    assert!(!native.completed_write_intervals_micros.contains(&315_181));
}

fn native_cause(
    cause_id: &str,
    interaction_id: Option<&str>,
    received_at: u64,
    revision: u64,
    kind: &str,
) -> NativeCause {
    NativeCause {
        cause_id: cause_id.into(),
        interaction_id: interaction_id.map(|value| InteractionId(value.into())),
        received_at: PresentationTimestamp(received_at),
        kind: kind.into(),
        resulting_revision: Some(revision),
        outcome: NativeCauseOutcome::VisibleChange,
    }
}

fn native_frame(sequence: u64, revision: u64, causes: &[&str], write_ended_at: u64) -> NativeFrame {
    NativeFrame {
        sequence,
        revision,
        cause_ids: causes.iter().map(|cause| (*cause).into()).collect(),
        requested_at: PresentationTimestamp(write_ended_at.saturating_sub(3)),
        render_started_at: PresentationTimestamp(write_ended_at.saturating_sub(2)),
        render_ended_at: PresentationTimestamp(write_ended_at),
        submitted_at: PresentationTimestamp(write_ended_at.saturating_sub(1)),
        frame_kind: "differential".into(),
        byte_count: 1,
        byte_sha256: format!("{sequence:064x}"),
    }
}

#[test]
fn production_motion_rejects_swapped_cancellation() {
    // arrange: complete typed external receipts with cancellation and recovery frames swapped only on Harness.
    let scenario = harness_testkit::tui_fidelity::Scenario::from_json(include_str!(
        "../../src/tui_fidelity_scenarios/baseline/cancel.json"
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

    // act: the typed receipts enter the production ordered-motion normalizer and comparator.
    let error = compare_ordered_presentations(&scenario, &reference, &candidate)
        .expect_err("swapped cancellation/recovery must fail");

    // assert: the motion gate observes an ordered semantic-frame defect.
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
