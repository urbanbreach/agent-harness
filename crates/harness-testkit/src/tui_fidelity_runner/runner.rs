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
use crate::tui_fidelity_compare::compare_capture;

pub fn run_compare(
    scenario: &Scenario,
    config: &RunnerConfig,
) -> Result<DualRuntimeReceipt, RunnerError> {
    run_compare_with_cached_reference(scenario, config, None)
}

pub fn run_compare_with_cached_reference(
    scenario: &Scenario,
    config: &RunnerConfig,
    cached_reference: Option<AdapterReceipt>,
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
    let result = run_inner(scenario, config, &workspace, &mut tracker, cached_reference);
    match workspace.cleanup() {
        Ok(path) => tracker.record_removed(&path),
        Err(error) => tracker.record_error(error.to_string()),
    }
    let result = result.and_then(|mut receipt| {
        let cleanup = tracker.receipt(None);
        let comparison = compare_capture(scenario, &receipt, &cleanup);
        write_json(&config.evidence_dir.join("comparison.json"), &comparison)?;
        receipt.comparison = Some(comparison.clone());
        write_json(&config.evidence_dir.join("receipt.json"), &receipt)?;
        if comparison.comparison_passed {
            Ok(receipt)
        } else {
            let detail = comparison
                .gates
                .iter()
                .filter(|(_, gate)| !gate.passed)
                .map(|(name, gate)| format!("{name}: {}", gate.detail))
                .collect::<Vec<_>>()
                .join("; ");
            Err(RunnerError::Comparison { detail })
        }
    });
    finish(&evidence, &tracker, result)
}

fn run_inner(
    scenario: &Scenario,
    config: &RunnerConfig,
    workspace: &OwnedRuntimeWorkspace,
    tracker: &mut CleanupTracker,
    cached_reference: Option<AdapterReceipt>,
) -> Result<DualRuntimeReceipt, RunnerError> {
    let before = source_guard::run(config, "source-guard-before.json", tracker)?;
    let reference_receipt = match cached_reference {
        Some(receipt) => validate_cached_reference(receipt, config)?,
        None => capture_adapter(scenario, config, workspace, tracker, AdapterKind::Grok)?,
    };
    let after = source_guard::run(config, "source-guard-after.json", tracker)?;
    let harness_receipt =
        capture_adapter(scenario, config, workspace, tracker, AdapterKind::Harness)?;
    Ok(DualRuntimeReceipt {
        schema_version: RUNNER_RECEIPT_SCHEMA.to_owned(),
        scenario_id: scenario.id.0.clone(),
        terminal_type: "xterm-256color".to_owned(),
        runtimes: vec![reference_receipt, harness_receipt],
        candidate_binding: config.candidate_binding.clone(),
        source_guard_before: before,
        source_guard_after: after,
        comparison: None,
    })
}

fn capture_adapter(
    scenario: &Scenario,
    config: &RunnerConfig,
    workspace: &OwnedRuntimeWorkspace,
    tracker: &mut CleanupTracker,
    adapter: AdapterKind,
) -> Result<AdapterReceipt, RunnerError> {
    let runtime = workspace.adapter_dir(adapter)?;
    let binary = match adapter {
        AdapterKind::Grok => &config.reference,
        AdapterKind::Harness => &config.harness,
    };
    let capture = execute(scenario, config.timing, adapter, binary, &runtime, tracker)?;
    let checkpoints = render(
        adapter,
        &capture,
        &mut RenderContext {
            config: &config.renderer,
            timing: config.timing,
            evidence_root: &config.evidence_dir,
            tracker,
        },
    )?;
    Ok(adapter_receipt(adapter, binary, capture, checkpoints))
}

fn validate_cached_reference(
    receipt: AdapterReceipt,
    config: &RunnerConfig,
) -> Result<AdapterReceipt, RunnerError> {
    if receipt.adapter == AdapterKind::Grok
        && receipt.binary.sha256 == config.reference.sha256
        && receipt.binary.source_revision == config.reference.source_revision
    {
        Ok(receipt)
    } else {
        Err(RunnerError::BinaryDigest {
            path: config.reference.path.clone(),
            expected: config.reference.sha256.clone(),
            actual: receipt.binary.sha256,
        })
    }
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
