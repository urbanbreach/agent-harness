use super::*;

#[test]
fn packet1_controlled_defect_matrix() {
    // arrange: one complete production receipt and five isolated controlled mutations.
    let fixture = packet1_fixture("defects");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    let receipt = run_capture(&scenario, &fixture.config).expect("complete receipt");
    let baseline_metrics = derive_comparison_presentation_timing(
        &receipt.runtimes[0].presentation,
        &receipt.runtimes[1].presentation,
    )
    .expect("baseline timing metrics");

    let mut delayed = receipt.clone();
    let PresentationEvidence::HarnessNative {
        external, native, ..
    } = &mut delayed.runtimes[1].presentation
    else {
        panic!("Harness native receipt");
    };
    for observation in &mut external.observations {
        observation.observed_at = PresentationTimestamp(observation.observed_at.0 + 10_000_000);
    }
    for ack in &mut native.acknowledgements {
        ack.write_ended_at = PresentationTimestamp(ack.write_ended_at.0 + 10_000_000);
        ack.acknowledged_at = PresentationTimestamp(ack.acknowledged_at.0 + 10_000_000);
    }

    let mut long_gap = receipt.clone();
    let external = harness_external_mut(&mut long_gap);
    let start = external
        .actual_input_sends
        .iter()
        .map(|send| send.sent_at.0)
        .max()
        .unwrap_or_default()
        .saturating_add(1_000);
    for (observation, offset) in external.observations.iter_mut().zip([0_u64, 100, 200, 501]) {
        observation.observed_at = PresentationTimestamp(start + offset);
    }

    let mut reordered = receipt.clone();
    let frame = &mut harness_external_mut(&mut reordered).observations[0].frame;
    frame.cells[0].grapheme = "controlled-transition-defect".to_owned();

    let mut shifted_schedule = receipt.clone();
    let shifted_external = harness_external_mut(&mut shifted_schedule);
    for send in &mut shifted_external.actual_input_sends {
        send.scheduled_at = PresentationTimestamp(send.scheduled_at.0 + 1_000_000);
    }

    let mut missing = receipt.clone();
    let external = harness_external_mut(&mut missing).clone();
    missing.runtimes[1].presentation = PresentationEvidence::ExternalOnly { external };

    // act: every mutation traverses its exact production gate implementation once.
    let delayed_metrics = derive_comparison_presentation_timing(
        &delayed.runtimes[0].presentation,
        &delayed.runtimes[1].presentation,
    )
    .expect("delayed timing metrics");
    let delayed_result =
        compare_presentation_timing(&delayed_metrics.reference, &delayed_metrics.candidate);
    let gap_metrics = derive_comparison_presentation_timing(
        &long_gap.runtimes[0].presentation,
        &long_gap.runtimes[1].presentation,
    )
    .expect("gap timing metrics");
    let gap_result = compare_presentation_timing(&gap_metrics.reference, &gap_metrics.candidate);
    let reorder_result =
        compare_ordered_motion(&scenario, &reordered.runtimes[0], &reordered.runtimes[1]);
    let schedule_metrics = derive_comparison_presentation_timing(
        &shifted_schedule.runtimes[0].presentation,
        &shifted_schedule.runtimes[1].presentation,
    )
    .expect("schedule timing metrics");
    let missing_result = validate_presentation_evidence(
        harness_testkit::tui_fidelity::AdapterKind::Harness,
        &missing.runtimes[1].presentation,
    );
    write_packet1_artifact(
        "controlled-defects.json",
        &serde_json::json!({
            "artificial_delay_timing_passed": delayed_result.is_ok(),
            "long_interval_timing_passed": gap_result.is_ok(),
            "reordered_transition_motion_passed": reorder_result.is_ok(),
            "schedule_only_timing_passed": compare_presentation_timing(
                &schedule_metrics.candidate,
                &schedule_metrics.candidate,
            ).is_ok(),
            "schedule_only_metrics_unchanged": schedule_metrics == baseline_metrics,
            "missing_native_presentation_passed": missing_result.is_ok(),
            "artificial_delay_detail": delayed_result.as_ref().err().map(ToString::to_string),
            "long_interval_detail": gap_result.as_ref().err().map(ToString::to_string),
            "reordered_transition_detail": reorder_result.as_ref().err().map(ToString::to_string),
            "missing_native_detail": missing_result.as_ref().err().map(ToString::to_string),
        }),
    );

    // assert: each defect reaches its named gate while schedule provenance changes nothing.
    assert!(delayed_result.is_err());
    assert!(gap_result.is_err());
    assert!(reorder_result.is_err());
    assert!(
        compare_presentation_timing(&schedule_metrics.candidate, &schedule_metrics.candidate,)
            .is_ok()
    );
    assert_eq!(schedule_metrics, baseline_metrics);
    assert!(missing_result.is_err());
}

#[test]
fn fixture_receipt_is_a_bounded_non_accepting_transport_boundary() {
    // arrange: two independent PTY programs use an explicitly non-eligible fixture binding.
    let mut fixture = Fixture::new("normal", "normal", "normal");
    fixture.config.candidate_binding.receipt_kind = CandidateReceiptKind::Fixture;
    fixture.config.candidate_binding.parity_acceptance_eligible = false;
    fixture.config.candidate_binding.release_eligible = false;
    fixture.config.candidate_binding.clean_release = false;
    if let Some(root) = std::env::var_os("HARNESS_PACKET2_EVIDENCE_DIR") {
        fixture.config.evidence_dir = PathBuf::from(root).join("fixture-boundary");
    }
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("startup scenario");

    // act: the production runner persists the rejected fixture receipt and performs teardown.
    let started = std::time::Instant::now();
    let error = run_compare(&scenario, &fixture.config)
        .expect_err("fixture receipt must not become acceptance-eligible");
    assert!(
        matches!(error, RunnerError::Comparison { .. }),
        "fixture capture stopped before comparison: {error}"
    );
    let elapsed = started.elapsed();
    let receipt: DualRuntimeReceipt = serde_json::from_slice(
        &std::fs::read(fixture.config.evidence_dir.join("receipt.json")).expect("fixture receipt"),
    )
    .expect("fixture receipt JSON");
    let cleanup: CleanupReceipt = serde_json::from_slice(
        &std::fs::read(fixture.config.evidence_dir.join("cleanup.json")).expect("cleanup receipt"),
    )
    .expect("cleanup JSON");

    // assert: transport, provenance rejection, resource bounds, and cleanup are truthful.
    assert!(elapsed <= Duration::from_secs(15), "elapsed={elapsed:?}");
    assert_eq!(receipt.runtimes.len(), 2);
    assert_ne!(
        receipt.runtimes[0].binary.sha256,
        receipt.runtimes[1].binary.sha256
    );
    for runtime in &receipt.runtimes {
        let external = match &runtime.presentation {
            PresentationEvidence::ExternalOnly { external }
            | PresentationEvidence::HarnessNative { external, .. } => external,
        };
        assert!(!external.raw_reads.is_empty());
        assert!(external.observations.len() <= 32);
        assert!(
            std::fs::metadata(&external.observations_artifact.path)
                .expect("observation artifact metadata")
                .len()
                <= 32 * 1024 * 1024
        );
    }
    let candidate_external = match &receipt.runtimes[1].presentation {
        PresentationEvidence::HarnessNative { external, .. } => external,
        PresentationEvidence::ExternalOnly { .. } => panic!("Harness native receipt required"),
    };
    assert!(
        harness_testkit::tui_fidelity_runner::validate_packet2_disclosure(candidate_external)
            .expect_err("fixture has no Packet2 disclosure transition")
            .to_string()
            .contains("transition missing")
    );
    let comparison = receipt.comparison.expect("comparison receipt");
    assert!(!comparison.gates["provenance"].passed);
    assert!(!comparison.comparison_passed);
    assert_eq!(
        receipt.candidate_binding.receipt_kind,
        CandidateReceiptKind::Fixture
    );
    assert!(!receipt.candidate_binding.parity_acceptance_eligible);
    assert!(cleanup.surviving_pids.is_empty());
    assert!(cleanup.cleanup_errors.is_empty());
}

pub(super) fn write_packet1_artifact(name: &str, value: &impl serde::Serialize) {
    let Some(root) = std::env::var_os("HARNESS_PACKET1_EVIDENCE_DIR") else {
        return;
    };
    let path = PathBuf::from(root).join(name);
    std::fs::create_dir_all(path.parent().expect("packet evidence parent"))
        .expect("create packet evidence root");
    std::fs::write(
        path,
        serde_json::to_vec_pretty(value).expect("serialize packet evidence"),
    )
    .expect("write packet evidence");
}

pub(super) fn packet1_fixture(label: &str) -> Fixture {
    let mut fixture = Fixture::new("normal", "normal", "normal");
    if let Some(root) = std::env::var_os("HARNESS_PACKET1_EVIDENCE_DIR") {
        fixture.config.evidence_dir = PathBuf::from(root).join(label);
    }
    fixture
}

fn harness_external_mut(
    receipt: &mut harness_testkit::tui_fidelity_runner::DualRuntimeReceipt,
) -> &mut harness_testkit::tui_fidelity_runner::ExternalPresentationEvidence {
    match &mut receipt.runtimes[1].presentation {
        PresentationEvidence::HarnessNative { external, .. } => external,
        PresentationEvidence::ExternalOnly { .. } => panic!("Harness native receipt required"),
    }
}

pub(super) fn clean_cleanup() -> CleanupReceipt {
    CleanupReceipt {
        schema_version: "harness.tui-fidelity.cleanup.v1".to_owned(),
        status: "clean".to_owned(),
        forced_termination_observed: false,
        detected_child_pids: Vec::new(),
        surviving_pids: Vec::new(),
        temporary_paths_removed: Vec::new(),
        cleanup_errors: Vec::new(),
        primary_error: None,
    }
}

#[test]
fn compare_writes_explicit_capture_and_comparison_gate_receipt() {
    // arrange
    let fixture = Fixture::new("normal", "normal", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    let receipt = run_compare(&scenario, &fixture.config).expect("dual runtime succeeds");
    let comparison_path = fixture.config.evidence_dir.join("comparison.json");
    // act
    let comparison: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&comparison_path).expect("comparison receipt"))
            .expect("comparison receipt JSON");
    write_packet1_artifact("all-nine-gates.json", &comparison);

    // assert
    assert_eq!(comparison["capture_succeeded"], true);
    assert_eq!(comparison["comparison_passed"], true);
    for gate in [
        "presentation",
        "semantic_cell",
        "pixel",
        "motion",
        "timing",
        "provenance",
        "checkpoint",
        "exit",
        "cleanup",
    ] {
        assert_eq!(comparison["gates"][gate]["passed"], true, "gate {gate}");
    }
    assert!(receipt.comparison.is_some());
}

#[test]
fn compare_waits_for_prompt_before_sending_first_action() {
    // arrange: both PTYs need time to initialize before accepting scripted input.
    let fixture = Fixture::new("delayed-prompt", "delayed-prompt", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    // act: the runner executes the same startup scenario against both adapters.
    let receipt = run_capture(&scenario, &fixture.config).expect("dual runtime succeeds");

    // assert: the first checkpoint contains the prompt, not an echoed first key.
    assert!(receipt.comparison.is_none());
    for runtime in &receipt.runtimes {
        assert!(
            runtime
                .input_timestamps_millis
                .first()
                .is_some_and(|timestamp| *timestamp < 25),
            "{} action clock must start after readiness",
            runtime.adapter.as_str()
        );
    }
    for adapter in ["grok", "harness"] {
        let ansi = std::fs::read_to_string(
            fixture
                .config
                .evidence_dir
                .join(adapter)
                .join("rest/terminal-ansi.txt"),
        )
        .expect("checkpoint ANSI stream");
        assert!(ansi.contains('❯'), "{adapter} prompt must be captured");
        assert!(
            !ansi.contains('h'),
            "{adapter} must not capture pre-ready input"
        );
    }
}
