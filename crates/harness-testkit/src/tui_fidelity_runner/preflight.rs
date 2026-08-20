use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::bounded_command::{self, BoundedFailureKind};
use super::cleanup::CleanupTracker;
use super::error::RunnerError;
use super::types::{CandidateReceiptKind, RunnerConfig, RuntimeBinary};
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
    if config.candidate_binding.schema_version != "harness.tui-fidelity.candidate-binding.v2"
        || !candidate_binding_kind_is_coherent(
            config.candidate_binding.receipt_kind,
            config.candidate_binding.parity_acceptance_eligible,
            config.candidate_binding.release_eligible,
            config.candidate_binding.clean_release,
            config.candidate_binding.repository.clean,
        )
    {
        return Err(RunnerError::CandidateBinding {
            path: path.clone(),
            detail: "candidate binding is not a coherent release or diagnostic v2 receipt"
                .to_owned(),
        });
    }
    validate_candidate_target(
        path,
        &config.candidate_binding.target_dir,
        &config.repo_root.join("target"),
    )
    .map_err(|detail| RunnerError::CandidateBinding {
        path: path.clone(),
        detail,
    })?;
    if config.harness.source_revision != config.candidate_binding.repository.head {
        return Err(RunnerError::CandidateBinding {
            path: path.clone(),
            detail: format!(
                "candidate SHA {} does not match binary source revision {}",
                config.candidate_binding.repository.head, config.harness.source_revision
            ),
        });
    }
    if config.harness.sha256 != config.candidate_binding.binaries.harness_sha256 {
        return Err(RunnerError::CandidateBinding {
            path: path.clone(),
            detail: format!(
                "candidate digest {} does not match binary digest {}",
                config.candidate_binding.binaries.harness_sha256, config.harness.sha256
            ),
        });
    }
    if config.candidate_binding.binaries.runner_sha256.len() != 64
        || config.candidate_binding.binaries.aggregate_sha256.len() != 64
    {
        return Err(RunnerError::CandidateBinding {
            path: path.clone(),
            detail: "runner and aggregate SHA-256 values must be 64-character digests".to_owned(),
        });
    }
    Ok(())
}

fn validate_candidate_target(
    candidate_path: &Path,
    binding_target: &Path,
    worktree_target: &Path,
) -> Result<(), String> {
    let canonical_path = std::fs::canonicalize(candidate_path)
        .map_err(|error| format!("cannot resolve candidate path: {error}"))?;
    let canonical_binding_target = std::fs::canonicalize(binding_target)
        .map_err(|error| format!("cannot resolve candidate target_dir: {error}"))?;
    if canonical_binding_target != binding_target {
        return Err("candidate target_dir must be an existing canonical path".to_owned());
    }
    let canonical_worktree_target = std::fs::canonicalize(worktree_target)
        .map_err(|error| format!("cannot resolve worktree target directory: {error}"))?;
    let canonical_target = canonical_path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "candidate path is not target/<profile>/debug/harness".to_owned())?;
    if !canonical_path.starts_with(&canonical_worktree_target)
        || !canonical_binding_target.starts_with(&canonical_worktree_target)
        || canonical_target != canonical_binding_target
    {
        return Err(format!(
            "candidate must be built under {} with target_dir {}, got {}",
            canonical_worktree_target.display(),
            canonical_binding_target.display(),
            canonical_target.display()
        ));
    }
    Ok(())
}

const fn candidate_binding_kind_is_coherent(
    receipt_kind: CandidateReceiptKind,
    parity_acceptance_eligible: bool,
    release_eligible: bool,
    clean_release: bool,
    repository_clean: bool,
) -> bool {
    match receipt_kind {
        CandidateReceiptKind::Release => {
            parity_acceptance_eligible && release_eligible && clean_release && repository_clean
        }
        CandidateReceiptKind::DiagnosticNonRelease => {
            parity_acceptance_eligible && !release_eligible && !clean_release && !repository_clean
        }
        CandidateReceiptKind::Fixture => {
            !parity_acceptance_eligible && !release_eligible && !clean_release
        }
    }
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use super::{candidate_binding_kind_is_coherent, validate_candidate_target};
    use crate::tui_fidelity_runner::CandidateReceiptKind;

    fn candidate_layout() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let root = tempfile::tempdir().expect("candidate root");
        let target = root.path().join("target");
        let candidate_target = target.join("candidate");
        let binary = candidate_target.join("debug/harness");
        fs::create_dir_all(binary.parent().expect("binary parent")).expect("candidate layout");
        fs::write(&binary, b"candidate").expect("candidate binary");
        (root, candidate_target, binary)
    }

    #[test]
    fn diagnostic_binding_can_accept_parity_without_becoming_release_eligible() {
        // arrange
        let release = CandidateReceiptKind::Release;
        let diagnostic = CandidateReceiptKind::DiagnosticNonRelease;

        // act
        let valid_release = candidate_binding_kind_is_coherent(release, true, true, true, true);
        let valid_diagnostic =
            candidate_binding_kind_is_coherent(diagnostic, true, false, false, false);
        let promoted_diagnostic =
            candidate_binding_kind_is_coherent(diagnostic, true, true, false, false);
        let dirty_release = candidate_binding_kind_is_coherent(release, true, true, true, false);

        // assert
        assert!(valid_release);
        assert!(valid_diagnostic);
        assert!(!promoted_diagnostic);
        assert!(!dirty_release);
    }

    #[test]
    fn canonical_candidate_target_is_accepted() {
        // arrange
        let (root, target, binary) = candidate_layout();

        // act
        let result = validate_candidate_target(&binary, &target, &root.path().join("target"));

        // assert
        assert!(result.is_ok(), "canonical target must validate: {result:?}");
    }

    #[test]
    fn lexical_parent_target_is_rejected() {
        // arrange
        let (root, target, binary) = candidate_layout();
        let nested = target.join("nested");
        fs::create_dir(&nested).expect("nested target");
        let lexical_target = nested.join("..");

        // act
        let result =
            validate_candidate_target(&binary, &lexical_target, &root.path().join("target"));

        // assert
        assert!(result.is_err(), "lexical target must fail closed");
    }

    #[test]
    fn symlink_target_escape_is_rejected() {
        // arrange
        let (root, _target, binary) = candidate_layout();
        let outside = tempfile::tempdir().expect("outside target");
        let escaped_target = root.path().join("target/escaped");
        symlink(outside.path(), &escaped_target).expect("escaped target symlink");

        // act
        let result =
            validate_candidate_target(&binary, &escaped_target, &root.path().join("target"));

        // assert
        assert!(result.is_err(), "symlink escape must fail closed");
    }

    #[test]
    fn nonexistent_target_is_rejected() {
        // arrange
        let (root, _target, binary) = candidate_layout();
        let missing = root.path().join("target/missing");

        // act
        let result = validate_candidate_target(&binary, &missing, &root.path().join("target"));

        // assert
        assert!(result.is_err(), "nonexistent target must fail closed");
    }

    #[test]
    fn wrong_canonical_target_is_rejected() {
        // arrange
        let (root, _target, binary) = candidate_layout();
        let wrong = root.path().join("target/wrong");
        fs::create_dir(&wrong).expect("wrong target");

        // act
        let result = validate_candidate_target(&binary, &wrong, &root.path().join("target"));

        // assert
        assert!(result.is_err(), "wrong canonical target must fail closed");
    }
}
