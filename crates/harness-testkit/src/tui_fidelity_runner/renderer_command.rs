use std::path::Path;
use std::process::Command;

use super::bounded_command::{self, BoundedFailureKind};
use super::cleanup::CleanupTracker;
use super::error::RunnerError;
use super::process::CapturedCheckpoint;
use super::types::{ArtifactDigest, RendererConfig, RunnerTiming};
use super::util::sha256_file_tracked;

pub(super) struct RendererInvocation<'a> {
    pub config: &'a RendererConfig,
    pub timing: RunnerTiming,
    pub checkpoint: &'a CapturedCheckpoint,
    pub input_path: &'a Path,
    pub root: &'a Path,
}

pub(super) fn invoke(
    invocation: RendererInvocation<'_>,
    tracker: &mut CleanupTracker,
) -> Result<(), RunnerError> {
    let RendererInvocation {
        config,
        timing,
        checkpoint,
        input_path,
        root,
    } = invocation;
    let mut command = Command::new(&config.node_program);
    command
        .arg(&config.script)
        .args(["--title", "TUI fidelity checkpoint", "--from-file"])
        .arg(input_path)
        .arg("--evidence-dir")
        .arg(root)
        .args(["--cols", &checkpoint.viewport.cols.to_string()])
        .args(["--rows", &checkpoint.viewport.rows.to_string()])
        .arg("--chrome-bin")
        .arg(&config.browser_program)
        .args(["--font-family", &config.font_family]);
    if let Some(node_modules) = &config.node_modules {
        command.env("NODE_PATH", node_modules);
        command.env("TUI_FIDELITY_NODE_MODULES", node_modules);
    }
    let output = bounded_command::run(
        &mut command,
        timing.scenario_timeout,
        timing.cleanup_timeout,
    )
    .map_err(|failure| {
        tracker.record_process(failure.cleanup);
        if matches!(failure.kind, BoundedFailureKind::Timeout) {
            RunnerError::RendererTimeout {
                checkpoint: checkpoint.name,
            }
        } else {
            RunnerError::Renderer {
                checkpoint: checkpoint.name,
                detail: failure.detail,
            }
        }
    })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(RunnerError::Renderer {
            checkpoint: checkpoint.name,
            detail: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

pub(super) fn artifact_digest(
    path: &Path,
    tracker: &mut CleanupTracker,
) -> Result<ArtifactDigest, RunnerError> {
    Ok(ArtifactDigest {
        path: path.display().to_string(),
        sha256: sha256_file_tracked(path, tracker)?,
    })
}
