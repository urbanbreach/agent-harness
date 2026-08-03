use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde::Serialize;

use super::bounded_command::{self, BoundedFailureKind};
use super::cleanup::CleanupTracker;
use super::error::RunnerError;

pub(super) fn sha256_file(path: &Path) -> Result<String, RunnerError> {
    sha256_file_inner(path, None)
}

pub(super) fn sha256_file_tracked(
    path: &Path,
    tracker: &mut CleanupTracker,
) -> Result<String, RunnerError> {
    sha256_file_inner(path, Some(tracker))
}

fn sha256_file_inner(
    path: &Path,
    tracker: Option<&mut CleanupTracker>,
) -> Result<String, RunnerError> {
    if !path.is_file() {
        return Err(RunnerError::Io {
            path: path.to_path_buf(),
            detail: "file does not exist".to_owned(),
        });
    }
    let mut command = Command::new("sha256sum");
    command.arg("--").arg(path);
    let output = bounded_command::run(&mut command, Duration::from_secs(5), Duration::from_secs(2))
        .map_err(|failure| {
            if let Some(tracker) = tracker {
                tracker.record_process(failure.cleanup);
            }
            if matches!(failure.kind, BoundedFailureKind::Timeout) {
                RunnerError::ExternalCommandTimeout {
                    command: "sha256sum".to_owned(),
                }
            } else {
                RunnerError::Io {
                    path: path.to_path_buf(),
                    detail: failure.detail,
                }
            }
        })?;
    if !output.status.success() {
        return Err(RunnerError::Io {
            path: path.to_path_buf(),
            detail: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| RunnerError::Io {
            path: path.to_path_buf(),
            detail: "sha256sum returned no digest".to_owned(),
        })
}

pub(super) fn write_json(path: &Path, value: &impl Serialize) -> Result<(), RunnerError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| RunnerError::Io {
            path: parent.to_path_buf(),
            detail: error.to_string(),
        })?;
    }
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| RunnerError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(|error| RunnerError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })
}

pub(super) fn write_json_new(path: &Path, value: &impl Serialize) -> Result<(), RunnerError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| RunnerError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    bytes.push(b'\n');
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| RunnerError::Io {
            path: path.to_path_buf(),
            detail: error.to_string(),
        })?;
    file.write_all(&bytes).map_err(|error| RunnerError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })
}
