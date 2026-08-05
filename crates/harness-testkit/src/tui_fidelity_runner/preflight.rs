use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::bounded_command::{self, BoundedFailureKind};
use super::cleanup::CleanupTracker;
use super::error::RunnerError;
use super::types::{RunnerConfig, RuntimeBinary};
use super::util::sha256_file_tracked;
use crate::tui_fidelity::{AdapterKind, Scenario};

pub(super) fn prepare(
    scenario: &Scenario,
    config: &RunnerConfig,
    tracker: &mut CleanupTracker,
) -> Result<(), RunnerError> {
    scenario.validate_for_adapter(AdapterKind::Grok)?;
    scenario.validate_for_adapter(AdapterKind::Harness)?;
    if config.reference.sha256 == config.harness.sha256 {
        return Err(RunnerError::SelfComparison {
            sha256: config.reference.sha256.clone(),
        });
    }
    validate_binary(AdapterKind::Grok, &config.reference, tracker)?;
    validate_binary(AdapterKind::Harness, &config.harness, tracker)?;
    validate_candidate_binding(config)?;
    if !is_executable(&config.renderer.browser_program) {
        return Err(RunnerError::MissingBrowser {
            path: config.renderer.browser_program.clone(),
        });
    }
    validate_font(&config.renderer.font_family, tracker)
}

fn validate_candidate_binding(config: &RunnerConfig) -> Result<(), RunnerError> {
    let path = &config.harness.path;
    let worktree_target = config.repo_root.join("target");
    let canonical_path =
        std::fs::canonicalize(path).map_err(|error| RunnerError::CandidateBinding {
            path: path.clone(),
            detail: format!("cannot resolve candidate path: {error}"),
        })?;
    let canonical_target = canonical_path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| RunnerError::CandidateBinding {
            path: path.clone(),
            detail: "candidate path is not target/<profile>/debug/harness".to_owned(),
        })?;
    let canonical_worktree_target =
        std::fs::canonicalize(&worktree_target).unwrap_or(worktree_target);
    if !canonical_path.starts_with(&canonical_worktree_target)
        || canonical_target != config.candidate_binding.target_dir
    {
        return Err(RunnerError::CandidateBinding {
            path: path.clone(),
            detail: format!(
                "candidate must be built under {} with target_dir {}, got {}",
                canonical_worktree_target.display(),
                config.candidate_binding.target_dir.display(),
                canonical_target.display()
            ),
        });
    }
    if config.harness.source_revision != config.candidate_binding.candidate_sha {
        return Err(RunnerError::CandidateBinding {
            path: path.clone(),
            detail: format!(
                "candidate SHA {} does not match binary source revision {}",
                config.candidate_binding.candidate_sha, config.harness.source_revision
            ),
        });
    }
    if config.harness.sha256 != config.candidate_binding.candidate_binary_sha256 {
        return Err(RunnerError::CandidateBinding {
            path: path.clone(),
            detail: format!(
                "candidate digest {} does not match binary digest {}",
                config.candidate_binding.candidate_binary_sha256, config.harness.sha256
            ),
        });
    }
    if config.candidate_binding.runner_sha256.len() != 64 {
        return Err(RunnerError::CandidateBinding {
            path: path.clone(),
            detail: "runner SHA-256 must be a 64-character digest".to_owned(),
        });
    }
    Ok(())
}

fn validate_binary(
    adapter: AdapterKind,
    binary: &RuntimeBinary,
    tracker: &mut CleanupTracker,
) -> Result<(), RunnerError> {
    if !is_executable(&binary.path) {
        return Err(RunnerError::MissingBinary {
            adapter,
            path: binary.path.clone(),
        });
    }
    let actual = sha256_file_tracked(&binary.path, tracker)?;
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

fn validate_font(family: &str, tracker: &mut CleanupTracker) -> Result<(), RunnerError> {
    let mut command = Command::new("fc-list");
    command.arg("--format=%{family}\n");
    let output = bounded_command::run(&mut command, Duration::from_secs(5), Duration::from_secs(2))
        .map_err(|failure| {
            tracker.record_process(failure.cleanup);
            if matches!(failure.kind, BoundedFailureKind::Timeout) {
                RunnerError::ExternalCommandTimeout {
                    command: "fc-list".to_owned(),
                }
            } else {
                RunnerError::MissingFont {
                    family: format!("{family}: {}", failure.detail),
                }
            }
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
