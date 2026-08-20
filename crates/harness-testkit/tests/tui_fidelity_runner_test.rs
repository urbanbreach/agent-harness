#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    reason = "owner tests use fail-fast fixture assertions"
)]

#[path = "support/tui_fidelity_runner_candidate_binding_support.rs"]
mod candidate_binding_cases;
#[path = "support/harness_bin.rs"]
mod harness_bin;
#[path = "support/tui_fidelity_lifecycle_cases_support.rs"]
mod lifecycle_cases;
#[path = "support/tui_fidelity_runner.rs"]
mod support;

use std::path::PathBuf;
use std::time::Duration;

use harness_testkit::parity::semantic_frame_from_vt100_screen;
use harness_testkit::tui_fidelity::{CheckpointError, CheckpointName, Scenario, ScenarioError};
use harness_testkit::tui_fidelity_compare::{
    compare_capture, compare_ordered_motion, compare_presentation_timing,
    derive_comparison_presentation_timing,
};
use harness_testkit::tui_fidelity_runner::{
    run_capture, run_compare, run_compare_with_cached_reference,
    run_compare_with_cached_reference_and_profile, semantic_state_matches, semantic_state_observed,
    validate_presentation_evidence, CandidateReceiptKind, CleanupReceipt, DualRuntimeReceipt,
    PresentationEvidence, PresentationTimestamp, RunnerError, RuntimeBinary,
};

use support::{Fixture, STARTUP_SMOKE};

#[test]
fn semantic_state_predicate_rejects_active_reference_idle_candidate() {
    // arrange: an active streaming reference frame and an idle prompt-ready candidate frame.
    let mut active_parser = vt100::Parser::new(24, 80, 0);
    active_parser.process(b"I inspected the requested stream.\r\nworking");
    let active = semantic_frame_from_vt100_screen(active_parser.screen());
    let mut idle_parser = vt100::Parser::new(24, 80, 0);
    idle_parser.process(b"fixture-ready\r\n\x1b[1;1H\xe2\x9d\xaf");
    let idle = semantic_frame_from_vt100_screen(idle_parser.screen());

    // act: both frames are checked against the same named checkpoint.
    let reference_matches = semantic_state_matches(
        harness_testkit::tui_fidelity::SemanticState::Streaming,
        &active,
        active.cols,
        active.rows,
    );
    let candidate_matches = semantic_state_matches(
        harness_testkit::tui_fidelity::SemanticState::Streaming,
        &idle,
        idle.cols,
        idle.rows,
    );

    // assert: an idle candidate cannot be classified as the active semantic checkpoint.
    assert!(reference_matches);
    assert!(!candidate_matches);
}

#[test]
fn resized_state_requires_fresh_rows_after_the_resize_boundary() {
    // arrange: a target-sized parser frame but no PTY bytes after the resize action boundary.
    let parser = vt100::Parser::new(30, 100, 0);
    let frame = semantic_frame_from_vt100_screen(parser.screen());

    // act: the resize observer evaluates the frame with and without a fresh post-action row.
    let stale = semantic_state_observed(
        harness_testkit::tui_fidelity::SemanticState::Resized,
        &frame,
        100,
        30,
        64,
        Some(64),
    );
    let fresh = semantic_state_observed(
        harness_testkit::tui_fidelity::SemanticState::Resized,
        &frame,
        100,
        30,
        65,
        Some(64),
    );

    // assert: parser dimensions alone cannot fabricate a successful resize receipt.
    assert!(!stale);
    assert!(fresh);
}

#[test]
fn baseline_vt100_replay_preserves_unicode_wide_cell_geometry() {
    // arrange: the existing shared vt100 terminal replay helper.
    let mut parser = vt100::Parser::new(4, 12, 0);

    // act: a real ANSI byte stream containing a wide glyph is replayed.
    parser.process(b"\x1b[2J\xe9\x9f\xa9A");
    let frame = semantic_frame_from_vt100_screen(parser.screen());

    // assert: the existing parity helper preserves lead and continuation widths.
    assert_eq!(frame.cell(0, 0).expect("lead cell").width, 2);
    assert!(frame.cell(0, 1).expect("continuation cell").continuation);
}

#[test]
fn malformed_scenario_is_rejected_before_process_execution() {
    // arrange: an otherwise valid scenario with the settled checkpoint removed.
    let mut value: serde_json::Value = serde_json::from_str(STARTUP_SMOKE).expect("fixture json");
    value["checkpoints"]
        .as_array_mut()
        .expect("checkpoints")
        .pop();

    // act: untrusted JSON crosses the scenario boundary.
    let error = Scenario::from_json(&value.to_string()).expect_err("missing checkpoint must fail");

    // assert: the typed checkpoint error is retained.
    assert!(matches!(
        error,
        ScenarioError::InvalidCheckpoint(CheckpointError::Count { observed: 2 })
            | ScenarioError::InvalidCheckpoint(CheckpointError::Missing(CheckpointName::Settled))
    ));
}

#[test]
fn compare_rejects_missing_reference_binary() {
    // arrange
    let mut fixture = Fixture::new("normal", "normal", "normal");
    fixture.config.reference.path = fixture.root().join("missing-reference");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    // act
    let error = run_compare(&scenario, &fixture.config).expect_err("missing binary must fail");

    // assert
    assert!(matches!(error, RunnerError::MissingBinary { .. }));
}

#[test]
fn compare_rejects_foreign_candidate_binary_before_capture() {
    // arrange: a same-digest Harness binary staged under an older Task 49 cache-shaped path.
    let mut fixture = Fixture::new("normal", "normal", "normal");
    let stale_path = fixture
        .root()
        .join("home/.cache/agent-harness-task49/candidate-target/debug/harness");
    std::fs::create_dir_all(stale_path.parent().expect("stale parent")).expect("stale parent");
    std::fs::copy(&fixture.config.harness.path, &stale_path).expect("stale binary copy");
    fixture.config.harness.path = stale_path;
    fixture.config.harness.source_revision = "563efc519c7caa989c54001504b7915a5bfcaf3c".to_owned();
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    // act: the runner is given the foreign candidate binary.
    let error = run_compare(&scenario, &fixture.config).expect_err("foreign binary must fail");

    // assert: binding must reject it before a runtime capture can begin.
    assert!(matches!(error, RunnerError::CandidateBinding { .. }));
    assert!(!fixture.config.evidence_dir.join("harness").exists());
}

#[test]
fn compare_rejects_same_binary_self_comparison() {
    // arrange
    let mut fixture = Fixture::new("normal", "normal", "normal");
    fixture.config.harness = fixture.config.reference.clone();
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    // act
    let error = run_compare(&scenario, &fixture.config).expect_err("self comparison must fail");

    // assert
    assert!(matches!(error, RunnerError::SelfComparison { .. }));
}

#[test]
fn compare_rejects_missing_browser_and_font_capabilities() {
    // arrange
    let mut missing_browser = Fixture::new("normal", "normal", "normal");
    missing_browser.config.renderer.browser_program =
        missing_browser.root().join("missing-browser");
    // act
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    // assert
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
    // arrange
    let mut dirty = Fixture::new("normal", "normal", "normal");
    dirty.config.source_guard.program = support::dirty_source_guard(dirty.root());
    // act
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    // assert
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
    // arrange
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    // act
    let mut timeout = Fixture::new("normal", "normal", "normal");
    timeout.config.timing.scenario_timeout = Duration::from_millis(3);
    // assert
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
    // arrange
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    let missing = Fixture::new("normal", "normal", "missing-checkpoint");
    // act
    let missing_error =
        run_compare(&scenario, &missing.config).expect_err("missing checkpoint must fail");
    // assert
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
    // arrange
    let fixture = Fixture::new("normal", "normal", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    // act
    let receipt = run_compare(&scenario, &fixture.config).expect("dual runtime succeeds");

    // assert
    assert_eq!(receipt.runtimes.len(), 2);
    assert!(fixture.config.evidence_dir.join("receipt.json").is_file());
    assert!(fixture.config.evidence_dir.join("cleanup.json").is_file());
    for runtime in &receipt.runtimes {
        let external = match &runtime.presentation {
            PresentationEvidence::ExternalOnly { external }
            | PresentationEvidence::HarnessNative { external, .. } => external,
        };
        assert_eq!(external.action_receipts.len(), scenario.actions.len());
        assert_eq!(
            external
                .action_receipts
                .iter()
                .map(|action| action.action_ordinal)
                .collect::<Vec<_>>(),
            (0..scenario.actions.len()).collect::<Vec<_>>()
        );
        assert!(external.action_receipts.iter().any(|action| matches!(
            action.kind,
            harness_testkit::tui_fidelity_runner::ActionExecutionKind::Observer
        )));
    }
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
    if let Ok(path) = std::env::var("HARNESS_SEMANTIC_RECEIPT_EVIDENCE") {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("semantic receipt evidence parent");
        }
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&receipt).expect("serialize semantic receipt evidence"),
        )
        .expect("write semantic receipt evidence");
    }
}

#[test]
fn runtime_workspace_is_removed_when_capture_fails() {
    // arrange: a reference process that exits before capture completes.
    let fixture = Fixture::new("premature", "normal", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    // act: the capture fails after the runner has created its owned workspace.
    let _error = run_compare(&scenario, &fixture.config).expect_err("capture must fail");

    // assert: failure still removes the complete runner-owned base.
    assert!(!fixture.root().join("tmp/tui-fidelity").exists());
}

#[test]
fn source_guards_match_binding_after_unignored_runtime_workspace_cleanup() {
    // arrange: source provenance observes generated files under tmp/tui-fidelity.
    let mut fixture = Fixture::new("normal", "normal", "normal");
    fixture.expose_runtime_workspace_to_source_guard();
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    // act: both adapters capture successfully through the production lifecycle.
    let _comparison_result = run_compare(&scenario, &fixture.config);
    let receipt: DualRuntimeReceipt = serde_json::from_slice(
        &std::fs::read(fixture.config.evidence_dir.join("receipt.json"))
            .expect("successful capture receipt"),
    )
    .expect("successful capture receipt JSON");

    // assert: neither source guard can observe the transient runtime workspace.
    let bound_bytes = std::fs::read(fixture.root().join("source-guard-template.json"))
        .expect("bound source guard bytes");
    assert_eq!(
        (
            std::fs::read(&receipt.source_guard_before.path).expect("before source guard bytes"),
            std::fs::read(&receipt.source_guard_after.path).expect("after source guard bytes")
        ),
        (bound_bytes.clone(), bound_bytes)
    );
    if let Ok(path) = std::env::var("HARNESS_SOURCE_GUARD_LIFECYCLE_EVIDENCE") {
        let path = PathBuf::from(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("lifecycle evidence parent");
        }
        let cleanup: serde_json::Value = serde_json::from_slice(
            &std::fs::read(fixture.config.evidence_dir.join("cleanup.json"))
                .expect("cleanup receipt"),
        )
        .expect("cleanup receipt JSON");
        let comparison: serde_json::Value = serde_json::from_slice(
            &std::fs::read(fixture.config.evidence_dir.join("comparison.json"))
                .expect("comparison receipt"),
        )
        .expect("comparison receipt JSON");
        let runtime_workspace_removed = !fixture.root().join("tmp/tui-fidelity").exists();
        let workspace_cleanup_succeeded = runtime_workspace_removed
            && cleanup["cleanup_errors"]
                .as_array()
                .is_some_and(Vec::is_empty);
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": "harness.tui-fidelity.source-guard-lifecycle.v1",
                "bound_sha256": fixture.config.candidate_binding.source_guard_receipt_sha256,
                "before": receipt.source_guard_before,
                "after": receipt.source_guard_after,
                "capture_succeeded": comparison["capture_succeeded"],
                "runtime_workspace_removed": runtime_workspace_removed,
                "workspace_cleanup_succeeded": workspace_cleanup_succeeded,
                "cleanup": cleanup,
            }))
            .expect("serialize lifecycle evidence"),
        )
        .expect("write lifecycle evidence");
    }
}

#[test]
fn presentation_trace_is_native_for_harness_and_external_for_grok() {
    // arrange: two real PTY fixture processes with Harness sidecar emission enabled by the runner.
    let fixture = Fixture::new("normal", "normal", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    // act: the production runner captures both adapters.
    let receipt = run_capture(&scenario, &fixture.config).expect("linked presentation receipt");

    // assert: Grok is external-only and Harness has a hashed, byte-linked native sidecar.
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
    let action_zero_observed = native.causes.iter().any(|cause| {
        cause
            .interaction_id
            .as_ref()
            .is_some_and(|interaction| interaction.0 == format!("{}:action:0", scenario.id.0))
    });
    write_packet1_artifact(
        "presentation-shape.json",
        &serde_json::json!({
            "reference_external_only": true,
            "candidate_harness_native": true,
            "candidate_action_zero_observed": action_zero_observed,
        }),
    );
    assert!(action_zero_observed);
}

#[test]
fn relative_evidence_path_preserves_harness_native_sidecar_across_runtime_cleanup() {
    // arrange: runner-owned evidence addressed relative to the runner cwd while the child uses a
    // separate temporary runtime cwd.
    let mut fixture = Fixture::new("normal", "normal", "normal");
    let runner_cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let relative_root = runner_cwd.join("target/tui-fidelity-relative-evidence");
    std::fs::create_dir_all(&relative_root).expect("relative evidence parent");
    let evidence = tempfile::tempdir_in(&relative_root).expect("relative evidence tempdir");
    let candidate = evidence.path().join("candidate/debug/harness");
    fixture.relocate_candidate_bundle(&candidate);
    fixture.config.repo_root = runner_cwd.clone();
    fixture.config.evidence_dir = evidence
        .path()
        .strip_prefix(&runner_cwd)
        .expect("evidence beneath runner cwd")
        .join("capture");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    // act: the production runner boots, acts on, and normally quits both PTY children.
    let receipt = run_capture(&scenario, &fixture.config).expect("relative evidence capture");

    // assert: Harness telemetry survives runtime-workspace cleanup at the requested evidence path.
    let sidecar = runner_cwd
        .join(&fixture.config.evidence_dir)
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
    // arrange: a Harness fixture that exits normally but emits no required sidecar.
    let fixture = Fixture::new("normal", "missing-telemetry", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");

    // act: the production runner reaches Harness receipt construction.
    let error = run_compare(&scenario, &fixture.config).expect_err("missing sidecar must fail");

    // assert: missing telemetry fails closed instead of being reconstructed from checkpoints.
    assert!(
        matches!(error, RunnerError::Io { detail, .. } if detail.contains("required native presentation sidecar"))
    );
}

#[test]
fn cached_reference_requires_exact_presentation_identity() {
    // arrange: a fresh external-only reference receipt and a second identical run request.
    let source = Fixture::new("normal", "normal", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    let cached = run_capture(&scenario, &source.config)
        .expect("source receipt")
        .runtimes[0]
        .clone();
    let target = Fixture::new("normal", "normal", "normal");

    // act: the exact cached receipt is supplied to the production runner.
    let result = run_compare_with_cached_reference(&scenario, &target.config, Some(cached.clone()));
    let receipt = receipt_after_cached_reference_validation(result, &target.config.evidence_dir);

    // assert: the supplied reference is retained exactly and reaches the comparison boundary.
    assert_eq!(receipt.runtimes[0], cached);
    assert!(matches!(
        receipt.runtimes[0].presentation,
        PresentationEvidence::ExternalOnly { .. }
    ));
    assert!(receipt.comparison.is_some());
    write_packet1_artifact(
        "cached-reference-identity.json",
        &serde_json::json!({
            "cached_reference_retained_exactly": receipt.runtimes[0] == cached,
            "reference_external_only": matches!(
                receipt.runtimes[0].presentation,
                PresentationEvidence::ExternalOnly { .. }
            ),
            "comparison_boundary_reached": receipt.comparison.is_some(),
        }),
    );
}

#[test]
fn cached_reference_rejects_trace_or_schedule_drift() {
    // arrange: a valid cached receipt whose schedule binding is mutated after capture.
    let source = Fixture::new("normal", "normal", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    let mut cached = run_capture(&scenario, &source.config)
        .expect("source receipt")
        .runtimes[0]
        .clone();
    cached.presentation_binding.action_schedule_sha256 = "0".repeat(64);
    let target = Fixture::new("normal", "normal", "normal");

    // act: the stale cached receipt is supplied to the production runner.
    let error = run_compare_with_cached_reference(&scenario, &target.config, Some(cached))
        .expect_err("schedule drift must invalidate cache");

    // assert: cache validation stops before the Harness capture or comparison.
    assert!(
        matches!(error, RunnerError::BinaryReceipt { detail, .. } if detail.contains("stale or incomplete"))
    );
}

#[test]
fn cached_reference_rejects_trace_artifact_drift() {
    // arrange: a valid cached reference whose raw PTY artifact changes after receipt creation.
    let source = Fixture::new("normal", "normal", "normal");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    let cached = run_capture(&scenario, &source.config)
        .expect("source receipt")
        .runtimes[0]
        .clone();
    let PresentationEvidence::ExternalOnly { external } = &cached.presentation else {
        panic!("Grok receipt must be external-only");
    };
    std::fs::write(&external.raw_ansi.path, b"tampered").expect("tamper raw PTY artifact");
    let target = Fixture::new("normal", "normal", "normal");

    // act: the cached reference is validated against its artifact bytes.
    let error = run_compare_with_cached_reference(&scenario, &target.config, Some(cached))
        .expect_err("artifact drift must invalidate cache");

    // assert: rehashing rejects the stale artifact before comparison.
    assert!(
        matches!(error, RunnerError::BinaryReceipt { detail, .. } if detail.contains("artifact hash changed"))
    );
}

#[test]
fn packet1_complete_receipt_passes_all_gates() {
    // arrange: a complete dual-runtime receipt captured from two real PTY fixture processes.
    let fixture = packet1_fixture("complete");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    let receipt = run_compare(&scenario, &fixture.config).expect("complete receipt");

    // act: the production comparator evaluates the persisted receipt again.
    let comparison = compare_capture(&scenario, &receipt, &clean_cleanup());

    // assert: presentation, timing, and ordered motion are required passing gates with metrics.
    assert!(comparison.comparison_passed);
    assert!(comparison.presentation.is_some());
    for gate in ["presentation", "timing", "motion"] {
        assert!(comparison.gates[gate].passed, "gate {gate}");
    }
    write_packet1_artifact("complete-comparison.json", &comparison);
}

#[test]
fn packet1_provenance_rejects_every_bound_identity_mismatch() {
    // arrange: one real capture reduced to the evidence consumed by provenance.
    let fixture = packet1_fixture("provenance-mismatch");
    let scenario = Scenario::from_json(STARTUP_SMOKE).expect("scenario");
    let receipt = provenance_only_receipt(
        run_capture(&scenario, &fixture.config).expect("complete source-guarded capture"),
    );
    let cleanup = clean_cleanup();
    let baseline = compare_capture(&scenario, &receipt, &cleanup);
    assert!(
        baseline.gates["provenance"].passed,
        "unmodified provenance must pass: {:?}",
        baseline.gates["provenance"]
    );

    let mut cases = Vec::new();
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.schema_version.push('x')
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.receipt_kind = CandidateReceiptKind::DiagnosticNonRelease;
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.parity_acceptance_eligible = false;
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.release_eligible = false;
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.clean_release = false;
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.repository.clean = false
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding
            .repository
            .canonical_path
            .push("foreign");
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.repository.head.push('0')
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.repository.tree.push('0')
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding
            .repository
            .tracked_source_sha256
            .push('0');
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding
            .repository
            .dirty_diff_sha256
            .push('0');
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding
            .repository
            .untracked_manifest_sha256
            .push('0');
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding
            .repository
            .cargo_lock_sha256
            .push('0');
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.repository.toolchain_sha256.push('0');
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.repository.cargo_config_sha256 = Some("0".repeat(64));
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.binaries.harness_sha256.push('0')
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.binaries.runner_sha256.push('0')
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.binaries.aggregate_sha256.push('0')
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.target_dir.push("foreign")
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.authority.revision.push('0')
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.authority.sha256.push('0')
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.reference_receipt.sha256.push('0')
    }));
    cases.push(mutated(&receipt, |item| {
        item.candidate_binding.source_guard_receipt_sha256.push('0');
    }));
    let altered_guard = fixture.root().join("altered-source-guard.json");
    std::fs::write(&altered_guard, b"{}\n").expect("altered source guard");
    cases.push(mutated(&receipt, |item| {
        item.source_guard_after.path = altered_guard.display().to_string();
        item.source_guard_after.sha256 = RuntimeBinary::from_path(&altered_guard, "guard")
            .expect("altered guard digest")
            .sha256;
    }));

    // act: the comparator evaluates each independently forged binding dimension.
    let verdicts = cases
        .iter()
        .map(|item| compare_capture(&scenario, item, &cleanup))
        .collect::<Vec<_>>();
    write_packet1_artifact(
        "provenance-mismatch-results.json",
        &serde_json::json!({
            "baseline_provenance_passed": baseline.gates["provenance"].passed,
            "case_count": verdicts.len(),
            "rejected_case_count": verdicts
                .iter()
                .filter(|comparison| !comparison.gates["provenance"].passed)
                .count(),
        }),
    );

    // assert: every mutation fails specifically at the provenance gate.
    assert!(verdicts.iter().all(|comparison| {
        !comparison.comparison_passed && !comparison.gates["provenance"].passed
    }));
}

fn mutated(
    receipt: &harness_testkit::tui_fidelity_runner::DualRuntimeReceipt,
    change: impl FnOnce(&mut harness_testkit::tui_fidelity_runner::DualRuntimeReceipt),
) -> harness_testkit::tui_fidelity_runner::DualRuntimeReceipt {
    let mut changed = receipt.clone();
    change(&mut changed);
    changed
}

fn receipt_after_cached_reference_validation(
    result: Result<harness_testkit::tui_fidelity_runner::DualRuntimeReceipt, RunnerError>,
    evidence_dir: &std::path::Path,
) -> harness_testkit::tui_fidelity_runner::DualRuntimeReceipt {
    match result {
        Ok(receipt) => receipt,
        Err(RunnerError::Comparison { .. }) => serde_json::from_slice(
            &std::fs::read(evidence_dir.join("receipt.json")).expect("persisted receipt"),
        )
        .expect("persisted receipt JSON"),
        Err(error) => panic!("cached reference did not reach comparison: {error}"),
    }
}

fn provenance_only_receipt(
    mut receipt: harness_testkit::tui_fidelity_runner::DualRuntimeReceipt,
) -> harness_testkit::tui_fidelity_runner::DualRuntimeReceipt {
    for runtime in &mut receipt.runtimes {
        runtime.checkpoints.clear();
        let external = match &mut runtime.presentation {
            PresentationEvidence::ExternalOnly { external }
            | PresentationEvidence::HarnessNative { external, .. } => external,
        };
        external.action_receipts.clear();
        external.actual_input_sends.clear();
        external.raw_reads.clear();
        external.observations.clear();
        external.interaction_observations.clear();
    }
    receipt
}

#[path = "support/tui_fidelity_runner_packet_support.rs"]
mod runner_packet;

use runner_packet::{clean_cleanup, packet1_fixture, write_packet1_artifact};
