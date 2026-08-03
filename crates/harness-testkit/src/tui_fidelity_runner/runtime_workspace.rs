use std::fs;
use std::path::PathBuf;

use super::error::RunnerError;
use super::types::RunnerConfig;
use crate::tui_fidelity::{AdapterKind, Scenario};

pub(super) fn runtime_dir(
    scenario: &Scenario,
    config: &RunnerConfig,
    adapter: AdapterKind,
) -> Result<PathBuf, RunnerError> {
    let temporary_root = scenario.cleanup.temporary_paths.first().map_or_else(
        || config.repo_root.join("tmp/tui-fidelity"),
        |path| config.repo_root.join(path),
    );
    let adapter_name = match adapter {
        AdapterKind::Grok => "grok",
        AdapterKind::Harness => "harness",
    };
    let path = temporary_root.join(&scenario.id.0).join(adapter_name);
    fs::create_dir_all(&path).map_err(|error| io_error(&path, error))?;
    Ok(path)
}

pub(super) fn cleanup_temporary_paths(
    scenario: &Scenario,
    config: &RunnerConfig,
) -> Result<Vec<String>, RunnerError> {
    let mut removed = Vec::new();
    for relative in &scenario.cleanup.temporary_paths {
        let path = config.repo_root.join(relative);
        if path.exists() {
            fs::remove_dir_all(&path).map_err(|error| io_error(&path, error))?;
        }
        removed.push(relative.clone());
    }
    Ok(removed)
}

fn io_error(path: &std::path::Path, error: impl std::fmt::Display) -> RunnerError {
    RunnerError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    }
}
