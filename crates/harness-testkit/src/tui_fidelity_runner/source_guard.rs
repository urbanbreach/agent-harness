use std::process::Command;
use std::time::Duration;

use super::bounded_command::{self, BoundedFailureKind};
use super::cleanup::CleanupTracker;
use super::error::RunnerError;
use super::types::{ArtifactDigest, RunnerConfig};
use super::util::sha256_file_tracked;

pub(super) fn run(
    config: &RunnerConfig,
    name: &str,
    tracker: &mut CleanupTracker,
) -> Result<ArtifactDigest, RunnerError> {
    let path = config.evidence_dir.join(name);
    let mut command = Command::new(&config.source_guard.program);
    command
        .args(["verify", "--reference"])
        .arg(&config.source_guard.reference_root)
        .args(["--revision", &config.source_guard.revision, "--receipt"])
        .arg(&path)
        .current_dir(&config.repo_root);
    let output = bounded_command::run(
        &mut command,
        Duration::from_secs(10).max(config.timing.scenario_timeout),
        config.timing.cleanup_timeout,
    )
    .map_err(|failure| {
        tracker.record_process(failure.cleanup);
        if matches!(failure.kind, BoundedFailureKind::Timeout) {
            RunnerError::ExternalCommandTimeout {
                command: "source guard".to_owned(),
            }
        } else {
            RunnerError::SourceGuard {
                detail: failure.detail,
            }
        }
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
        sha256: sha256_file_tracked(&path, tracker)?,
    })
}
