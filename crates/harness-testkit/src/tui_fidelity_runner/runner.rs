use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::error::RunnerError;
use super::process::execute;
use super::renderer::render;
use super::runtime_workspace::{cleanup_temporary_paths, runtime_dir};
use super::types::{
    AdapterReceipt, ArtifactDigest, CleanupReceipt, DualRuntimeReceipt, RunnerConfig, RuntimeBinary,
};
use super::util::{sha256_file, write_json};
use super::RUNNER_RECEIPT_SCHEMA;
use crate::tui_fidelity::{AdapterKind, Scenario};

pub fn run_compare(
    scenario: &Scenario,
    config: &RunnerConfig,
) -> Result<DualRuntimeReceipt, RunnerError> {
    prepare(scenario, config)?;
    let result = run_inner(scenario, config);
    let temporary_paths_removed = cleanup_temporary_paths(scenario, config)?;
    let cleanup = CleanupReceipt {
        schema_version: "harness.tui-fidelity.cleanup.v1".to_owned(),
        status: if result.is_ok() { "clean" } else { "error" }.to_owned(),
        forced_termination_observed: matches!(result, Err(RunnerError::ForcedKillOnly { .. })),
        surviving_pids: Vec::new(),
        temporary_paths_removed,
    };
    write_json(&config.evidence_dir.join("cleanup.json"), &cleanup)?;
    result
}

fn run_inner(
    scenario: &Scenario,
    config: &RunnerConfig,
) -> Result<DualRuntimeReceipt, RunnerError> {
    let before = run_source_guard(config, "source-guard-before.json")?;
    let reference_runtime = runtime_dir(scenario, config, AdapterKind::Grok)?;
    let reference_result = execute(
        scenario,
        config.timing,
        AdapterKind::Grok,
        &config.reference,
        &reference_runtime,
    );
    let after = run_source_guard(config, "source-guard-after.json")?;
    let reference_capture = reference_result?;
    let harness_runtime = runtime_dir(scenario, config, AdapterKind::Harness)?;
    let harness_capture = execute(
        scenario,
        config.timing,
        AdapterKind::Harness,
        &config.harness,
        &harness_runtime,
    )?;
    let reference_checkpoints = render(
        AdapterKind::Grok,
        &reference_capture,
        &config.renderer,
        &config.evidence_dir,
    )?;
    let harness_checkpoints = render(
        AdapterKind::Harness,
        &harness_capture,
        &config.renderer,
        &config.evidence_dir,
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

fn prepare(scenario: &Scenario, config: &RunnerConfig) -> Result<(), RunnerError> {
    scenario.validate_for_adapter(AdapterKind::Grok)?;
    scenario.validate_for_adapter(AdapterKind::Harness)?;
    if config.evidence_dir.exists()
        && fs::read_dir(&config.evidence_dir)
            .map_err(|error| io_error(&config.evidence_dir, error))?
            .next()
            .is_some()
    {
        return Err(RunnerError::StaleEvidence {
            path: config.evidence_dir.clone(),
        });
    }
    fs::create_dir_all(&config.evidence_dir)
        .map_err(|error| io_error(&config.evidence_dir, error))?;
    validate_binary(AdapterKind::Grok, &config.reference)?;
    validate_binary(AdapterKind::Harness, &config.harness)?;
    if config.reference.sha256 == config.harness.sha256 {
        return Err(RunnerError::SelfComparison {
            sha256: config.reference.sha256.clone(),
        });
    }
    if !is_executable(&config.renderer.browser_program) {
        return Err(RunnerError::MissingBrowser {
            path: config.renderer.browser_program.clone(),
        });
    }
    validate_font(&config.renderer.font_family)
}

fn validate_binary(adapter: AdapterKind, binary: &RuntimeBinary) -> Result<(), RunnerError> {
    if !is_executable(&binary.path) {
        return Err(RunnerError::MissingBinary {
            adapter,
            path: binary.path.clone(),
        });
    }
    let actual = sha256_file(&binary.path)?;
    if actual == binary.sha256 {
        Ok(())
    } else {
        Err(RunnerError::BinaryDigest {
            path: binary.path.clone(),
            expected: binary.sha256.clone(),
            actual,
        })
    }
}

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

fn validate_font(family: &str) -> Result<(), RunnerError> {
    let output = Command::new("fc-list")
        .arg("--format=%{family}\n")
        .output()
        .map_err(|error| RunnerError::MissingFont {
            family: format!("{family}: {error}"),
        })?;
    let found = output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .lines()
            .flat_map(|line| line.split(','))
            .any(|candidate| candidate.trim() == family);
    if found {
        Ok(())
    } else {
        Err(RunnerError::MissingFont {
            family: family.to_owned(),
        })
    }
}

fn run_source_guard(config: &RunnerConfig, name: &str) -> Result<ArtifactDigest, RunnerError> {
    let path = config.evidence_dir.join(name);
    let output = Command::new(&config.source_guard.program)
        .args(["verify", "--reference"])
        .arg(&config.source_guard.reference_root)
        .args(["--revision", &config.source_guard.revision, "--receipt"])
        .arg(&path)
        .current_dir(&config.repo_root)
        .output()
        .map_err(|error| RunnerError::SourceGuard {
            detail: error.to_string(),
        })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return if detail.contains("dirty reference source") {
            Err(RunnerError::DirtyReference { detail })
        } else {
            Err(RunnerError::SourceGuard { detail })
        };
    }
    Ok(ArtifactDigest {
        path: path.display().to_string(),
        sha256: sha256_file(&path)?,
    })
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

fn io_error(path: &Path, error: impl std::fmt::Display) -> RunnerError {
    RunnerError::Io {
        path: PathBuf::from(path),
        detail: error.to_string(),
    }
}
