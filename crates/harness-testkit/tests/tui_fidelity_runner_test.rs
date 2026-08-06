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
use harness_testkit::tui_fidelity_runner::{RunnerError, run_compare};

use support::{Fixture, STARTUP_SMOKE};

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
    assert!(
        forced
            .config
            .evidence_dir
            .join("grok/terminal-ansi.txt")
            .is_file()
    );
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
