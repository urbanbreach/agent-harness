use super::cleanup::{CleanupTracker, EvidenceSession};
use super::error::RunnerError;
use super::preflight::prepare;
use super::process::execute;
use super::renderer::{render, RenderContext};
use super::runtime_workspace::OwnedRuntimeWorkspace;
use super::source_guard;
use super::types::{AdapterReceipt, DualRuntimeReceipt, RunnerConfig, RuntimeBinary};
use super::util::write_json;
use super::RUNNER_RECEIPT_SCHEMA;
use crate::tui_fidelity::{AdapterKind, Scenario};

pub fn run_compare(
    scenario: &Scenario,
    config: &RunnerConfig,
) -> Result<DualRuntimeReceipt, RunnerError> {
    let evidence = EvidenceSession::initialize(&config.evidence_dir)?;
    let mut tracker = CleanupTracker::default();
    if evidence.is_stale() {
        return finish(
            &evidence,
            &tracker,
            Err(RunnerError::StaleEvidence {
                path: evidence.directory().to_path_buf(),
            }),
        );
    }
    if let Err(error) = prepare(scenario, config, &mut tracker) {
        return finish(&evidence, &tracker, Err(error));
    }
    let relative_base = scenario
        .cleanup
        .temporary_paths
        .first()
        .map_or("tmp/tui-fidelity", String::as_str);
    let mut workspace =
        match OwnedRuntimeWorkspace::create(&config.repo_root, relative_base, &scenario.id.0) {
            Ok(workspace) => workspace,
            Err(error) => {
                tracker.record_error(error.to_string());
                return finish(&evidence, &tracker, Err(error));
            }
        };
    let result = run_inner(scenario, config, &workspace, &mut tracker);
    match workspace.cleanup() {
        Ok(path) => tracker.record_removed(&path),
        Err(error) => tracker.record_error(error.to_string()),
    }
    finish(&evidence, &tracker, result)
}

fn run_inner(
    scenario: &Scenario,
    config: &RunnerConfig,
    workspace: &OwnedRuntimeWorkspace,
    tracker: &mut CleanupTracker,
) -> Result<DualRuntimeReceipt, RunnerError> {
    let before = source_guard::run(config, "source-guard-before.json", tracker)?;
    let reference_runtime = workspace.adapter_dir(AdapterKind::Grok)?;
    let reference_result = execute(
        scenario,
        config.timing,
        AdapterKind::Grok,
        &config.reference,
        &reference_runtime,
        tracker,
    );
    let after = source_guard::run(config, "source-guard-after.json", tracker)?;
    let reference_capture = reference_result?;
    let harness_runtime = workspace.adapter_dir(AdapterKind::Harness)?;
    let harness_capture = execute(
        scenario,
        config.timing,
        AdapterKind::Harness,
        &config.harness,
        &harness_runtime,
        tracker,
    )?;
    let reference_checkpoints = render(
        AdapterKind::Grok,
        &reference_capture,
        &mut RenderContext {
            config: &config.renderer,
            timing: config.timing,
            evidence_root: &config.evidence_dir,
            tracker,
        },
    )?;
    let harness_checkpoints = render(
        AdapterKind::Harness,
        &harness_capture,
        &mut RenderContext {
            config: &config.renderer,
            timing: config.timing,
            evidence_root: &config.evidence_dir,
            tracker,
        },
    )?;
    let receipt = DualRuntimeReceipt {
        schema_version: RUNNER_RECEIPT_SCHEMA.to_owned(),
        scenario_id: scenario.id.0.clone(),
        terminal_type: "xterm-256color".to_owned(),
        runtimes: vec![
            adapter_receipt(
                AdapterKind::Grok,
                &config.reference,
                reference_capture,
                reference_checkpoints,
            ),
            adapter_receipt(
                AdapterKind::Harness,
                &config.harness,
                harness_capture,
                harness_checkpoints,
            ),
        ],
        source_guard_before: before,
        source_guard_after: after,
    };
    write_json(&config.evidence_dir.join("receipt.json"), &receipt)?;
    Ok(receipt)
}

fn finish<T>(
    evidence: &EvidenceSession,
    tracker: &CleanupTracker,
    result: Result<T, RunnerError>,
) -> Result<T, RunnerError> {
    let receipt = tracker.receipt(result.as_ref().err());
    if let Err(write_error) = evidence.write(&receipt) {
        return Err(RunnerError::Cleanup {
            primary: result.err().map(Box::new),
            detail: format!("cleanup receipt: {write_error}"),
        });
    }
    if tracker.has_errors() {
        return Err(RunnerError::Cleanup {
            primary: result.err().map(Box::new),
            detail: tracker.error_detail(),
        });
    }
    result
}

fn adapter_receipt(
    adapter: AdapterKind,
    binary: &RuntimeBinary,
    capture: super::process::ProcessCapture,
    checkpoints: Vec<super::types::CheckpointReceipt>,
) -> AdapterReceipt {
    AdapterReceipt {
        adapter,
        binary: binary.clone(),
        normal_exit_code: capture.exit_code,
        input_timestamps_millis: capture
            .input_timestamps
            .iter()
            .map(std::time::Duration::as_millis)
            .collect(),
        checkpoints,
    }
}
