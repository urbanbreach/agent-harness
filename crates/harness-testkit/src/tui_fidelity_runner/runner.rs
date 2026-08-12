use super::cleanup::{CleanupTracker, EvidenceSession};
use super::error::RunnerError;
use super::preflight::prepare;
use super::process::execute;
use super::receipt_presentation;
use super::renderer::{render, RenderContext};
use super::runtime_workspace::OwnedRuntimeWorkspace;
use super::source_guard;
use super::types::{AdapterReceipt, DualRuntimeReceipt, RunnerConfig, RuntimeBinary};
use super::util::write_json;
use super::RUNNER_RECEIPT_SCHEMA;
use crate::tui_fidelity::{AdapterKind, Scenario};
use crate::tui_fidelity_compare::{compare_capture_with_profile, AcceptanceProfile};

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
    run_compare_with_cached_reference_and_profile(
        scenario,
        config,
        cached_reference,
        AcceptanceProfile::FullParity,
    )
}

pub fn run_compare_with_cached_reference_and_profile(
    scenario: &Scenario,
    config: &RunnerConfig,
    cached_reference: Option<AdapterReceipt>,
    profile: AcceptanceProfile,
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
        let comparison = compare_capture_with_profile(scenario, &receipt, &cleanup, profile);
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
        Some(receipt) => validate_cached_reference(receipt, scenario, config)?,
        None => capture_adapter(scenario, config, workspace, tracker, AdapterKind::Grok)?,
    };
    let after = source_guard::run(config, "source-guard-after.json", tracker)?;
    let harness_receipt =
        capture_adapter(scenario, config, workspace, tracker, AdapterKind::Harness)?;
    Ok(DualRuntimeReceipt {
        schema_version: RUNNER_RECEIPT_SCHEMA.to_owned(),
        scenario_id: scenario.id.0.clone(),
        terminal_type: scenario.terminal_type.as_str().to_owned(),
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
    let evidence_dir = config.evidence_dir.join(adapter.as_str());
    let fixture = if scenario.id.0 == "packet2-sustained-stream" {
        Some(
            crate::tui_fidelity_fixture::Packet2FixtureServer::start().map_err(|error| {
                RunnerError::Process {
                    adapter,
                    detail: format!("start Packet 2 fixture: {error}"),
                }
            })?,
        )
    } else {
        None
    };
    let fixture_base_url = fixture
        .as_ref()
        .map(crate::tui_fidelity_fixture::Packet2FixtureServer::base_url);
    let capture_result = execute(
        scenario,
        config.timing,
        adapter,
        binary,
        &runtime,
        &evidence_dir,
        tracker,
        fixture_base_url.as_deref(),
    );
    let fixture_result = fixture.map(|fixture| fixture.finish());
    if let Some(result) = fixture_result {
        let trace = result.map_err(|error| RunnerError::Process {
            adapter,
            detail: format!("finish Packet 2 fixture: {error}"),
        })?;
        write_json(&evidence_dir.join("packet2-fixture.json"), &trace)?;
    }
    let capture = capture_result?;
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
    adapter_receipt(
        scenario,
        adapter,
        binary,
        capture,
        checkpoints,
        &evidence_dir,
    )
}

fn validate_cached_reference(
    receipt: AdapterReceipt,
    scenario: &Scenario,
    config: &RunnerConfig,
) -> Result<AdapterReceipt, RunnerError> {
    let expected_actions = binding_hash(&scenario.actions)?;
    let expected_motion = binding_hash(&scenario.motion_capture)?;
    let binding = &receipt.presentation_binding;
    let exact_identity = receipt.adapter == AdapterKind::Grok
        && receipt.binary.sha256 == config.reference.sha256
        && receipt.binary.source_revision == config.reference.source_revision
        && binding.receipt_schema == RUNNER_RECEIPT_SCHEMA
        && binding.scenario_id == scenario.id.0
        && binding.action_schedule_sha256 == expected_actions
        && binding.motion_contract_sha256 == expected_motion
        && binding.observer_version == receipt_presentation::PTY_OBSERVER_VERSION
        && binding.terminal_identity == scenario.terminal_type.as_str();
    if !exact_identity {
        return Err(RunnerError::BinaryReceipt {
            path: config.reference.path.clone(),
            detail: "cached reference presentation identity is stale or incomplete".to_owned(),
        });
    }
    super::presentation_validation::validate_presentation_evidence(
        AdapterKind::Grok,
        &receipt.presentation,
    )
    .map_err(|error| RunnerError::BinaryReceipt {
        path: config.reference.path.clone(),
        detail: error.to_string(),
    })?;
    rehash_presentation_artifacts(&receipt)?;
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
    scenario: &Scenario,
    adapter: AdapterKind,
    binary: &RuntimeBinary,
    capture: super::process::ProcessCapture,
    checkpoints: Vec<super::types::CheckpointReceipt>,
    evidence_dir: &std::path::Path,
) -> Result<AdapterReceipt, RunnerError> {
    let (presentation, presentation_binding) =
        receipt_presentation::build(scenario, adapter, evidence_dir, &capture)?;
    super::presentation_validation::validate_presentation_evidence(adapter, &presentation)
        .map_err(|error| RunnerError::Process {
            adapter,
            detail: format!("presentation evidence: {error}"),
        })?;
    if scenario.id.0 == "packet2-sustained-stream" {
        let external = match &presentation {
            super::presentation_receipt::PresentationEvidence::ExternalOnly { external }
            | super::presentation_receipt::PresentationEvidence::HarnessNative {
                external, ..
            } => external,
        };
        super::presentation_validation::validate_packet2_disclosure(external).map_err(|error| {
            RunnerError::Process {
                adapter,
                detail: format!("Packet 2 disclosure evidence: {error}"),
            }
        })?;
    }
    Ok(AdapterReceipt {
        adapter,
        binary: binary.clone(),
        normal_exit_code: capture.exit_code,
        input_timestamps_millis: capture
            .input_timestamps
            .iter()
            .map(std::time::Duration::as_millis)
            .collect(),
        checkpoints,
        presentation,
        presentation_binding,
    })
}

fn binding_hash(value: &impl serde::Serialize) -> Result<String, RunnerError> {
    use sha2::Digest as _;
    let bytes = serde_json::to_vec(value).map_err(|error| RunnerError::Arguments {
        detail: format!("cached presentation binding: {error}"),
    })?;
    Ok(sha2::Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            use std::fmt::Write as _;
            let _ = write!(output, "{byte:02x}");
            output
        }))
}

fn rehash_presentation_artifacts(receipt: &AdapterReceipt) -> Result<(), RunnerError> {
    let external = match &receipt.presentation {
        super::presentation_receipt::PresentationEvidence::ExternalOnly { external } => external,
        super::presentation_receipt::PresentationEvidence::HarnessNative { .. } => {
            return Err(RunnerError::BinaryReceipt {
                path: receipt.binary.path.clone(),
                detail: "cached reference cannot contain Harness-native evidence".to_owned(),
            });
        }
    };
    for artifact in [&external.raw_ansi, &external.observations_artifact] {
        let observed = super::util::sha256_file(std::path::Path::new(&artifact.path))?;
        if observed != artifact.sha256 {
            return Err(RunnerError::BinaryReceipt {
                path: artifact.path.clone().into(),
                detail: "cached presentation artifact hash changed".to_owned(),
            });
        }
    }
    Ok(())
}
