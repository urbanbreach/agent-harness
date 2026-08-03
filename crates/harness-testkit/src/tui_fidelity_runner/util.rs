use std::fs;
use std::path::Path;
use std::process::Command;

use serde::Serialize;

use super::error::RunnerError;

pub(super) fn sha256_file(path: &Path) -> Result<String, RunnerError> {
    if !path.is_file() {
        return Err(RunnerError::Io {
            path: path.to_path_buf(),
            detail: "file does not exist".to_owned(),
        });
    }
    let output = Command::new("sha256sum")
        .arg("--")
        .arg(path)
        .output()
        .map_err(|error| RunnerError::Io {
            path: path.to_path_buf(),
            detail: error.to_string(),
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
