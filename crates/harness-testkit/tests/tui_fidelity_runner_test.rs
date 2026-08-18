#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast fixture assertions"
)]

#[path = "support/tui_fidelity_lifecycle_cases.rs"]
mod lifecycle_cases;
#[path = "support/tui_fidelity_runner.rs"]
mod support;

use std::path::PathBuf;
use std::time::Duration;

use harness_testkit::parity::semantic_frame_from_vt100_screen;
use harness_testkit::tui_fidelity::{CheckpointError, CheckpointName, Scenario, ScenarioError};
use harness_testkit::tui_fidelity_compare::compare_capture;
use harness_testkit::tui_fidelity_runner::{
    run_compare, run_compare_with_cached_reference, run_compare_with_cached_reference_and_profile,
    CleanupReceipt, PresentationEvidence, PresentationTimestamp, RunnerError, RuntimeBinary,
};

use support::{Fixture, STARTUP_SMOKE};

const PACKET2_SUSTAINED_STREAM: &str =
    include_str!("fixtures/tui_fidelity/packet2-sustained-stream.json");

#[test]
fn baseline_vt100_replay_preserves_unicode_wide_cell_geometry() {
    // Given: the existing shared vt100 terminal replay helper.
    let mut parser = vt100::Parser::new(4, 12, 0);

    // When: a real ANSI byte stream containing a wide glyph is replayed.
    parser.process(b"\x1b[2J\xe9\x9f\xa9A");
    let frame = semantic_frame_from_vt100_screen(parser.screen());

    // Then: the existing parity helper preserves lead and continuation widths.
    assert_eq!(frame.cell(0, 0).expect("lead cell").width, 2);
    assert!(frame.cell(0, 1).expect("continuation cell").continuation);
}

#[test]
fn malformed_scenario_is_rejected_before_process_execution() {
    // Given: an otherwise valid scenario with the settled checkpoint removed.
    let mut value: serde_json::Value = serde_json::from_str(STARTUP_SMOKE).expect("fixture json");
    value["checkpoints"]
        .as_array_mut()
        .expect("checkpoints")
        .pop();

    // When: untrusted JSON crosses the scenario boundary.
    let error = Scenario::from_json(&value.to_string()).expect_err("missing checkpoint must fail");

    // Then: the typed checkpoint error is retained.
    assert!(matches!(
        error,
        ScenarioError::InvalidCheckpoint(CheckpointError::Count { observed: 2 })
            | ScenarioError::InvalidCheckpoint(CheckpointError::Missing(CheckpointName::Settled))
    ));
}

#[test]
fn compare_rejects_missing_reference_binary() {
    let mut fixture = Fixture::new("normal", "normal", "normal");
    fixture.config.reference.path = fixture.root().join("missing-reference");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    let error = run_compare(&scenario, &fixture.config).expect_err("missing binary must fail");

    assert!(matches!(error, RunnerError::MissingBinary { .. }));
}

#[test]
fn compare_rejects_foreign_candidate_binary_before_capture() {
    // Given: a same-digest Harness binary staged under an older Task 49 cache-shaped path.
    let mut fixture = Fixture::new("normal", "normal", "normal");
    let stale_path = fixture
        .root()
        .join("home/.cache/agent-harness-task49/candidate-target/debug/harness");
    std::fs::create_dir_all(stale_path.parent().expect("stale parent")).expect("stale parent");
    std::fs::copy(&fixture.config.harness.path, &stale_path).expect("stale binary copy");
    fixture.config.harness.path = stale_path;
    fixture.config.harness.source_revision = "563efc519c7caa989c54001504b7915a5bfcaf3c".to_owned();
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    // When: the runner is given the foreign candidate binary.
    let error = run_compare(&scenario, &fixture.config).expect_err("foreign binary must fail");

    // Then: binding must reject it before a runtime capture can begin.
    assert!(matches!(error, RunnerError::CandidateBinding { .. }));
    assert!(!fixture.config.evidence_dir.join("harness").exists());
}

#[test]
fn compare_rejects_same_binary_self_comparison() {
    let mut fixture = Fixture::new("normal", "normal", "normal");
    fixture.config.harness = fixture.config.reference.clone();
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    let error = run_compare(&scenario, &fixture.config).expect_err("self comparison must fail");

    assert!(matches!(error, RunnerError::SelfComparison { .. }));
}

#[test]
fn compare_rejects_missing_browser_and_font_capabilities() {
    let mut missing_browser = Fixture::new("normal", "normal", "normal");
    missing_browser.config.renderer.browser_program =
        missing_browser.root().join("missing-browser");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    assert!(matches!(
        run_compare(&scenario, &missing_browser.config).expect_err("missing browser must fail"),
        RunnerError::MissingBrowser { .. }
    ));

    let mut missing_font = Fixture::new("normal", "normal", "normal");
    missing_font.config.renderer.browser_program = PathBuf::from("/bin/true");
    missing_font.config.renderer.font_family = "Definitely Missing Fidelity Font".to_owned();
    assert!(matches!(
        run_compare(&scenario, &missing_font.config).expect_err("missing font must fail"),
        RunnerError::MissingFont { .. }
    ));
}

#[test]
fn compare_rejects_dirty_reference_and_skipped_reference() {
    let mut dirty = Fixture::new("normal", "normal", "normal");
    dirty.config.source_guard.program = support::dirty_source_guard(dirty.root());
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    assert!(matches!(
        run_compare(&scenario, &dirty.config).expect_err("dirty source must fail"),
        RunnerError::DirtyReference { .. }
    ));

    let skipped = Fixture::new("skipped", "normal", "normal");
    assert!(matches!(
        run_compare(&scenario, &skipped.config).expect_err("skipped reference must fail"),
        RunnerError::SkippedReference
    ));
}

#[test]
fn compare_rejects_timeout_premature_exit_and_forced_kill_completion() {
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    let mut timeout = Fixture::new("normal", "normal", "normal");
    timeout.config.timing.scenario_timeout = Duration::from_millis(3);
    assert!(matches!(
        run_compare(&scenario, &timeout.config).expect_err("timeout must fail"),
        RunnerError::Timeout { .. }
    ));

    let premature = Fixture::new("premature", "normal", "normal");
    assert!(matches!(
        run_compare(&scenario, &premature.config).expect_err("premature exit must fail"),
        RunnerError::PrematureExit { .. }
    ));

    let forced = Fixture::new("hang", "normal", "normal");
    assert!(matches!(
        run_compare(&scenario, &forced.config).expect_err("forced kill cannot pass"),
        RunnerError::ForcedKillOnly { .. }
    ));
    let timeline: serde_json::Value = serde_json::from_slice(
        &std::fs::read(forced.config.evidence_dir.join("grok/action-timeline.json"))
            .expect("forced Grok action timeline"),
    )
    .expect("forced Grok action timeline JSON");
    assert_eq!(timeline["phase"], "normal_exit_waiting");
    assert_eq!(timeline["actions"][8]["bytes_hex"], "15");
    assert_eq!(timeline["actions"][9]["bytes_hex"], "2f657869740d");
    assert!(forced
        .config
        .evidence_dir
        .join("grok/terminal-ansi.txt")
        .is_file());
}

#[test]
fn compare_rejects_missing_checkpoint_and_surviving_child() {
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    let missing = Fixture::new("normal", "normal", "missing-checkpoint");
    let missing_error =
        run_compare(&scenario, &missing.config).expect_err("missing checkpoint must fail");
    assert!(
        matches!(&missing_error, RunnerError::MissingCheckpoint { .. }),
        "unexpected missing-checkpoint error: {missing_error:?}"
    );

    let survivor = Fixture::new("survivor", "normal", "normal");
    assert!(matches!(
        run_compare(&scenario, &survivor.config).expect_err("surviving child must fail"),
        RunnerError::SurvivingChild { .. }
    ));
}

#[test]
fn compare_writes_dual_runtime_checkpoint_and_cleanup_receipts() {
    let fixture = Fixture::new("normal", "normal", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    let receipt = run_compare(&scenario, &fixture.config).expect("dual runtime succeeds");

    assert_eq!(receipt.runtimes.len(), 2);
    assert!(fixture.config.evidence_dir.join("receipt.json").is_file());
    assert!(fixture.config.evidence_dir.join("cleanup.json").is_file());
    for adapter in ["grok", "harness"] {
        for checkpoint in ["rest", "mid", "settled"] {
            let root = fixture.config.evidence_dir.join(adapter).join(checkpoint);
            for artifact in [
                "terminal.png",
                "terminal.txt",
                "terminal-ansi.txt",
                "cells.json",
                "cells.txt",
            ] {
                assert!(
                    root.join(artifact).is_file(),
                    "missing {adapter}/{checkpoint}/{artifact}"
                );
            }
        }
    }
}

#[test]
fn presentation_trace_is_native_for_harness_and_external_for_grok() {
    // Given: two real PTY fixture processes with Harness sidecar emission enabled by the runner.
    let fixture = Fixture::new("normal", "normal", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    // When: the production runner captures and compares both adapters.
    let receipt = run_compare(&scenario, &fixture.config).expect("linked presentation receipt");

    // Then: Grok is external-only and Harness has a hashed, byte-linked native sidecar.
    assert!(matches!(
        receipt.runtimes[0].presentation,
        PresentationEvidence::ExternalOnly { .. }
    ));
    assert!(matches!(
        &receipt.runtimes[1].presentation,
        PresentationEvidence::HarnessNative {
            native_trace_artifact,
            links,
            ..
        } if native_trace_artifact.sha256.len() == 64 && !links.is_empty()
    ));
    let PresentationEvidence::HarnessNative { native, .. } = &receipt.runtimes[1].presentation
    else {
        panic!("Harness receipt must contain native presentation evidence");
    };
    assert!(native.causes.iter().any(|cause| {
        cause
            .interaction_id
            .as_ref()
            .is_some_and(|interaction| interaction.0 == format!("{}:action:0", scenario.id.0))
    }));
}

#[test]
fn relative_evidence_path_preserves_harness_native_sidecar_across_runtime_cleanup() {
    // Given: runner-owned evidence addressed relative to the runner cwd while the child uses a
    // separate temporary runtime cwd.
    let mut fixture = Fixture::new("normal", "normal", "normal");
    let runner_cwd = std::env::current_dir().expect("runner cwd");
    let relative_root = runner_cwd.join("target/tui-fidelity-relative-evidence");
    std::fs::create_dir_all(&relative_root).expect("relative evidence parent");
    let evidence = tempfile::tempdir_in(&relative_root).expect("relative evidence tempdir");
    let candidate = evidence.path().join("candidate/debug/harness");
    std::fs::create_dir_all(candidate.parent().expect("candidate parent"))
        .expect("candidate directory");
    std::fs::copy(&fixture.config.harness.path, &candidate).expect("candidate copy");
    fixture.config.harness = RuntimeBinary::from_path(&candidate, "harness-revision")
        .expect("relative evidence candidate identity");
    fixture.config.candidate_binding.candidate_binary_sha256 =
        fixture.config.harness.sha256.clone();
    fixture.config.candidate_binding.target_dir = candidate
        .parent()
        .and_then(std::path::Path::parent)
        .expect("candidate target")
        .to_path_buf();
    fixture.config.repo_root = runner_cwd.clone();
    fixture.config.evidence_dir = evidence
        .path()
        .strip_prefix(&runner_cwd)
        .expect("evidence beneath runner cwd")
        .join("capture");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    // When: the production runner boots, acts on, and normally quits both PTY children.
    let receipt = run_compare(&scenario, &fixture.config).expect("relative evidence compare");

    // Then: Harness telemetry survives runtime-workspace cleanup at the requested evidence path.
    let sidecar = fixture
        .config
        .evidence_dir
        .join("harness/native-presentation.json");
    let trace: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&sidecar).expect("native presentation sidecar"))
            .expect("valid native presentation JSON");
    assert!(trace["frames"]
        .as_array()
        .is_some_and(|frames| !frames.is_empty()));
    assert!(matches!(
        &receipt.runtimes[1].presentation,
        PresentationEvidence::HarnessNative { .. }
    ));
}

#[test]
fn missing_or_failed_native_trace_is_rejected() {
    // Given: a Harness fixture that exits normally but emits no required sidecar.
    let fixture = Fixture::new("normal", "missing-telemetry", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    // When: the production runner reaches Harness receipt construction.
    let error = run_compare(&scenario, &fixture.config).expect_err("missing sidecar must fail");

    // Then: missing telemetry fails closed instead of being reconstructed from checkpoints.
    assert!(
        matches!(error, RunnerError::Io { detail, .. } if detail.contains("required native presentation sidecar"))
    );
}

#[test]
fn cached_reference_requires_exact_presentation_identity() {
    // Given: a fresh external-only reference receipt and a second identical run request.
    let source = Fixture::new("normal", "normal", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    let cached = run_compare(&scenario, &source.config)
        .expect("source receipt")
        .runtimes[0]
        .clone();
    let target = Fixture::new("normal", "normal", "normal");

    // When: the exact cached receipt is supplied to the production runner.
    let receipt = run_compare_with_cached_reference(&scenario, &target.config, Some(cached))
        .expect("exact cached reference");

    // Then: the reference remains external-only and comparison succeeds without recapture.
    assert!(matches!(
        receipt.runtimes[0].presentation,
        PresentationEvidence::ExternalOnly { .. }
    ));
}

#[test]
fn cached_reference_rejects_trace_or_schedule_drift() {
    // Given: a valid cached receipt whose schedule binding is mutated after capture.
    let source = Fixture::new("normal", "normal", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    let mut cached = run_compare(&scenario, &source.config)
        .expect("source receipt")
        .runtimes[0]
        .clone();
    cached.presentation_binding.action_schedule_sha256 = "0".repeat(64);
    let target = Fixture::new("normal", "normal", "normal");

    // When: the stale cached receipt is supplied to the production runner.
    let error = run_compare_with_cached_reference(&scenario, &target.config, Some(cached))
        .expect_err("schedule drift must invalidate cache");

    // Then: cache validation stops before the Harness capture or comparison.
    assert!(
        matches!(error, RunnerError::BinaryReceipt { detail, .. } if detail.contains("stale or incomplete"))
    );
}

#[test]
fn cached_reference_rejects_trace_artifact_drift() {
    // Given: a valid cached reference whose raw PTY artifact changes after receipt creation.
    let source = Fixture::new("normal", "normal", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    let cached = run_compare(&scenario, &source.config)
        .expect("source receipt")
        .runtimes[0]
        .clone();
    let PresentationEvidence::ExternalOnly { external } = &cached.presentation else {
        panic!("Grok receipt must be external-only");
    };
    std::fs::write(&external.raw_ansi.path, b"tampered").expect("tamper raw PTY artifact");
    let target = Fixture::new("normal", "normal", "normal");

    // When: the cached reference is validated against its artifact bytes.
    let error = run_compare_with_cached_reference(&scenario, &target.config, Some(cached))
        .expect_err("artifact drift must invalidate cache");

    // Then: rehashing rejects the stale artifact before comparison.
    assert!(
        matches!(error, RunnerError::BinaryReceipt { detail, .. } if detail.contains("artifact hash changed"))
    );
}

#[test]
fn packet1_complete_receipt_passes_all_gates() {
    // Given: a complete dual-runtime receipt captured from two real PTY fixture processes.
    let fixture = packet1_fixture("complete");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    let receipt = run_compare(&scenario, &fixture.config).expect("complete receipt");

    // When: the production comparator evaluates the persisted receipt again.
    let comparison = compare_capture(&scenario, &receipt, &clean_cleanup());

    // Then: presentation, timing, and ordered motion are required passing gates with metrics.
    assert!(comparison.comparison_passed);
    assert!(comparison.presentation.is_some());
    for gate in ["presentation", "timing", "motion"] {
        assert!(comparison.gates[gate].passed, "gate {gate}");
    }
    write_packet1_artifact("complete-comparison.json", &comparison);
}

#[test]
fn packet1_controlled_defect_matrix() {
    // Given: one complete production receipt and five isolated controlled mutations.
    let fixture = packet1_fixture("defects");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    let receipt = run_compare(&scenario, &fixture.config).expect("complete receipt");
    let cleanup = clean_cleanup();

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

    // When: every mutation traverses the production comparator.
    let delayed_result = compare_capture(&scenario, &delayed, &cleanup);
    let gap_result = compare_capture(&scenario, &long_gap, &cleanup);
    let reorder_result = compare_capture(&scenario, &reordered, &cleanup);
    let schedule_result = compare_capture(&scenario, &shifted_schedule, &cleanup);
    let missing_result = compare_capture(&scenario, &missing, &cleanup);
    write_packet1_artifact(
        "controlled-defects.json",
        &serde_json::json!({
            "artificial_delay_timing_passed": delayed_result.gates["timing"].passed,
            "long_interval_timing_passed": gap_result.gates["timing"].passed,
            "reordered_transition_motion_passed": reorder_result.gates["motion"].passed,
            "schedule_only_timing_passed": schedule_result.gates["timing"].passed,
            "schedule_only_metrics_unchanged": schedule_result.presentation
                == receipt.comparison.as_ref().and_then(|value| value.presentation.clone()),
            "missing_native_presentation_passed": missing_result.gates["presentation"].passed,
            "artificial_delay_detail": delayed_result.gates["timing"].detail,
            "long_interval_detail": gap_result.gates["timing"].detail,
            "reordered_transition_detail": reorder_result.gates["motion"].detail,
            "missing_native_detail": missing_result.gates["presentation"].detail,
        }),
    );

    // Then: each defect reaches its named gate while schedule provenance changes nothing.
    assert!(!delayed_result.gates["timing"].passed);
    assert!(!gap_result.gates["timing"].passed);
    assert!(!reorder_result.gates["motion"].passed);
    assert!(schedule_result.gates["timing"].passed);
    assert_eq!(
        schedule_result.presentation,
        receipt.comparison.unwrap().presentation
    );
    assert!(!missing_result.gates["presentation"].passed);
}

#[test]
fn packet2_complete_real_process_receipt_passes() {
    // Given: the pinned reference and current Harness binaries with isolated loopback fixtures.
    let fixture = real_packet2_fixture("complete");
    let scenario = Scenario::from_json(PACKET2_SUSTAINED_STREAM).expect("Packet 2 scenario");

    // When: the production runner drives both real PTYs through the sustained scenario.
    let receipt = run_compare_with_cached_reference_and_profile(
        &scenario,
        &fixture.config,
        None,
        harness_testkit::tui_fidelity_compare::AcceptanceProfile::Packet2Scheduling,
    )
    .expect("Packet 2 real-process receipt");

    // Then: external evidence exists for both and Harness binds a scheduling sidecar.
    assert_eq!(receipt.runtimes.len(), 2);
    assert!(matches!(
        &receipt.runtimes[1].presentation,
        PresentationEvidence::HarnessNative {
            scheduling_sidecar: Some(sidecar),
            ..
        } if sidecar.sha256.len() == 64
    ));
    let cleanup: CleanupReceipt = serde_json::from_slice(
        &std::fs::read(fixture.config.evidence_dir.join("cleanup.json")).expect("cleanup receipt"),
    )
    .expect("cleanup JSON");
    assert!(cleanup.surviving_pids.is_empty());
    assert!(cleanup.cleanup_errors.is_empty());
}

#[test]
fn packet2_rejects_unobserved_disclosure_and_stale_scheduling_digest() {
    // Given: a complete Packet 2 receipt captured from real processes.
    let fixture = real_packet2_fixture("defect");
    let scenario = Scenario::from_json(PACKET2_SUSTAINED_STREAM).expect("Packet 2 scenario");
    let mut receipt = run_compare_with_cached_reference_and_profile(
        &scenario,
        &fixture.config,
        None,
        harness_testkit::tui_fidelity_compare::AcceptanceProfile::Packet2Scheduling,
    )
    .expect("complete Packet 2 receipt");

    // When: disclosure observations and the scheduling artifact are independently forged.
    let external = harness_external_mut(&mut receipt);
    for observation in &mut external.observations {
        for cell in &mut observation.frame.cells {
            cell.grapheme.clear();
        }
    }
    let disclosure_error =
        harness_testkit::tui_fidelity_runner::validate_packet2_disclosure(external)
            .expect_err("unobserved disclosure must fail");
    let PresentationEvidence::HarnessNative {
        scheduling_sidecar: Some(sidecar),
        ..
    } = &receipt.runtimes[1].presentation
    else {
        panic!("Harness scheduling sidecar");
    };
    std::fs::write(&sidecar.path, b"stale").expect("forge scheduling sidecar");
    let aggregate_error = harness_testkit::tui_fidelity_aggregate::aggregate_with_profile(
        &five_copies_of_run(&fixture.config.evidence_dir),
        harness_testkit::tui_fidelity_compare::AcceptanceProfile::Packet2Scheduling,
    )
    .expect_err("stale scheduling digest must fail");

    // Then: both defects identify their fail-closed evidence boundary.
    assert!(disclosure_error.to_string().contains("transition missing"));
    assert!(aggregate_error.to_string().contains("digest"));
}

fn real_packet2_fixture(label: &str) -> Fixture {
    let mut fixture = Fixture::new("normal", "normal", "normal");
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical repository root");
    let revision = String::from_utf8(
        std::process::Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&repo)
            .output()
            .expect("Git revision")
            .stdout,
    )
    .expect("UTF-8 revision")
    .trim()
    .to_owned();
    let harness = repo.join("target/debug/harness");
    let reference = repo.join("inspirations/grok-build/target/debug/xai-grok-pager");
    fixture.config.repo_root = repo.clone();
    fixture.config.reference =
        RuntimeBinary::from_path(&reference, "eb267feff13129e568df38fb6fdf0ceb65f735d6")
            .expect("reference binary");
    fixture.config.harness = RuntimeBinary::from_path(&harness, &revision).expect("Harness binary");
    fixture.config.candidate_binding.candidate_sha = revision;
    fixture.config.candidate_binding.candidate_binary_sha256 =
        fixture.config.harness.sha256.clone();
    fixture.config.candidate_binding.target_dir = repo.join("target");
    fixture.config.timing = harness_testkit::tui_fidelity_runner::RunnerTiming {
        tick: Duration::from_millis(75),
        scenario_timeout: Duration::from_secs(20),
        normal_exit_timeout: Duration::from_secs(5),
        cleanup_timeout: Duration::from_secs(2),
    };
    if let Some(root) = std::env::var_os("HARNESS_PACKET2_EVIDENCE_DIR") {
        fixture.config.evidence_dir = PathBuf::from(root).join(label);
    }
    fixture
}

fn five_copies_of_run(root: &std::path::Path) -> Vec<PathBuf> {
    vec![root.to_path_buf(); 5]
}

fn write_packet1_artifact(name: &str, value: &impl serde::Serialize) {
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

fn packet1_fixture(label: &str) -> Fixture {
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

fn clean_cleanup() -> CleanupReceipt {
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
    let fixture = Fixture::new("normal", "normal", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    let receipt = run_compare(&scenario, &fixture.config).expect("dual runtime succeeds");
    let comparison_path = fixture.config.evidence_dir.join("comparison.json");
    let comparison: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&comparison_path).expect("comparison receipt"))
            .expect("comparison receipt JSON");

    assert_eq!(comparison["capture_succeeded"], true);
    assert_eq!(comparison["comparison_passed"], true);
    for gate in [
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
    // Given: both PTYs need time to initialize before accepting scripted input.
    let fixture = Fixture::new("delayed-prompt", "delayed-prompt", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    // When: the runner executes the same startup scenario against both adapters.
    let receipt = run_compare(&scenario, &fixture.config).expect("dual runtime succeeds");

    // Then: the first checkpoint contains the prompt, not an echoed first key.
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
